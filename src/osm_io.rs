//! IO functionality for OSM maps.
//!
//! You create readers or writers with the [`create_reader`] and [`create_writer`] functions. These
//! returns readers and writers appropriate to the input or output format specified.
//!
//! The input and output format is defined by the [`FileFormat`] enum. This can either be specified
//! explicitly or parsed from a path or string.
//!
//! Error handling is defined in the [`error`] module.
//!
//! # Examples
//! Convert a .osm file to a .o5m file.
//! ```rust,no_run
//! # use vadeen_osm::Osm;
//! # use vadeen_osm::osm_io::{create_writer, FileFormat, create_reader};
//! # use std::fs::File;
//! # use std::path::Path;
//! # use std::convert::TryInto;
//! # use std::io::BufReader;
//! // Read map from map.osm
//! let path = Path::new("map.osm");
//! let format = path.try_into().unwrap(); // Parse input format from path.
//! let file = File::open(path).unwrap();
//! let mut reader = create_reader(BufReader::new(file), format);
//! let osm = reader.read().unwrap();
//!
//! // Write map to map.o5m
//! let path = Path::new("map.o5m");
//! let output = File::create(path).unwrap();
//! let mut writer = create_writer(output, FileFormat::O5m);
//! writer.write(&osm);
//! ```
//!
//! [`create_reader`]: fn.create_reader.html
//! [`create_writer`]: fn.create_writer.html
//! [`FileFormat`]: enum.FileFormat.html
//! [`error`]: error/index.html
pub mod error;
mod o5m;
mod xml;

use self::error::*;
use self::o5m::O5mWriter;
use self::xml::XmlWriter;
use crate::osm_io::o5m::O5mReader;
use crate::osm_io::xml::XmlReader;
use crate::Osm;
use std::convert::{TryFrom, TryInto};
use std::io::{BufRead, Write};
use std::path::Path;

/// Represent a osm file format.
///
/// See OSM documentation over [`file formats`].
///
/// # Examples
/// ```
/// # use vadeen_osm::osm_io::FileFormat;
/// # use std::path::Path;
/// # use std::convert::TryInto;
/// assert_eq!("osm".try_into(), Ok(FileFormat::Xml));
/// assert_eq!(Path::new("./path/file.o5m").try_into(), Ok(FileFormat::O5m));
/// assert_eq!(FileFormat::from("o5m"), Some(FileFormat::O5m));
/// ```
/// [`file formats`]: https://wiki.openstreetmap.org/wiki/OSM_file_formats
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum FileFormat {
    Xml,
    O5m,
}

/// Writer for the osm formats.
pub trait OsmWriter<W: Write> {
    fn write(&mut self, osm: &Osm) -> std::result::Result<(), ErrorKind>;

    fn into_inner(self: Box<Self>) -> W;
}

/// Reader for the osm formats.
pub trait OsmReader {
    fn read(&mut self) -> std::result::Result<Osm, Error>;
}

/// Creates an `OsmWriter` appropriate to the provided `FileFormat`.
pub fn create_writer<'a, W: Write + 'a>(
    writer: W,
    format: FileFormat,
) -> Box<dyn OsmWriter<W> + 'a> {
    match format {
        FileFormat::O5m => Box::new(O5mWriter::new(writer)),
        FileFormat::Xml => Box::new(XmlWriter::new(writer)),
    }
}

pub fn create_reader<'a, R: BufRead + 'a>(
    reader: R,
    format: FileFormat,
) -> Box<dyn OsmReader + 'a> {
    match format {
        FileFormat::Xml => Box::new(XmlReader::new(reader)),
        FileFormat::O5m => Box::new(O5mReader::new(reader)),
    }
}

impl FileFormat {
    pub fn from(s: &str) -> Option<Self> {
        match s {
            "osm" => Some(FileFormat::Xml),
            "o5m" => Some(FileFormat::O5m),
            _ => None,
        }
    }
}

impl TryFrom<&str> for FileFormat {
    type Error = String;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        if let Some(format) = FileFormat::from(&value) {
            Ok(format)
        } else {
            Err(format!("'{:?}' is not a valid osm file format", value))
        }
    }
}

impl TryFrom<&String> for FileFormat {
    type Error = String;

    fn try_from(value: &String) -> std::result::Result<Self, Self::Error> {
        (value[..]).try_into()
    }
}

impl TryFrom<&Path> for FileFormat {
    type Error = String;

    fn try_from(path: &Path) -> std::result::Result<Self, Self::Error> {
        if let Some(ext) = path.extension() {
            if let Some(str) = ext.to_str() {
                return str.try_into();
            }
        }
        Err(format!("Unknown file format of '{:?}'", path.to_str()))
    }
}

#[cfg(test)]
mod tests {
    use crate::osm_io::FileFormat;
    use std::convert::TryInto;
    use std::path::Path;

    #[test]
    fn file_format_from_path() {
        let path = Path::new("test.o5m");
        let format = path.try_into();
        assert_eq!(format, Ok(FileFormat::O5m));

        let path = Path::new("test.osm");
        let format = path.try_into();
        assert_eq!(format, Ok(FileFormat::Xml));
    }

    #[test]
    fn file_format_from_str() {
        let format = "o5m".try_into();
        assert_eq!(format, Ok(FileFormat::O5m));

        let format = "osm".try_into();
        assert_eq!(format, Ok(FileFormat::Xml));
    }

    #[test]
    fn file_format_from_string() {
        let format = (&"o5m".to_owned()).try_into();
        assert_eq!(format, Ok(FileFormat::O5m));

        let format = (&"osm".to_owned()).try_into();
        assert_eq!(format, Ok(FileFormat::Xml));
    }
}
