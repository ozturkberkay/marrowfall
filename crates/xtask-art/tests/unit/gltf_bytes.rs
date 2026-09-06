//! The glTF container, for the fixture builders that write one.
//!
//! `meshes.rs`, `rigs.rs` and `clips.rs` all build a document and a binary
//! chunk. This is the plumbing they share: the accessor tables, the chunk
//! itself, and the wrapper. Nothing here knows what a mesh or a bone is.

use glam::DMat4;
use serde_json::{Value, json};

/// glTF accessor component types.
pub const FLOAT: u32 = 5126;
pub const UNSIGNED_SHORT: u32 = 5123;
pub const UNSIGNED_INT: u32 = 5125;

/// The binary chunk under construction and the tables that index it.
#[derive(Default)]
pub struct Buffers {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffers {
    /// How long the binary chunk is, which is what the `buffers` entry says.
    pub fn len(&self) -> usize {
        self.binary.len()
    }

    pub fn views(&self) -> &[Value] {
        &self.views
    }

    pub fn accessors(&self) -> &[Value] {
        &self.accessors
    }

    /// Appends raw values to the binary chunk and returns the view index.
    /// Every value the fixtures write is four bytes wide, so the chunk stays
    /// aligned by itself.
    pub fn push_view<T: AsBytes>(&mut self, values: &[T]) -> usize {
        let offset = self.binary.len();
        for value in values {
            self.binary.extend_from_slice(&value.as_bytes());
        }
        self.views.push(json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": self.binary.len() - offset,
        }));
        self.views.len() - 1
    }

    pub fn push_accessor(&mut self, accessor: Value) -> usize {
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// One accessor of `count` elements of `kind`, from four-byte floats.
    pub fn push_floats(&mut self, values: &[f32], kind: &str, count: usize) -> usize {
        let view = self.push_view(values);
        self.push_accessor(json!({
            "bufferView": view,
            "componentType": FLOAT,
            "count": count,
            "type": kind,
        }))
    }

    /// Wraps a document around this chunk, in the GLB container.
    pub fn wrap(&self, document: &Value) -> Vec<u8> {
        glb(&document.to_string(), &self.binary)
    }
}

/// The four-byte types the fixtures write.
pub trait AsBytes {
    fn as_bytes(&self) -> [u8; 4];
}

impl AsBytes for f32 {
    fn as_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
}

impl AsBytes for u32 {
    fn as_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
}

/// A matrix as glTF writes one: sixteen floats, column major.
pub fn columns(matrix: DMat4) -> Vec<f64> {
    matrix.to_cols_array().to_vec()
}

/// Wraps a JSON document and a binary blob in the GLB container. Each chunk
/// is padded to four bytes, JSON with spaces and binary with zeros, which the
/// container requires.
pub fn glb(document: &str, binary: &[u8]) -> Vec<u8> {
    let mut json = document.as_bytes().to_vec();
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bin = binary.to_vec();
    bin.resize(bin.len().next_multiple_of(4), 0);

    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2_u32.to_le_bytes());
    let total = 12 + 8 + json.len() + 8 + bin.len();
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}
