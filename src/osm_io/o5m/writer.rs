use std::io;
use std::io::Write;

use super::*;
use crate::geo::{Boundary, Coordinate};
use crate::osm_io::error::Error;
use crate::osm_io::o5m::varint::WriteVarInt;
use crate::osm_io::o5m::Delta::{
    ChangeSet, Id, Lat, Lon, RelNodeRef, RelRelRef, RelWayRef, Time, WayRef,
};
use crate::osm_io::OsmWriter;
use crate::{Meta, Node, Osm, Relation, RelationMember, Way};

/// A writer for the o5m binary format.
#[derive(Debug)]
pub struct O5mWriter<W> {
    inner: W,
    encoder: O5mEncoder,
}

/// Encodes data into bytes according the o5m specification. Keeps track of string references and
/// delta values.
#[derive(Debug)]
struct O5mEncoder {
    string_table: StringReferenceTable,
    delta: DeltaState,
}

impl<W: Write> O5mWriter<W> {
    pub fn new(writer: W) -> O5mWriter<W> {
        O5mWriter {
            inner: writer,
            encoder: O5mEncoder::new(),
        }
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Reset
    fn reset(&mut self) -> io::Result<()> {
        self.inner.write_all(&[O5M_RESET])?;
        self.encoder.reset();
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Bounding_Box
    fn write_bounding_box(&mut self, boundary: &Boundary) -> Result<()> {
        let mut bytes = Vec::new();
        bytes.write_varint(boundary.min.lon)?;
        bytes.write_varint(boundary.min.lat)?;
        bytes.write_varint(boundary.max.lon)?;
        bytes.write_varint(boundary.max.lat)?;

        self.inner.write_all(&[O5M_BOUNDING_BOX])?;
        self.inner.write_varint(bytes.len() as u64)?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Node
    fn write_node(&mut self, node: &Node) -> Result<()> {
        let bytes = self.encoder.node_to_bytes(node)?;
        self.inner.write_all(&[O5M_NODE])?;
        self.inner.write_varint(bytes.len() as u64)?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Way
    fn write_way(&mut self, way: &Way) -> Result<()> {
        let bytes = self.encoder.way_to_bytes(way)?;
        self.inner.write_all(&[O5M_WAY])?;
        self.inner.write_varint(bytes.len() as u64)?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Relation
    fn write_relation(&mut self, rel: &Relation) -> Result<()> {
        let bytes = self.encoder.relation_to_bytes(rel)?;
        self.inner.write_all(&[O5M_RELATION])?;
        self.inner.write_varint(bytes.len() as u64)?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }
}

impl<W: Write> OsmWriter<W> for O5mWriter<W> {
    fn write(&mut self, osm: &Osm) -> std::result::Result<(), Error> {
        self.reset()?;
        self.inner.write_all(&[O5M_HEADER])?;
        self.inner.write_all(O5M_HEADER_DATA)?;

        if let Some(boundary) = &osm.boundary {
            self.write_bounding_box(&boundary)?;
        }

        self.reset()?;
        for node in &osm.nodes {
            self.write_node(&node)?;
        }

        self.reset()?;
        for way in &osm.ways {
            self.write_way(&way)?;
        }

        self.reset()?;
        for rel in &osm.relations {
            self.write_relation(&rel)?;
        }

        self.inner.write_all(&[O5M_EOF])?;
        Ok(())
    }

    fn into_inner(self: Box<Self>) -> W {
        self.inner
    }
}

impl O5mEncoder {
    pub fn new() -> Self {
        O5mEncoder {
            string_table: StringReferenceTable::new(),
            delta: DeltaState::new(),
        }
    }

    /// Resets string reference table and all deltas.
    pub fn reset(&mut self) {
        self.string_table.clear();
        self.delta = DeltaState::new();
    }

    /// Converts a node into a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Node
    pub fn node_to_bytes(&mut self, node: &Node) -> Result<Vec<u8>> {
        let delta_id = self.delta.encode(Id, node.id);
        let delta_coordinate = self.delta_coordinate(node.coordinate);

        let mut bytes = Vec::new();
        bytes.write_varint(delta_id)?;
        bytes.write(&self.meta_to_bytes(&node.meta)?)?;
        bytes.write_varint(delta_coordinate.lon)?;
        bytes.write_varint(delta_coordinate.lat)?;

        for tag in &node.meta.tags {
            bytes.write(&self.string_pair_to_bytes(&tag.key, &tag.value))?;
        }

        Ok(bytes)
    }

    /// Converts a way into a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Way
    pub fn way_to_bytes(&mut self, way: &Way) -> Result<Vec<u8>> {
        let delta_id = self.delta.encode(Id, way.id);
        let ref_bytes = self.way_refs_to_bytes(&way.refs)?;

        let mut bytes = Vec::new();
        bytes.write_varint(delta_id)?;
        bytes.write(&self.meta_to_bytes(&way.meta)?)?;
        bytes.write_varint(ref_bytes.len() as u64)?;
        bytes.write(&ref_bytes)?;

        for tag in &way.meta.tags {
            bytes.write(&self.string_pair_to_bytes(&tag.key, &tag.value))?;
        }

        Ok(bytes)
    }

    /// Converts way references to bytes.
    fn way_refs_to_bytes(&mut self, refs: &[i64]) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for i in refs {
            let delta = self.delta.encode(WayRef, *i);
            bytes.write_varint(delta)?;
        }
        Ok(bytes)
    }

    /// Converts a relation into a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Relation
    pub fn relation_to_bytes(&mut self, rel: &Relation) -> Result<Vec<u8>> {
        let delta_id = self.delta.encode(Id, rel.id);
        let mem_bytes = self.rel_members_to_bytes(&rel.members)?;

        let mut bytes = Vec::new();
        bytes.write_varint(delta_id)?;
        bytes.write(&self.meta_to_bytes(&rel.meta)?)?;
        bytes.write_varint(mem_bytes.len() as u64)?;
        bytes.write(&mem_bytes)?;

        for tag in &rel.meta.tags {
            bytes.write(&self.string_pair_to_bytes(&tag.key, &tag.value))?;
        }

        Ok(bytes)
    }

    /// Converts relation members to bytes.
    fn rel_members_to_bytes(&mut self, members: &[RelationMember]) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for m in members {
            let mem_type = member_type(m);
            let mem_role = m.role();
            let delta = self.delta_rel_member(m);

            let mut mem_bytes = Vec::new();
            mem_bytes.push(0x00);
            mem_bytes.write(&mem_type.as_bytes().to_owned())?;
            mem_bytes.write(&mem_role.as_bytes().to_owned())?;
            mem_bytes.push(0x0);

            bytes.write_varint(delta)?;
            bytes.write(&self.string_table.reference(mem_bytes))?;
        }
        Ok(bytes)
    }

    /// Converts meta to bytes. It's positioned directly after the id of the element.
    pub fn meta_to_bytes(&mut self, meta: &Meta) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        if let Some(version) = meta.version {
            bytes.write_varint(version)?;

            if let Some(author) = meta.author.as_ref() {
                let delta_time = self.delta.encode(Time, author.created);
                let delta_change_set = self.delta.encode(ChangeSet, author.change_set as i64);

                bytes.write_varint(delta_time)?;
                bytes.write_varint(delta_change_set)?;
                bytes.write(&self.user_to_bytes(author.uid, &author.user)?)?;
            } else {
                bytes.push(0x00); // No author info.
            }
        } else {
            bytes.push(0x00); // No version, no timestamp and no author info.
        }
        Ok(bytes)
    }

    /// Converts a string pair into a byte vector that can be written to file.
    /// If the string has appeared previously after the last reset a reference is returned.
    ///
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Strings
    fn string_pair_to_bytes(&mut self, key: &str, value: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(0x00);
        for byte in key.bytes() {
            bytes.push(byte);
        }

        bytes.push(0x00);
        for byte in value.bytes() {
            bytes.push(byte);
        }
        bytes.push(0x00);

        self.string_table.reference(bytes)
    }

    /// Converts a user to a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Strings
    fn user_to_bytes(&mut self, uid: u64, username: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        bytes.push(0);
        bytes.write_varint(uid)?;

        bytes.push(0);
        for byte in username.bytes() {
            bytes.push(byte);
        }
        bytes.push(0);

        Ok(self.string_table.reference(bytes))
    }

    /// Relation members have delta split on the relation type.
    fn delta_rel_member(&mut self, member: &RelationMember) -> i64 {
        match member {
            RelationMember::Node(id, _) => self.delta.encode(RelNodeRef, *id),
            RelationMember::Way(id, _) => self.delta.encode(RelWayRef, *id),
            RelationMember::Relation(id, _) => self.delta.encode(RelRelRef, *id),
        }
    }

    fn delta_coordinate(&mut self, coordinate: Coordinate) -> Coordinate {
        Coordinate {
            lat: self.delta.encode(Lat, coordinate.lat as i64) as i32,
            lon: self.delta.encode(Lon, coordinate.lon as i64) as i32,
        }
    }
}

/// See: https://wiki.openstreetmap.org/wiki/O5m#cite_note-1
fn member_type(member: &RelationMember) -> &str {
    match member {
        RelationMember::Node(_, _) => "0",
        RelationMember::Way(_, _) => "1",
        RelationMember::Relation(_, _) => "2",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthorInformation, Meta, Relation, RelationMember, Way};

    #[test]
    fn string_pair_bytes() {
        let mut encoder = O5mEncoder::new();
        let bytes = encoder.string_pair_to_bytes("oneway", "yes");
        let expected: Vec<u8> = vec![
            0x00, 0x6f, 0x6e, 0x65, 0x77, 0x61, 0x79, 0x00, 0x79, 0x65, 0x73, 0x00,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn string_references() {
        let mut references = O5mEncoder::new();
        assert_eq!(
            references.string_pair_to_bytes("oneway", "yes"),
            vec![0x00, 0x6f, 0x6e, 0x65, 0x77, 0x61, 0x79, 0x00, 0x79, 0x65, 0x73, 0x00]
        );
        assert_eq!(
            references.string_pair_to_bytes("atm", "no"),
            vec![0x00, 0x61, 0x74, 0x6d, 0x00, 0x6e, 0x6f, 0x00]
        );
        assert_eq!(references.string_pair_to_bytes("oneway", "yes"), vec![0x02]);
        assert_eq!(
            references.user_to_bytes(1020, "John").unwrap(),
            vec![0x00, 0xfc, 0x07, 0x00, 0x4a, 0x6f, 0x68, 0x6e, 0x00]
        );
        assert_eq!(references.string_pair_to_bytes("atm", "no"), vec![0x02]);
        assert_eq!(references.string_pair_to_bytes("oneway", "yes"), vec![0x03]);
        assert_eq!(references.user_to_bytes(1020, "John").unwrap(), vec![0x01]);
    }

    #[test]
    fn write_node() {
        let expected: Vec<u8> = vec![
            0x10, // Node type
            0x26, // Length
            0x80, 0x01, // Id, delta
            0x01, // Version
            0xe4, 0x8e, 0xa7, 0xca, 0x09, // Timestamp
            0x94, 0xfe, 0xd2, 0x05, // Changeset
            0x00, 0x85, 0xe3, 0x02, 0x00, // Uid
            0x55, 0x53, 0x63, 0x68, 0x61, 0x00, // User
            0x08, // Lon, delta
            0x81, 0x01, // Lat, delta
            // oneway=yes
            0x00, 0x6F, 0x6E, 0x65, 0x77, 0x61, 0x79, 0x00, 0x79, 0x65, 0x73, 0x00,
        ];

        let node = Node {
            id: 64,
            coordinate: Coordinate { lat: -65, lon: 4 },
            meta: Meta {
                tags: vec![("oneway", "yes").into()],
                version: Some(1),
                author: Some(AuthorInformation {
                    created: 1285874610,
                    change_set: 5922698,
                    uid: 45445,
                    user: "UScha".to_string(),
                }),
                ..Default::default()
            },
        };

        let mut writer = O5mWriter::new(Vec::new());
        writer.write_node(&node).unwrap();
        assert_eq!(writer.inner, expected)
    }

    #[test]
    fn write_way() {
        let expected: Vec<u8> = vec![
            0x11, // Way type
            0x1B, // Length
            0x80, 0x01, // Id, delta
            0x01, // Version
            0x00, // Timestamp
            0x03, // Length of ref section
            0x80, 0x01, // Ref1
            0x02, // Ref2
            // highway=secondary
            0x00, 0x68, 0x69, 0x67, 0x68, 0x77, 0x61, 0x79, 0x00, 0x73, 0x65, 0x63, 0x6f, 0x6e,
            0x64, 0x61, 0x72, 0x79, 0x00,
        ];

        let way = Way {
            id: 64,
            refs: vec![64, 65],
            meta: Meta {
                tags: vec![("highway", "secondary").into()],
                version: Some(1),
                ..Default::default()
            },
        };

        let mut writer = O5mWriter::new(Vec::new());
        writer.write_way(&way).unwrap();
        assert_eq!(writer.inner, expected)
    }

    #[test]
    fn relation_bytes() {
        let expected: Vec<u8> = vec![
            0x12, // Relation type
            0x2B, // Length
            0x80, 0x01, // Id, delta
            0x00, // Version
            0x14, // Length of ref section
            0x08, // Ref id, delta
            0x00, 0x31, // Way
            0x6F, 0x75, 0x74, 0x65, 0x72, 0x00, // Outer
            0x08, // Ref id, delta
            0x00, 0x31, // Way
            0x69, 0x6e, 0x6e, 0x65, 0x72, 0x00, // Inner
            0x08, // Ref id, delta
            0x01, // String ref to way inner.
            // type=multipolygon
            0x00, 0x74, 0x79, 0x70, 0x65, 0x00, 0x6D, 0x75, 0x6C, 0x74, 0x69, 0x70, 0x6F, 0x6C,
            0x79, 0x67, 0x6F, 0x6E, 0x00,
        ];
        let relation = Relation {
            id: 64,
            members: vec![
                RelationMember::Way(4, "outer".to_owned()),
                RelationMember::Way(8, "inner".to_owned()),
                RelationMember::Way(12, "inner".to_owned()),
            ],
            meta: Meta {
                tags: vec![("type", "multipolygon").into()],
                ..Default::default()
            },
        };

        let mut writer = O5mWriter::new(Vec::new());
        writer.write_relation(&relation).unwrap();
        assert_eq!(writer.inner, expected)
    }

    #[test]
    fn coordinate_delta() {
        let mut encoder = O5mEncoder::new();
        assert_eq!(
            encoder.delta_coordinate(Coordinate { lat: 1, lon: 10 }),
            Coordinate { lat: 1, lon: 10 }
        );
        assert_eq!(
            encoder.delta_coordinate(Coordinate { lat: 2, lon: 11 }),
            Coordinate { lat: 1, lon: 1 }
        );
    }
}
