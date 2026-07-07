use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::result::Result;

use quick_xml::events::Event;
use zip::result::ZipError;

use crate::Error::{MissingRootPart, NoRelsFile, XmlFormat};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip io error: {0}")]
    ZipIo(#[from] zip::result::ZipError),
    #[error("missing relation of root part in _rels/.rels")]
    MissingRootPart,
    #[error("no _rels/.rels file in 3mf: {0}")]
    NoRelsFile(#[source] ZipError),
    #[error("xml encoding error {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),
    #[error("xml format error {0}")]
    XmlFormat(#[from] quick_xml::Error),
    #[error("invalid utf-8 in target path: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

pub struct ThreeMFContainer {
    #[allow(dead_code)]
    archive: zip::ZipArchive<std::io::BufReader<File>>,
    #[allow(dead_code)]
    root_part_path: String,
}

pub fn open(path: &Path) -> Result<ThreeMFContainer, Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut zip_archive = zip::ZipArchive::new(reader)?;

    let rels_file = match zip_archive.by_name("_rels/.rels") {
        Ok(rels_file) => rels_file,
        Err(zip_error) => return Err(NoRelsFile(zip_error)),
    };
    //let mut rels_data: String = Default::default();
    //rels_file.read_to_string(&mut rels_data)?;
    let mut xml_reader = quick_xml::Reader::from_reader(BufReader::new(rels_file));
    let mut buf = Vec::new();
    let mut target_path: String = Default::default();
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                //println!("Starting tag event with tag {:?}", e.name());
                if e.name().as_ref() == b"Relationship" {
                    let is_node = e
                        .attributes()
                        .flatten()
                        .find(|attr| attr.key.as_ref() == b"Type")
                        .is_some_and(|attr| {
                            attr.value.as_ref()
                                == b"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"
                        });
                    if is_node {
                        let target = e
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.as_ref() == b"Target")
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
    Ok(ThreeMFContainer {
        archive: zip_archive,
        root_part_path: target_path,
    })
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
