//! Render-facing view of one simulation tick. These types are the sim→render
//! boundary protocol: plain copyable data, no engine handles, no references
//! back into the world.

use glam::Vec2;

/// One drawable entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityView {
    /// Stable identifier; frontends key scene objects on it across snapshots.
    pub id: u64,
    /// World-space position in tile units.
    pub pos: Vec2,
}

/// Everything a frontend needs to draw one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderSnapshot {
    /// Tick this snapshot describes.
    pub tick: u64,
    /// Simulation time in seconds at that tick.
    pub time: f64,
    pub entities: Vec<EntityView>,
}

impl RenderSnapshot {
    /// Interpolates positions between two snapshots. `alpha` is `0` for
    /// `prev`, `1` for `curr`. Entities new in `curr` snap to it.
    #[must_use]
    pub fn lerp(prev: &Self, curr: &Self, alpha: f32) -> Vec<EntityView> {
        curr.entities
            .iter()
            .map(|now| {
                let before = prev
                    .entities
                    .iter()
                    .find(|p| p.id == now.id)
                    .map_or(now.pos, |p| p.pos);
                EntityView {
                    id: now.id,
                    pos: before.lerp(now.pos, alpha),
                }
            })
            .collect()
    }
}
