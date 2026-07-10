use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::result::Result;

use quick_xml::events::Event;
use zip::{ZipArchive, result::ZipError};

use crate::Error::{MissingRootPart, MissingFile, XmlFormat};

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
                    let is_node = e
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.as_ref() == TYPE_ATTR)
                        .is_some_and(|attr| attr.value.as_ref() == MODEL_RELATIONSHIP_TYPE);
                    if is_node {
                        let target = e
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.as_ref() == TARGET_ATTR)
                            .map(|attr| attr.value.into_owned())
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
    validate_part_type(&mut zip_archive, &target_path)?;
    Ok(ThreeMFContainer {
        archive: zip_archive,
        root_part_path: target_path,
    })
}

fn validate_part_type(zipfile: &mut ZipArchive<BufReader<File>>, root_part_path: &str) -> Result<bool, Error>
{
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
    let extension = Path::new(&root_part_path).extension().and_then(|ext| ext.to_str()).unwrap_or_default();
    let mut valid_content_type = false;
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if e.name().as_ref() == DEFAULT_TAG {
                    let ext_matches = e.attributes()
                    .flatten()
                    .any(|attr| (attr.key.as_ref() == EXTENSION_ATTR) && (attr.value.as_ref() == extension.as_bytes()));

                    if ext_matches && let Some(content_type_attr) = e.attributes().flatten()
                        .find(|attr| attr.key.as_ref() == CONTENT_TYPE_ATTR) {
                        valid_content_type = content_type_attr.value.as_ref() == MODEL_CONTENT_TYPE;
                        println!("The 3mf root_part path has the correct content type");
                        return Ok(valid_content_type)
                    }
                }
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(XmlFormat(e)),
            _ => (),

        }
        buf.clear();
    }
    Ok(valid_content_type)
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
