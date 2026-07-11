use std::io::BufReader;
use std::path::Path;
use std::result::Result;
use std::{borrow::Cow, fs::File};

use quick_xml::events::{BytesStart, Event};
use zip::{ZipArchive, result::ZipError};

use crate::Error::{InvalidContentType, MissingFile, MissingRootPart, XmlFormat};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip io error: {0}")]
    ZipIo(#[from] zip::result::ZipError),
    #[error("missing relation of root part in _rels/.rels")]
    MissingRootPart,
    #[error("missing file {1} in 3mf: {0}")]
    MissingFile(#[source] ZipError, String),
    #[error("xml encoding error {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),
    #[error("xml format error {0}")]
    XmlFormat(#[from] quick_xml::Error),
    #[error("invalid utf-8 in target path: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("Invalid content type for file {0}")]
    InvalidContentType(String),
}
#[derive(Debug)]
pub struct ThreeMFContainer {
    #[allow(dead_code)]
    archive: zip::ZipArchive<BufReader<File>>,
    #[allow(dead_code)]
    root_part_path: String,
}

pub fn open(path: &Path) -> Result<ThreeMFContainer, Error> {
    const RELS_ENTRY: &str = "_rels/.rels";
    const RELATIONSHIP_TAG: &[u8] = b"Relationship";
    const TYPE_ATTR: &[u8] = b"Type";
    const TARGET_ATTR: &[u8] = b"Target";
    const MODEL_RELATIONSHIP_TYPE: &[u8] =
        b"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut zip_archive = zip::ZipArchive::new(reader)?;

    let rels_file = match zip_archive.by_name(RELS_ENTRY) {
        Ok(rels_file) => rels_file,
        Err(zip_error) => return Err(MissingFile(zip_error, RELS_ENTRY.to_string())),
    };
    let mut xml_reader = quick_xml::Reader::from_reader(BufReader::new(rels_file));
    let mut buf = Vec::new();
    let mut target_path: String = Default::default();
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                //println!("Starting tag event with tag {:?}", e.name());
                if e.name().as_ref() == RELATIONSHIP_TAG {
                    let is_node = find_attr_value(&e, TYPE_ATTR)
                        .is_some_and(|v| v.as_ref() == MODEL_RELATIONSHIP_TYPE);
                    if is_node {
                        let target = find_attr_value(&e, TARGET_ATTR)
                            .map(Cow::into_owned)
                            .ok_or(MissingRootPart)?;
                        target_path = String::from_utf8(target)?;
                        //println!("The root part path is {}", target_path);
                    }
                }
            }
            Err(e) => return Err(XmlFormat(e)),
            Ok(Event::Eof) => break,
            _ => (),
        }
        buf.clear();
    }
    drop(xml_reader);
    if target_path.is_empty() {
        return Err(MissingRootPart);
    }
    validate_part_type(&mut zip_archive, &target_path)?;

    Ok(ThreeMFContainer {
        archive: zip_archive,
        root_part_path: target_path,
    })
}

fn validate_part_type(
    zipfile: &mut ZipArchive<BufReader<File>>,
    root_part_path: &str,
) -> Result<(), Error> {
    const CONTENT_TYPES_ENTRY: &str = "[Content_Types].xml";
    const DEFAULT_TAG: &[u8] = b"Default";
    const EXTENSION_ATTR: &[u8] = b"Extension";
    const CONTENT_TYPE_ATTR: &[u8] = b"ContentType";
    const MODEL_CONTENT_TYPE: &[u8] = b"application/vnd.ms-package.3dmanufacturing-3dmodel+xml";

    let content_types_file = match zipfile.by_name(CONTENT_TYPES_ENTRY) {
        Ok(content_types_file) => content_types_file,
        Err(zip_error) => return Err(MissingFile(zip_error, CONTENT_TYPES_ENTRY.to_string())),
    };

    let mut xml_reader = quick_xml::Reader::from_reader(BufReader::new(content_types_file));
    let mut buf = Vec::new();
    let extension = Path::new(&root_part_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if e.name().as_ref() == DEFAULT_TAG {
                    let ext_matches = find_attr_value(&e, EXTENSION_ATTR)
                        .is_some_and(|v| v.as_ref() == extension.as_bytes());

                    if ext_matches {
                        let content_type_marches = find_attr_value(&e, CONTENT_TYPE_ATTR)
                            .is_some_and(|v| v.as_ref() == MODEL_CONTENT_TYPE);
                        if content_type_marches {
                            println!("The 3mf root_part path has the correct content type");
                            return Ok(());
                        } else {
                            return Err(InvalidContentType(root_part_path.to_string()));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(XmlFormat(e)),
            _ => (),
        }
        buf.clear();
    }
    Err(InvalidContentType(root_part_path.to_string()))
}

fn find_attr_value<'a>(e: &'a BytesStart, key: &[u8]) -> Option<Cow<'a, [u8]>> {
    e.attributes()
        .flatten()
        .find(|attr| attr.key.as_ref() == key)
        .map(|attr| attr.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_finds_root_part_relationship() {
        let container = open(Path::new("fixtures/core/box.3mf")).unwrap();
        assert_eq!(container.root_part_path, "/3D/3dmodel.model");
    }
}
