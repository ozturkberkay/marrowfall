//! Integration tier: real threads and real channels, no engine.
//!
//! This is the first user of the tier. Everything here needs the simulation
//! thread and the generation pool actually running, which is exactly what the
//! unit tier is not allowed to do.

#![allow(clippy::unwrap_used)]

mod test_streaming;
