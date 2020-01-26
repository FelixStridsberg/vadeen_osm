use std::io;
use std::io::Write;

use super::*;
use crate::geo::{Boundary, Coordinate};
use crate::osm_io::error::ErrorKind;
use crate::osm_io::o5m::Delta::{Id, Lat, Lon, RelNodeRef, RelRelRef, RelWayRef, WayRef};
use crate::osm_io::OsmWriter;
use crate::{Node, Osm, Relation, RelationMember, Way};
use std::collections::VecDeque;

/// Todo write user information etc...
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
    string_table: VecDeque<Vec<u8>>,
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
    fn write_bounding_box(&mut self, boundary: &Boundary) -> io::Result<()> {
        let mut bytes = Vec::new();
        bytes.append(&mut varint_to_bytes(boundary.min.lon.into()));
        bytes.append(&mut varint_to_bytes(boundary.min.lat.into()));
        bytes.append(&mut varint_to_bytes(boundary.max.lon.into()));
        bytes.append(&mut varint_to_bytes(boundary.max.lat.into()));

        self.inner.write_all(&[O5M_BOUNDING_BOX])?;
        self.inner
            .write_all(&uvarint_to_bytes(bytes.len() as u64))?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Node
    fn write_node(&mut self, node: &Node) -> io::Result<()> {
        let bytes = self.encoder.node_to_bytes(node);
        self.inner.write_all(&[O5M_NODE])?;
        self.inner
            .write_all(&uvarint_to_bytes(bytes.len() as u64))?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Way
    fn write_way(&mut self, way: &Way) -> io::Result<()> {
        let bytes = self.encoder.way_to_bytes(way);
        self.inner.write_all(&[O5M_WAY])?;
        self.inner
            .write_all(&uvarint_to_bytes(bytes.len() as u64))?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }

    /// See: https://wiki.openstreetmap.org/wiki/O5m#Relation
    fn write_relation(&mut self, rel: &Relation) -> io::Result<()> {
        let bytes = self.encoder.relation_to_bytes(rel);
        self.inner.write_all(&[O5M_RELATION])?;
        self.inner
            .write_all(&uvarint_to_bytes(bytes.len() as u64))?;
        self.inner.write_all(&bytes)?;
        Ok(())
    }
}

impl<W: Write> OsmWriter<W> for O5mWriter<W> {
    fn write(&mut self, osm: &Osm) -> std::result::Result<(), ErrorKind> {
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
            string_table: VecDeque::new(),
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
    pub fn node_to_bytes(&mut self, node: &Node) -> Vec<u8> {
        let delta_id = self.delta.encode(Id, node.id);
        let delta_coordinate = self.delta_coordinate(node.coordinate);

        let mut bytes = Vec::new();
        bytes.append(&mut varint_to_bytes(delta_id));
        bytes.append(&mut uvarint_to_bytes(node.meta.version as u64));
        bytes.push(0x00); // Timestamp
        bytes.append(&mut varint_to_bytes(delta_coordinate.lon.into()));
        bytes.append(&mut varint_to_bytes(delta_coordinate.lat.into()));

        for tag in &node.meta.tags {
            bytes.append(&mut self.string_pair_to_bytes(&tag.key, &tag.value));
        }

        bytes
    }

    /// Converts a way into a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Way
    pub fn way_to_bytes(&mut self, way: &Way) -> Vec<u8> {
        let delta_id = self.delta.encode(Id, way.id);
        let mut ref_bytes = self.way_refs_to_bytes(&way.refs);

        let mut bytes = Vec::new();
        bytes.append(&mut varint_to_bytes(delta_id));
        bytes.append(&mut uvarint_to_bytes(way.meta.version as u64));
        bytes.push(0x00); // Timestamp
        bytes.append(&mut uvarint_to_bytes(ref_bytes.len() as u64));
        bytes.append(&mut ref_bytes);

        for tag in &way.meta.tags {
            bytes.append(&mut self.string_pair_to_bytes(&tag.key, &tag.value));
        }

        bytes
    }

    /// Converts way references to bytes.
    fn way_refs_to_bytes(&mut self, refs: &[i64]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in refs {
            let delta = self.delta.encode(WayRef, *i);
            bytes.append(&mut varint_to_bytes(delta));
        }
        bytes
    }

    /// Converts a relation into a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Relation
    pub fn relation_to_bytes(&mut self, rel: &Relation) -> Vec<u8> {
        let delta_id = self.delta.encode(Id, rel.id);
        let mut mem_bytes = self.rel_members_to_bytes(&rel.members);

        let mut bytes = Vec::new();
        bytes.append(&mut varint_to_bytes(delta_id));
        bytes.append(&mut uvarint_to_bytes(rel.meta.version as u64));
        bytes.push(0x00); // Timestamp
        bytes.append(&mut uvarint_to_bytes(mem_bytes.len() as u64));
        bytes.append(&mut mem_bytes);

        for tag in &rel.meta.tags {
            bytes.append(&mut self.string_pair_to_bytes(&tag.key, &tag.value));
        }

        bytes
    }

    /// Converts relation members to bytes.
    fn rel_members_to_bytes(&mut self, members: &[RelationMember]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for m in members {
            let mem_type = member_type(m);
            let mem_role = m.role();
            let delta = self.delta_rel_member(m);

            bytes.append(&mut varint_to_bytes(delta));
            bytes.push(0x00);
            for b in mem_type.bytes() {
                bytes.push(b);
            }
            for b in mem_role.bytes() {
                bytes.push(b);
            }
            bytes.push(0x00);
        }
        bytes
    }

    /// Converts a string pair into a byte vector that can be written to file.
    /// If the string has appeared previously after the last reset a reference is returned.
    ///
    /// Only string pairs of at most 250 characters can be references as per spec.
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

        if key.len() + value.len() > 250 {
            bytes
        } else {
            self.string_reference_table(bytes)
        }
    }

    /// Looks up bytes from a string in a table. Returns a reference if the bytes already exists
    /// in the table.
    /// At most 15000 strings may exist in the reference table, if more is added the oldest is
    /// removed.
    ///
    /// TODO the 15000 limit
    fn string_reference_table(&mut self, bytes: Vec<u8>) -> Vec<u8> {
        if let Some(pos) = self.string_table.iter().position(|b| b == &bytes) {
            uvarint_to_bytes(1 + pos as u64)
        } else {
            self.string_table.push_front(bytes.clone());
            bytes
        }
    }

    /// Converts a user to a byte vector that can be written to file.
    /// See: https://wiki.openstreetmap.org/wiki/O5m#Strings
    fn user_to_bytes(&mut self, uid: u64, username: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(0);
        bytes.append(&mut uvarint_to_bytes(uid));

        bytes.push(0);
        for byte in username.bytes() {
            bytes.push(byte);
        }
        bytes.push(0);

        self.string_reference_table(bytes)
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

/// Uvarint uses the most significant bit of every byte to determine if there is more bytes
/// remaining.
/// See: https://wiki.openstreetmap.org/wiki/O5m#Numbers
pub fn uvarint_to_bytes(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();

    while value > 0x7F {
        bytes.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }

    if value > 0 {
        bytes.push(value as u8);
    }

    bytes
}

/// Varint is same as uvarint, but it also uses the least significant bit of the least significant
/// byte to determine if the number is negative or not.
/// See: https://wiki.openstreetmap.org/wiki/O5m#Numbers
pub fn varint_to_bytes(mut value: i64) -> Vec<u8> {
    let mut sign_bit = 0x00;
    if value < 0 {
        sign_bit = 0x01;

        // We handle the sign our selves, negative range is shifted by 1.
        value = -value - 1;
    }

    let value = value as u64;
    let least_significant = (((value << 1) & 0x7F) | sign_bit) as u8;

    let mut bytes = Vec::new();
    // We can only fit 6 bits in first byte since we use 1 bit for sign and 1 for length.
    if value > 0x3F {
        bytes.push(least_significant | 0x80);

        // Only first byte is special, rest is same as uvarint.
        let mut rest = uvarint_to_bytes(value >> 6);
        bytes.append(&mut rest);
    } else {
        bytes.push(least_significant);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meta, Relation, RelationMember, Way};

    #[test]
    fn one_byte_uvarint() {
        let bytes = uvarint_to_bytes(5);
        assert_eq!(bytes, vec![0x05]);
    }

    #[test]
    fn max_one_byte_uvarint() {
        let bytes = uvarint_to_bytes(127);
        assert_eq!(bytes, vec![0x7F]);
    }

    #[test]
    fn two_byte_uvarint() {
        let bytes = uvarint_to_bytes(323);
        assert_eq!(bytes, vec![0xC3, 0x02]);
    }

    #[test]
    fn three_byte_uvarint() {
        let bytes = uvarint_to_bytes(16384);
        assert_eq!(bytes, vec![0x80, 0x80, 0x01]);
    }

    #[test]
    fn one_byte_positive_varint() {
        let bytes = varint_to_bytes(4);
        assert_eq!(bytes, vec![0x08]);
    }

    #[test]
    fn one_byte_negative_varint() {
        let bytes = varint_to_bytes(-3);
        assert_eq!(bytes, vec![0x05]);
    }

    #[test]
    fn two_byte_positive_varint() {
        let bytes = varint_to_bytes(64);
        assert_eq!(bytes, vec![0x80, 0x01]);
    }

    #[test]
    fn two_byte_negative_varint() {
        let bytes = varint_to_bytes(-65);
        assert_eq!(bytes, vec![0x81, 0x01]);
    }

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
            references.user_to_bytes(1020, "John"),
            vec![0x00, 0xfc, 0x07, 0x00, 0x4a, 0x6f, 0x68, 0x6e, 0x00]
        );
        assert_eq!(references.string_pair_to_bytes("atm", "no"), vec![0x02]);
        assert_eq!(references.string_pair_to_bytes("oneway", "yes"), vec![0x03]);
        assert_eq!(references.user_to_bytes(1020, "John"), vec![0x01]);
    }

    #[test]
    fn write_node() {
        let expected: Vec<u8> = vec![
            0x10, // Node type
            0x13, // Length
            0x80, 0x01, // Id, delta
            0x01, // Version
            0x00, // Timestamp
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
            0x2A, // Length
            0x80, 0x01, // Id, delta
            0x01, // Version
            0x00, // Timestamp
            0x12, // Length of ref section
            0x08, // Ref id, delta
            0x00, 0x31, // Way
            0x6F, 0x75, 0x74, 0x65, 0x72, 0x00, // Outer
            0x08, // Ref id, delta
            0x00, 0x31, // Way
            0x69, 0x6e, 0x6e, 0x65, 0x72, 0x00, // Inner
            // type=multipolygon
            0x00, 0x74, 0x79, 0x70, 0x65, 0x00, 0x6D, 0x75, 0x6C, 0x74, 0x69, 0x70, 0x6F, 0x6C,
            0x79, 0x67, 0x6F, 0x6E, 0x00,
        ];
        let relation = Relation {
            id: 64,
            members: vec![
                RelationMember::Way(4, "outer".to_owned()),
                RelationMember::Way(8, "inner".to_owned()),
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
