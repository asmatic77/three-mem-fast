use mint::Point3;
use std::collections::HashMap;

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
pub struct Triangle {
    pub(crate) v1: u32,
    pub(crate) v2: u32,
    pub(crate) v3: u32,
}

pub type Transform = [f32; 12];

pub struct Mesh {
    pub vertices: Vec<Point3<f32>>,
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
    pub objects: HashMap<u32, Object>,
    pub build_items: Vec<Item>,
    pub current: Option<Object>,
}
