// Test binary: `unwrap()` is the idiomatic assertion here, and a panic is the
// failure report. The workspace forbids it in real code, where a panic is a
// crash.
#![allow(clippy::unwrap_used)]

mod test_chunk;
mod test_hash;
mod test_region;
mod test_rules;
mod test_site;
mod test_stray;
mod test_tile;
