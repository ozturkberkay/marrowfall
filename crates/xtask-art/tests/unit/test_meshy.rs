use serde_json::json;
use xtask_art::meshy::{
    Task, TaskStatus, animation_body, image_to_3d_body, rigging_body, truncate,
};
use xtask_art::spec::TextureResolution;

fn task(payload: serde_json::Value) -> Task {
    Task {
        id: "t".into(),
        status: TaskStatus::Succeeded,
        progress: 100,
        credits: None,
        payload,
    }
}

#[test]
fn task_status_parses_api_casing() {
    let parse = |s: &str| serde_json::from_str::<TaskStatus>(&format!("\"{s}\"")).unwrap();
    assert_eq!(parse("PENDING"), TaskStatus::Pending);
    assert_eq!(parse("IN_PROGRESS"), TaskStatus::InProgress);
    assert_eq!(parse("SUCCEEDED"), TaskStatus::Succeeded);
    assert_eq!(parse("CANCELED"), TaskStatus::Canceled);
}

/// A status the API adds later must not crash a poll loop that has already
/// been paid for.
#[test]
fn unrecognised_status_falls_back_instead_of_failing() {
    let parsed: TaskStatus = serde_json::from_str("\"EXPIRED\"").unwrap();
    assert_eq!(parsed, TaskStatus::Unknown);
}

#[test]
fn only_finished_states_are_terminal() {
    assert!(!TaskStatus::Pending.is_terminal());
    assert!(!TaskStatus::InProgress.is_terminal());
    assert!(TaskStatus::Succeeded.is_terminal());
    assert!(TaskStatus::Failed.is_terminal());
}

/// Each endpoint puts the GLB somewhere different.
#[test]
fn glb_url_is_found_for_every_endpoint_shape() {
    let mesh = task(json!({"model_urls": {"glb": "https://x/m.glb"}}));
    assert_eq!(mesh.glb_url(), Some("https://x/m.glb"));

    let rigged = task(json!({"result": {"rigged_character_glb_url": "https://x/r.glb"}}));
    assert_eq!(rigged.glb_url(), Some("https://x/r.glb"));

    let animated = task(json!({"result": {"animation_glb_url": "https://x/a.glb"}}));
    assert_eq!(animated.glb_url(), Some("https://x/a.glb"));

    let flat = task(json!({"animation_glb_url": "https://x/f.glb"}));
    assert_eq!(flat.glb_url(), Some("https://x/f.glb"));

    assert_eq!(task(json!({"status": "SUCCEEDED"})).glb_url(), None);
}

#[test]
fn thumbnails_are_collected_for_review() {
    let many = task(json!({"result": {"thumbnail_urls": ["a", "b"]}}));
    assert_eq!(many.thumbnail_urls(), vec!["a", "b"]);

    let single = task(json!({"thumbnail_url": "only"}));
    assert_eq!(single.thumbnail_urls(), vec!["only"]);

    assert!(task(json!({})).thumbnail_urls().is_empty());
}

/// The API rejects a numeric resolution; it must be sent as "2k".
#[test]
fn texture_resolution_is_sent_as_a_string() {
    let body = image_to_3d_body(
        &["data:a".into()],
        30_000,
        true,
        true,
        TextureResolution::K2,
    );
    assert_eq!(body["texture_resolution"], "2k");
}

#[test]
fn model_body_carries_remesh_and_texture_settings() {
    let body = image_to_3d_body(
        &["data:a".into()],
        30_000,
        true,
        true,
        TextureResolution::K2,
    );
    assert_eq!(body["target_polycount"], 30_000);
    assert_eq!(body["topology"], "quad");
    assert_eq!(body["enable_pbr"], true);
    assert_eq!(body["ai_model"], "meshy-7");
    assert_eq!(body["image_urls"].as_array().unwrap().len(), 1);
}

#[test]
fn triangle_topology_is_selectable() {
    let body = image_to_3d_body(&[], 10_000, false, false, TextureResolution::K2);
    assert_eq!(body["topology"], "triangle");
}

/// The rigging endpoint has no body-plan field; sending one risks a 400.
#[test]
fn rigging_body_has_no_body_plan_field() {
    let body = rigging_body("task-1", 1.7);
    assert_eq!(body["input_task_id"], "task-1");
    let height = body["height_meters"].as_f64().unwrap();
    assert!((height - 1.7).abs() < 1e-6, "got {height}");
    assert!(body.get("character_type").is_none());
}

/// Animations are selected by numeric id against `rig_task_id`.
#[test]
fn animation_body_uses_rig_task_id_and_numeric_action() {
    let body = animation_body("rig-1", 92);
    assert_eq!(body["rig_task_id"], "rig-1");
    assert_eq!(body["action_id"], 92);
    assert!(body.get("input_task_id").is_none());
    assert!(body.get("action_name").is_none());
}

/// Truncation runs while reporting another error, so it must never panic.
#[test]
fn truncate_never_splits_a_character() {
    let text = "é".repeat(500);
    let cut = truncate(&text, 300);
    assert!(cut.ends_with('…'));
    assert_eq!(truncate("short", 300), "short");
}

#[test]
fn every_texture_resolution_reaches_the_api_as_the_string_it_expects() {
    // The core type is a plain measurement; this spelling is Meshy's, and a
    // number is rejected on the wire.
    for (resolution, expected) in [
        (TextureResolution::K2, "2k"),
        (TextureResolution::K4, "4k"),
        (TextureResolution::K8, "8k"),
    ] {
        let body = image_to_3d_body(&["data:a".into()], 30_000, true, true, resolution);
        assert_eq!(body["texture_resolution"], expected);
    }
}
