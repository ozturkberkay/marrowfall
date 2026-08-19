//! The four movement actions live in `project.godot`, and no Rust test can ask
//! the engine about them. The names are pinned here, so a lost binding fails a
//! test instead of shipping a game that ignores the keyboard.

/// Embedded and not read, so a moved file fails the build.
const PROJECT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../project/project.godot"
));

/// The keycodes are physical, so the keys stay in the same place on AZERTY.
const ACTIONS: [(&str, u32); 4] = [
    ("move_up", 87),    // W
    ("move_down", 83),  // S
    ("move_left", 65),  // A
    ("move_right", 68), // D
];

#[test]
fn every_movement_action_is_bound_to_its_physical_key() {
    for (action, keycode) in ACTIONS {
        let binding = PROJECT
            .split(&format!("\n{action}="))
            .nth(1)
            .unwrap_or_else(|| panic!("project.godot has no {action} action"));
        let binding = binding.split("\n[").next().unwrap_or(binding);

        assert!(
            binding.contains(&format!("\"physical_keycode\":{keycode}")),
            "{action} is not bound to physical keycode {keycode}"
        );
        // A real device id only ever matches itself.
        assert!(
            binding.contains("\"device\":-1"),
            "{action} is bound to one device instead of all of them"
        );
    }
}
