mod error;
mod parser;
mod types;

use crate::error::Error::{self, NoOpenObject};
use std::result::Result;

pub use crate::parser::{Parser3mf, open};
pub use crate::types::{
    GeometryStatistics, Item, Mesh, Object, Scene3mf, Scene3mfBuilder, Transform, Triangle,
};

pub trait MeshSink {
    fn begin_object(&mut self, id: u32) -> Result<(), Error>;
    fn end_object(&mut self) -> Result<(), Error>;
    fn vertex(&mut self, x: f32, y: f32, z: f32) -> Result<(), Error>;
    fn triangle(&mut self, v1: u32, v2: u32, v3: u32) -> Result<(), Error>;
    fn build_item(
        &mut self,
        object_id: u32,
        transform: Option<[f32; 12]>,
        part_number: Option<String>,
    ) -> Result<(), Error>;
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

    fn build_item(
        &mut self,
        object_id: u32,
        transform: Option<[f32; 12]>,
        partname: Option<String>,
    ) -> Result<(), Error> {
        let item = Item {
            object_id,
            transform: transform.map(Into::into),
            partname,
        };
        self.build_items.push(item);
        Ok(())
    }

    fn vertex(&mut self, x: f32, y: f32, z: f32) -> Result<(), Error> {
        let obj = self.current.as_mut().ok_or(Error::NoOpenObject)?;
        obj.mesh.vertices.push(mint::Point3 { x, y, z });
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
