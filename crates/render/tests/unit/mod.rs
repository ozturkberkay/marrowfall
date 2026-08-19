// Test binary: `unwrap()` is the idiomatic assertion here, and a panic is the
// failure report. The workspace forbids it in real code, where a panic is a
// crash.
#![allow(clippy::unwrap_used)]

mod test_draw;
mod test_input_map;
mod test_iso;
