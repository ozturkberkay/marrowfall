//! ECS components: plain data only. Systems live with the code that runs
//! them, not here.

use glam::Vec2;

/// World-space position in tile units (1.0 = one tile edge). The frontend
/// converts tile units to pixels; the simulation never thinks in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position(pub Vec2);
