//! Shared test scaffolding.
//!
//! The pipeline reads its API keys and hosts from the environment, so pointing
//! it at a local server means setting process-global state. [`EnvGuard`]
//! serialises that and restores it, so tests in this binary cannot leak
//! settings into each other.

use std::sync::{Mutex, MutexGuard};

use xtask_art::library::AnimationLibrary;
use xtask_art::spec::{Bake, CharacterSpec, CharacterType, Remesh, Subject, Texture};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Holds the environment lock and undoes every variable it set on drop.
/// One per test: the lock is not re-entrant, so nesting two deadlocks.
pub struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    restore: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    pub fn new() -> Self {
        Self {
            // A poisoned lock means another test panicked while holding it; the
            // variables were still restored by this type's Drop, so continuing
            // is correct.
            _lock: ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            restore: Vec::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> &mut Self {
        self.restore.push((key.to_owned(), std::env::var(key).ok()));
        // SAFETY: every test touching the environment holds ENV_LOCK, so no
        // other thread in this binary is reading or writing it concurrently.
        unsafe { std::env::set_var(key, value) };
        self
    }

    pub fn remove(&mut self, key: &str) -> &mut Self {
        self.restore.push((key.to_owned(), std::env::var(key).ok()));
        // SAFETY: as above.
        unsafe { std::env::remove_var(key) };
        self
    }

    /// Points both providers at a local server and supplies dummy keys.
    pub fn with_api(&mut self, base: &str) -> &mut Self {
        self.set("MESHY_API_KEY", "test-key")
            .set("OPENAI_API_KEY", "test-key")
            .set("MARROWFALL_MESHY_BASE_URL", base)
            .set("MARROWFALL_OPENAI_BASE_URL", base)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in self.restore.drain(..).rev() {
            // SAFETY: the lock is still held until this value is fully dropped.
            unsafe {
                match previous {
                    Some(value) => std::env::set_var(&key, value),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}

/// A spec with the same defaults `cargo art new` writes, ready to vary.
pub fn a_spec(name: &str) -> CharacterSpec {
    CharacterSpec {
        name: name.to_owned(),
        subject: Subject {
            kind: CharacterType::Humanoid,
            description: "a lean, weathered survivor in torn shorts".to_owned(),
            height_meters: 1.7,
            skeleton: xtask_art::library::HUMANOID.to_owned(),
        },
        animations: vec!["idle".to_owned()],
        remesh: Remesh {
            target: 30_000,
            quads: true,
        },
        texture: Texture {
            pbr: true,
            resolution: xtask_art::spec::TextureResolution::K2,
        },
        bake: Bake {
            directions: 8,
            render_size: 256,
            sprite_height: 160,
            fps: 12,
            forearm_roll: 0.0,
            trim_start: 0.0,
        },
    }
}

/// A 2x2 PNG, small enough to inline and real enough to decode.
pub fn a_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encoding a 2x2 png");
    bytes
}

/// The library every test resolves names against.
pub fn a_library() -> AnimationLibrary {
    AnimationLibrary::template()
}

/// Writes the library plus a stub GLB for each animation, so a bake finds them.
pub fn install_library(root: &std::path::Path) -> AnimationLibrary {
    let library = a_library();
    library.save(root).expect("saving the library");
    for name in library.animations.keys() {
        let glb = AnimationLibrary::glb(root, name);
        std::fs::create_dir_all(glb.parent().expect("glb has a parent")).expect("mkdir");
        std::fs::write(glb, b"glTF").expect("writing a stub animation");
    }
    library
}
