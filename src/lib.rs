use std::collections::HashMap;
use std::io::BufReader;
use std::num::{ParseFloatError, ParseIntError};
use std::path::Path;
use std::result::Result;
use std::{borrow::Cow, fs::File};

use quick_xml::events::{BytesStart, Event};
use zip::{ZipArchive, result::ZipError};

use crate::Error::{
    InvalidContentType, MissingAttribute, MissingFile, MissingRootPart, NoOpenObject, XmlFormat,
};

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
    #[error("invalid utf-8 in attribute: {0}")]
    WrongUtf8(#[from] std::str::Utf8Error),
    #[error("Wrong float format: {0}")]
    FloatFormatError(#[from] ParseFloatError),
    #[error("Wrong float format: {0}")]
    FFloatFormatError(#[from] fast_float2::Error),
    #[error("Wrong integer format: {0}")]
    IntFormatError(#[from] ParseIntError),
    #[error("Invalid content type for file {0}")]
    InvalidContentType(String),
    #[error("Missing attribute {0} in element {1}")]
    MissingAttribute(String, String),
    #[error("Geometry found outside object")]
    NoOpenObject,
    #[error("Out of memory: could not reserve memory for data")]
    OutOfMemory(#[from] std::collections::TryReserveError),
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Parser3mf {
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

#[derive(Debug, Default)]
#[repr(C)]
pub struct Vertex {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct Triangle {
    v1: u32,
    v2: u32,
    v3: u32,
}

pub type Transform = [f32; 12];

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<Triangle>,
}

pub struct Object {
    pub id: u32,
    pub mesh: Mesh,
    pub name: String,
}

pub struct Item {
    pub object_id: u32,
    pub transform: Option<Transform>, // None means identity Transform
    pub partname: String,
}

pub struct Scene3mf {
    pub objects: HashMap<u32, Object>,
    pub build_items: Vec<Item>,
}

#[derive(Default)]
pub struct Scene3mfBuilder {
    objects: HashMap<u32, Object>,
    build_items: Vec<Item>,
    current: Option<Object>,
}

pub trait MeshSink {
    fn begin_object(&mut self, id: u32) -> Result<(), Error>;
    fn end_object(&mut self) -> Result<(), Error>;
    fn vertex(&mut self, x: f32, y: f32, z: f32) -> Result<(), Error>;
    fn triangle(&mut self, v1: u32, v2: u32, v3: u32) -> Result<(), Error>;
    fn build_item(&mut self, object_id: u32, transform: Option<Transform>) -> Result<(), Error>;
}

impl MeshSink for Scene3mfBuilder {
    fn begin_object(&mut self, id: u32) -> Result<(), Error> {
        if self.current.is_some() {
            return Err(NoOpenObject);
        }
        const INITIAL_VERTEX_RESERVE: usize = 1024;
        const INITIAL_TRIANGLE_RESERVE: usize = INITIAL_VERTEX_RESERVE * 2;
        let mut vertices = Vec::new();
        vertices.try_reserve(INITIAL_VERTEX_RESERVE)?;
        let mut triangles = Vec::new();
        triangles.try_reserve(INITIAL_TRIANGLE_RESERVE)?;
        self.current = Some(Object {
            id,
            mesh: Mesh {
                vertices,
                triangles,
            },
            name: String::new(),
        });
        Ok(())
    }
    fn end_object(&mut self) -> Result<(), Error> {
        let obj = self.current.take().ok_or(NoOpenObject)?;
        self.objects.insert(obj.id, obj);
        // TODO: evaluate call to shrink_to_fit in the triangles and vertices
        Ok(())
    }

    fn build_item(&mut self, object_id: u32, transform: Option<Transform>) -> Result<(), Error> {
        let item = Item {
            object_id,
            transform,
            partname: String::new(),
        };
        self.build_items.push(item);
        Ok(())
    }

    fn vertex(&mut self, x: f32, y: f32, z: f32) -> Result<(), Error> {
        let obj = self.current.as_mut().ok_or(Error::NoOpenObject)?;
        obj.mesh.vertices.push(Vertex { x, y, z });
        Ok(())
    }
    fn triangle(&mut self, v1: u32, v2: u32, v3: u32) -> Result<(), Error> {
        let obj = self.current.as_mut().ok_or(Error::NoOpenObject)?;
        obj.mesh.triangles.push(Triangle { v1, v2, v3 });
        Ok(())
    }
}

impl Scene3mfBuilder {
    pub fn into_scene(self) -> Scene3mf {
        Scene3mf {
            objects: self.objects,
            build_items: self.build_items,
        }
    }
}

pub fn open(path: &Path) -> Result<Parser3mf, Error> {
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

    Ok(Parser3mf {
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
                            //println!("The 3mf root_part path has the correct content type");
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

fn find_vertex(e: &BytesStart, sink: &mut impl MeshSink) -> Result<(), Error> {
    let (mut x, mut y, mut z) = (None, None, None);
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"x" => x = Some(fast_float2::parse(attr.value.as_ref())?),
            b"y" => y = Some(fast_float2::parse(attr.value.as_ref())?),
            b"z" => z = Some(fast_float2::parse(attr.value.as_ref())?),
            _ => {}
        }
    }
    sink.vertex(
        x.ok_or(MissingAttribute("x".to_string(), "vertex".to_string()))?,
        y.ok_or(MissingAttribute("y".to_string(), "vertex".to_string()))?,
        z.ok_or(MissingAttribute("z".to_string(), "vertex".to_string()))?,
    )?;
    Ok(())
}

fn find_triangle(e: &BytesStart, sink: &mut impl MeshSink) -> Result<(), Error> {
    let (mut v1, mut v2, mut v3) = (None, None, None);
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"v1" => v1 = atoi::atoi::<u32>(attr.value.as_ref()),
            b"v2" => v2 = atoi::atoi::<u32>(attr.value.as_ref()),
            b"v3" => v3 = atoi::atoi::<u32>(attr.value.as_ref()),
            _ => {}
        }
    }
    sink.triangle(
        v1.ok_or(MissingAttribute("v1".to_string(), "triangle".to_string()))?,
        v2.ok_or(MissingAttribute("v2".to_string(), "triangle".to_string()))?,
        v3.ok_or(MissingAttribute("v3".to_string(), "triangle".to_string()))?,
    )?;
    Ok(())
}

fn find_attr_value<'a>(e: &'a BytesStart, key: &[u8]) -> Option<Cow<'a, [u8]>> {
    e.attributes()
        .flatten()
        .find(|attr| attr.key.as_ref() == key)
        .map(|attr| attr.value)
}

impl Parser3mf {
    pub fn parse_root_part(&mut self, sink: &mut impl MeshSink) -> Result<(), Error> {
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
        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        MODEL_TAG => (),
                        RESOURCES_TAG => (),
                        OBJECT_TAG => Parser3mf::parse_object(&e, sink, &mut stats)?,
                        MESH_TAG => (),
                        VERTICES_TAG => (),
                        VERTEX_TAG => {
                            //Parser3mf::parse_vertex(&e, sink, &mut stats)?;
                            find_vertex(&e, sink)?;
                            stats.vertex_count += 1;
                        }
                        TRIANGLES_TAG => (),
                        TRIANGLE_TAG => {
                            find_triangle(&e, sink)?;
                            stats.triangle_count += 1;
                        }
                        BUILD_TAG => (),
                        ITEM_TAG => Parser3mf::parse_item(&e, sink, &mut stats)?,
                        _ => (), // TODO: components!
                    }
                }
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == OBJECT_TAG {
                        sink.end_object()?
                    }
                }
                Err(e) => return Err(XmlFormat(e)),
                Ok(Event::Eof) => break,
                _ => (),
            }
            buf.clear();
        }
        drop(xml_reader);
        self.geometry_stats = stats;
        Ok(())
    }

    fn parse_object(
        object_element: &BytesStart,
        sink: &mut impl MeshSink,
        stats: &mut GeometryStatistics,
    ) -> Result<(), Error> {
        let id = find_attr_value(object_element, b"id")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(MissingAttribute("id".to_string(), "object".to_string()))?;
        sink.begin_object(id)?;
        stats.object_count += 1;
        Ok(())
    }
    fn _parse_vertex(
        vertex_element: &BytesStart,
        sink: &mut impl MeshSink,
        stats: &mut GeometryStatistics,
    ) -> Result<(), Error> {
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
        sink.vertex(x, y, z)?;
        Ok(())
    }

    fn _parse_triangle(
        triangle_element: &BytesStart,
        sink: &mut impl MeshSink,
        stats: &mut GeometryStatistics,
    ) -> Result<(), Error> {
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
        sink.triangle(v1, v2, v3)?;
        Ok(())
    }

    fn parse_item(
        item_element: &BytesStart,
        sink: &mut impl MeshSink,
        stats: &mut GeometryStatistics,
    ) -> Result<(), Error> {
        let object_id = find_attr_value(item_element, b"objectid")
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(MissingAttribute("objectid".to_string(), "item".to_string()))?;
        stats.build_items += 1;
        sink.build_item(object_id, None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn open_finds_root_part_relationship() {
        let parser = open(Path::new("fixtures/core/box.3mf")).unwrap();
        assert_eq!(parser.root_part_path, "/3D/3dmodel.model");
    }

    #[test]
    fn open_and_parses_geometry() {
        let mut parser = open(Path::new("fixtures/core/box.3mf")).unwrap();
        let mut builder = Scene3mfBuilder::default();
        parser.parse_root_part(&mut builder).unwrap();
        assert_eq!(parser.geometry_stats.object_count, 1);
        assert_eq!(parser.geometry_stats.build_items, 1);
        assert_eq!(parser.geometry_stats.vertex_count, 8);
        assert_eq!(parser.geometry_stats.triangle_count, 12);
    }

    #[test]
    fn opens_parses_and_stores_geometry() {
        let mut parser = open(Path::new("fixtures/core/box.3mf")).unwrap();
        let mut builder: Scene3mfBuilder = Scene3mfBuilder::default();
        parser.parse_root_part(&mut builder).unwrap();

        assert_eq!(builder.objects.len(), 1);
        assert_eq!(builder.build_items.len(), 1);
        let obj = builder.objects.get(&1).expect("object id 1");
        assert_eq!(obj.mesh.vertices.len(), 8);
        assert_eq!(obj.mesh.triangles.len(), 12);
    }

    #[test]
    fn opens_parses_and_returns_scene() {
        let mut parser = open(Path::new("fixtures/core/box.3mf")).unwrap();
        let mut builder: Scene3mfBuilder = Scene3mfBuilder::default();
        parser.parse_root_part(&mut builder).unwrap();
        let scene = builder.into_scene();
        assert_eq!(scene.objects.len(), 1);
        assert_eq!(scene.build_items.len(), 1);
        let obj = scene.objects.get(&1).expect("object id 1");
        assert_eq!(obj.mesh.vertices.len(), 8);
        assert_eq!(obj.mesh.triangles.len(), 12);
    }
}
