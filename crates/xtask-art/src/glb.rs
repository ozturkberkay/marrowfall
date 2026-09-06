//! The GLB container, opened so the two halves can be edited apart.
//!
//! A binary glTF is a header and a run of chunks: the JSON document first,
//! then the buffer it addresses. Splitting them is what lets a bone be
//! renamed without a vertex moving, because a joint's name is one string in
//! the JSON chunk and every vertex is bytes in the other one.
//!
//! [`gltf`] is the reader everything else here uses. It does not write, and
//! re-encoding a document through a writer would rewrite bytes nobody asked
//! to change, so this edits the two chunks in place instead.

use anyhow::{Context as _, Result, ensure};
use serde_json::Value;

const MAGIC: &[u8; 4] = b"glTF";
const VERSION: u32 = 2;
const JSON_CHUNK: &[u8; 4] = b"JSON";
const BINARY_CHUNK: &[u8; 4] = b"BIN\0";
/// Every chunk is padded to this, JSON with spaces and binary with zeros.
const ALIGNMENT: usize = 4;

/// One binary glTF, as its two chunks.
#[derive(Debug, Clone)]
pub struct Glb {
    /// The JSON chunk, parsed. Edited by name, index or key.
    pub document: Value,
    binary: Vec<u8>,
}

impl Glb {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 12 && &bytes[..4] == MAGIC,
            "not a binary glTF: it does not start with {MAGIC:?}"
        );
        let version = read_u32(bytes, 4)?;
        ensure!(version == VERSION, "glTF version {version}, expected 2");
        let mut json = None;
        let mut binary = Vec::new();
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let length = read_u32(bytes, at)? as usize;
            let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().expect("four bytes");
            let start = at + 8;
            let end = start
                .checked_add(length)
                .filter(|end| *end <= bytes.len())
                .with_context(|| format!("a chunk at byte {at} runs past the end of the file"))?;
            match &kind {
                JSON_CHUNK => json = Some(bytes[start..end].to_vec()),
                BINARY_CHUNK => binary = bytes[start..end].to_vec(),
                // The specification says to skip a chunk type we do not know.
                _ => {}
            }
            at = end;
        }
        let json = json.context("the file carries no JSON chunk")?;
        Ok(Self {
            document: serde_json::from_slice(&json).context("parsing the glTF JSON chunk")?,
            binary,
        })
    }

    /// The buffer chunk, which is what a test hashes to prove a vertex did
    /// not move.
    pub fn binary(&self) -> &[u8] {
        &self.binary
    }

    /// Overwrites `bytes` at `offset` of the buffer chunk.
    ///
    /// The length is fixed by the caller's own accessor, so this refuses a
    /// write that would run past the buffer rather than growing it: the
    /// offsets of everything after it are already recorded in the document.
    pub fn overwrite(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .filter(|end| *end <= self.binary.len())
            .with_context(|| {
                format!(
                    "writing {} bytes at {offset} runs past the {} byte buffer",
                    bytes.len(),
                    self.binary.len()
                )
            })?;
        self.binary[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    /// The file again, with the JSON chunk re-encoded and the buffer as it
    /// stands.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut json = serde_json::to_vec(&self.document).context("encoding the glTF JSON")?;
        json.resize(json.len().next_multiple_of(ALIGNMENT), b' ');
        let mut binary = self.binary.clone();
        binary.resize(binary.len().next_multiple_of(ALIGNMENT), 0);

        let chunks: Vec<(&Vec<u8>, &[u8; 4])> = [(&json, JSON_CHUNK), (&binary, BINARY_CHUNK)]
            .into_iter()
            .filter(|(chunk, _)| !chunk.is_empty())
            .collect();
        let total: usize = 12
            + chunks
                .iter()
                .map(|(chunk, _)| 8 + chunk.len())
                .sum::<usize>();

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(total)
                .context("the file is larger than 4 GiB")?
                .to_le_bytes(),
        );
        for (chunk, kind) in chunks {
            out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(chunk);
        }
        Ok(out)
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let slice = bytes
        .get(at..at + 4)
        .with_context(|| format!("the file ends before byte {at}"))?;
    Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
}
