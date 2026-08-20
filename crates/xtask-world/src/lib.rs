//! `cargo world`: look at a seed without launching the game.
//!
//! Iterating on a generator by playing it costs a rebuild, a launch and a walk.
//! This turns that into a rerun and a picture, which is the difference between
//! tuning terrain and hoping.

pub mod cli;
pub mod paint;

pub use paint::{Shot, render};
