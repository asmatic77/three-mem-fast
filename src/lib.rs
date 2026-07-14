use std::io::BufReader;
use std::path::Path;
use std::result::Result;
use std::{borrow::Cow, fs::File};

use quick_xml::events::{BytesStart, Event};
use zip::{ZipArchive, result::ZipError};

use crate::Error::{InvalidContentType, MissingAttribute, MissingFile, MissingRootPart, XmlFormat};

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
    #[error("Missing attribute {0} in element {1}")]
    MissingAttribute(String, String),
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct ThreeMFContainer {
    archive: zip::ZipArchive<BufReader<File>>,
    root_part_path: String,
    geometry_stats: GeometryStatistics,
    units: String,
}
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct GeometryStatistics {
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub object_count: usize,
    pub build_items: usize,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct Vertex {
    x: f32,
    y: f32,
    z: f32,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct Triangle {
    v1: u32,
    v2: u32,
    v3: u32,
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
        geometry_stats: GeometryStatistics::default(),
        units: String::default(),
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
                        let content_type_matches = find_attr_value(&e, CONTENT_TYPE_ATTR)
                            .is_some_and(|v| v.as_ref() == MODEL_CONTENT_TYPE);
                        if content_type_matches {
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

impl ThreeMFContainer {
    pub fn parse_root_part(&mut self) -> Result<(), Error> {
        let root_file_path = self
            .root_part_path
            .strip_prefix("/")
            .unwrap_or(&self.root_part_path);

        let part_file = match self.archive.by_name(root_file_path) {
            Ok(part_file) => part_file,
            Err(zip_error) => return Err(MissingFile(zip_error, root_file_path.to_string())),
        };
        let mut xml_reader = quick_xml::Reader::from_reader(BufReader::new(part_file));
        let mut buf = Vec::new();

        const MODEL_TAG: &[u8] = b"model";
        const RESOURCES_TAG: &[u8] = b"resources";
        const OBJECT_TAG: &[u8] = b"object";
        const MESH_TAG: &[u8] = b"mesh";
        const VERTICES_TAG: &[u8] = b"vertices";
        const VERTEX_TAG: &[u8] = b"vertex";
        const TRIANGLES_TAG: &[u8] = b"triangles";
        const TRIANGLE_TAG: &[u8] = b"triangle";
        const BUILD_TAG: &[u8] = b"build";
        const ITEM_TAG: &[u8] = b"item";

        let mut stats = GeometryStatistics::default();
        let mut _current_tag: Option<Vec<u8>>; // TODO maybe we dont need to track this?
        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                    _current_tag = Some(Vec::from(e.name().as_ref()));
                    match e.name().as_ref() {
                        MODEL_TAG => (),
                        RESOURCES_TAG => (),
                        OBJECT_TAG => ThreeMFContainer::parse_object(&e, &mut stats),
                        MESH_TAG => (),
                        VERTICES_TAG => (),
                        VERTEX_TAG => {
                            ThreeMFContainer::parse_vertex(&e, &mut stats)?;
                        }
                        TRIANGLES_TAG => (),
                        TRIANGLE_TAG => {
                            ThreeMFContainer::parse_triangle(&e, &mut stats)?;
                        }
                        BUILD_TAG => (),
                        ITEM_TAG => ThreeMFContainer::parse_item(&e, &mut stats),
                        _ => (),
                    }
                }
                Ok(Event::End(_e)) => _current_tag = None,
                Err(e) => return Err(XmlFormat(e)),
                Ok(Event::Eof) => break,
                _ => (),
            }
            buf.clear();
        }
        drop(xml_reader);
        self.geometry_stats = stats;
        return Ok(())
    }

    fn parse_object(_object_element: &BytesStart, stats: &mut GeometryStatistics) {
        stats.object_count += 1;
    }

    fn parse_vertex(
        vertex_element: &BytesStart,
        stats: &mut GeometryStatistics,
    ) -> Result<Vertex, Error> {
        let x = find_attr_value(vertex_element, b"x")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<f32>().ok())
            .ok_or(MissingAttribute("x".to_string(), "vertex".to_string()))?;

        let y = find_attr_value(vertex_element, b"y")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<f32>().ok())
            .ok_or(MissingAttribute("y".to_string(), "vertex".to_string()))?;

        let z = find_attr_value(vertex_element, b"z")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<f32>().ok())
            .ok_or(MissingAttribute("z".to_string(), "vertex".to_string()))?;
        stats.vertex_count += 1;
        Ok(Vertex { x, y, z })
    }

    fn parse_triangle(
        triangle_element: &BytesStart,
        stats: &mut GeometryStatistics,
    ) -> Result<Triangle, Error> {
        let v1 = find_attr_value(triangle_element, b"v1")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(MissingAttribute("v1".to_string(), "triangle".to_string()))?;

        let v2 = find_attr_value(triangle_element, b"v2")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(MissingAttribute("v2".to_string(), "triangle".to_string()))?;

        let v3 = find_attr_value(triangle_element, b"v3")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(MissingAttribute("v3".to_string(), "triangle".to_string()))?;

        stats.triangle_count += 1;

        Ok(Triangle { v1, v2, v3 })
    }

    fn parse_item(_item_element: &BytesStart, stats: &mut GeometryStatistics) {
        stats.build_items += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_finds_root_part_relationship() {
        let container = open(Path::new("fixtures/core/box.3mf")).unwrap();
        assert_eq!(container.root_part_path, "/3D/3dmodel.model");
    }

    #[test]
    fn open_and_parses_geometry() {
        let mut container = open(Path::new("fixtures/core/box.3mf")).unwrap();
        container.parse_root_part().unwrap();
        assert_eq!(container.geometry_stats.object_count, 1);
        assert_eq!(container.geometry_stats.build_items, 1);
        assert_eq!(container.geometry_stats.vertex_count, 8);
        assert_eq!(container.geometry_stats.triangle_count, 12);
    }
}
