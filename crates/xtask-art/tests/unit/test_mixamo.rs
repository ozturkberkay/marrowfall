//! The Mixamo protocol's pure half: what we send, and what we make of what
//! comes back. No network here; the client tests use a local server.

use serde_json::json;
use xtask_art::providers::mixamo::client::{
    CHARACTER_ID, Motion, Progress, check_fbx, export_body, products_in, progress_in,
};

/// The 20 byte header every binary FBX starts with, then filler so the
/// length check is not what fails.
fn an_fbx(bytes: usize) -> Vec<u8> {
    let mut fbx = b"Kaydara FBX Binary  \x00".to_vec();
    fbx.resize(bytes.max(22), 0x42);
    fbx
}

/// What the library declares for a Mixamo clip: the export renders at this
/// rate and the retarget reads it at the same one.
const SOURCE_FPS: u32 = 30;

fn a_motion() -> Motion {
    Motion {
        name: "Walking Backward".to_owned(),
        gms_hash: json!({"model-id": 123_530_901, "mirror": false, "inplace": false}),
    }
}

// --- Search ---------------------------------------------------------------

#[test]
fn a_search_response_lists_its_products() {
    let payload = json!({"results": [
        {"id": "c9ccc468", "name": "Walking Backward", "description": "Backward Walk"},
        {"id": "c9c97b90", "name": "Left Strafe", "description": ""},
    ]});
    let products = products_in(&payload);

    assert_eq!(products.len(), 2);
    assert_eq!(products[0].id, "c9ccc468");
    assert_eq!(products[0].name, "Walking Backward");
    // The searchable phrase lives in the description, not the name.
    assert_eq!(products[0].description, "Backward Walk");
}

#[test]
fn a_search_response_of_an_unexpected_shape_lists_nothing() {
    assert!(products_in(&json!({"error": "nope"})).is_empty());
    assert!(products_in(&json!({"results": "not a list"})).is_empty());
    assert!(products_in(&json!({"results": [{"name": "no id"}]})).is_empty());
}

// --- The export request ---------------------------------------------------

#[test]
fn the_export_body_echoes_the_providers_own_parameters() {
    let body = export_body(&a_motion(), SOURCE_FPS).unwrap();

    // gms_hash is opaque: read from the product call and sent back with one
    // key set, so a default like `mirror` stays the provider's.
    assert_eq!(body["gms_hash"][0]["mirror"], json!(false));
    assert_eq!(body["gms_hash"][0]["model-id"], json!(123_530_901));
    assert_eq!(body["product_name"], "Walking Backward");
    assert_eq!(body["type"], "Motion");
    assert_eq!(body["character_id"], CHARACTER_ID);
}

/// Decision 12: every source is fetched traveling, so the femur ratio can
/// size the step to our own body.
///
/// **The key name is unconfirmed.** The only `gms_hash` in this repository is
/// the one `a_motion` writes by hand, and nothing here may touch the network,
/// so this pins that the request carries the flag rather than that Mixamo
/// reads it. `source.traveling` is what catches an in-place export.
#[test]
fn the_export_asks_for_the_clip_with_its_root_motion() {
    let mut in_place = a_motion();
    in_place.gms_hash["inplace"] = json!(true);

    for motion in [a_motion(), in_place] {
        assert_eq!(
            export_body(&motion, SOURCE_FPS).unwrap()["gms_hash"][0]["inplace"],
            json!(false),
            "whatever the product said, the export asks for travel"
        );
    }
}

/// And a `gms_hash` that is not an object at all: the field is opaque, so
/// nothing before this says what shape the product call returned.
#[test]
fn a_gms_hash_that_carries_no_keys_names_the_endpoint_and_the_shape() {
    for (shape, gms_hash) in [("an array", json!([])), ("a string", json!("nope"))] {
        let motion = Motion {
            gms_hash,
            ..a_motion()
        };
        let error = export_body(&motion, SOURCE_FPS).unwrap_err().to_string();

        assert!(error.contains("/animations/export"), "got: {error}");
        assert!(error.contains(shape), "got: {error}");
    }
}

/// The rate is the library's own field rather than a literal, so the rate a
/// clip is rendered at and the rate the retarget reads it at are one number.
#[test]
fn the_export_renders_at_the_rate_the_library_declares() {
    assert_eq!(
        export_body(&a_motion(), 30).unwrap()["preferences"]["fps"],
        "30"
    );
    assert_eq!(
        export_body(&a_motion(), 24).unwrap()["preferences"]["fps"],
        "24"
    );
}

#[test]
fn the_export_asks_for_an_unreduced_fbx_with_its_skin() {
    let body = export_body(&a_motion(), SOURCE_FPS).unwrap();
    assert_eq!(body["preferences"]["format"], "fbx7");
    // The bake resamples anyway, so unreduced keys only bound how smooth that
    // resample can be.
    assert_eq!(body["preferences"]["reducekf"], "0");
    // Measured, not assumed: an FBX with no skin carries no bind pose, and
    // Blender then invents a rest pose, which the retarget depends on.
    assert_eq!(body["preferences"]["skin"], "true");
}

// --- The monitor ----------------------------------------------------------

#[test]
fn a_finished_export_reports_the_file_to_download() {
    let payload = json!({"status": "completed", "job_result": "https://x/motion.fbx"});
    assert_eq!(
        progress_in(&payload).unwrap(),
        Progress::Ready("https://x/motion.fbx".to_owned())
    );
}

#[test]
fn an_export_still_running_asks_to_be_polled_again() {
    assert_eq!(
        progress_in(&json!({"status": "processing"})).unwrap(),
        Progress::Working
    );
    // Undocumented API: an unrecognized state is polled, and the deadline is
    // what stops a run that never finishes.
    assert_eq!(
        progress_in(&json!({"status": "something new"})).unwrap(),
        Progress::Working
    );
    assert_eq!(progress_in(&json!({})).unwrap(), Progress::Working);
}

#[test]
fn a_failed_export_surfaces_the_providers_reason() {
    let payload = json!({"status": "failed", "message": "character not found"});
    let error = progress_in(&payload).unwrap_err().to_string();
    assert!(error.contains("character not found"), "got: {error}");
}

#[test]
fn a_completed_export_with_no_file_fails_rather_than_downloading_nothing() {
    let error = progress_in(&json!({"status": "completed"}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no file"), "got: {error}");
}

// --- Proving the bytes are a clip -----------------------------------------

#[test]
fn a_real_fbx_passes() {
    check_fbx(&an_fbx(60_000)).unwrap();
}

#[test]
fn an_error_page_returned_with_a_success_status_is_caught() {
    let html = b"<!DOCTYPE html><html><body>Service unavailable</body></html>";
    let error = check_fbx(html).unwrap_err().to_string();
    assert!(error.contains("not an FBX"), "got: {error}");
    assert!(error.contains("DOCTYPE"), "quotes what arrived: {error}");
}

#[test]
fn a_truncated_download_is_caught() {
    let error = check_fbx(&an_fbx(64)).unwrap_err().to_string();
    assert!(error.contains("64 bytes"), "got: {error}");
}
