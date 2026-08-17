use xtask_art::openai::{front_prompt, view_prompt};
use xtask_art::spec::View;

#[test]
fn front_is_the_reference_the_others_derive_from() {
    assert_eq!(View::ALL.len(), 4);
    assert_eq!(View::ALL[0], View::Front, "front must be generated first");
    let derived: Vec<View> = View::derived().collect();
    assert_eq!(derived.len(), 3);
    assert!(!derived.contains(&View::Front));
}

#[test]
fn front_prompt_carries_description_and_pose() {
    let prompt = front_prompt("a gaunt skeleton", "T-pose with arms out");
    assert!(prompt.contains("a gaunt skeleton"));
    assert!(prompt.contains("T-pose with arms out"));
    assert!(prompt.contains("FRONT"));
}

#[test]
fn derived_views_demand_consistency_with_the_reference() {
    let prompt = view_prompt(View::Back, "a gaunt skeleton", "T-pose");
    assert!(prompt.contains("SAME character"));
    assert!(prompt.contains("attached image as reference"));
    assert!(prompt.contains("180 degrees"));
}

/// A camera clause is guaranteed by the match rather than a string lookup
/// with a fallback arm, which used to inject a view name verbatim.
#[test]
fn every_view_reaches_the_prompt_with_its_own_camera() {
    let prompts: Vec<String> = View::ALL
        .iter()
        .map(|view| view_prompt(*view, "a survivor", "T-pose"))
        .collect();
    for prompt in &prompts {
        assert!(prompt.contains("eye level"), "no camera clause: {prompt}");
    }
    let unique: std::collections::HashSet<_> = prompts.iter().collect();
    assert_eq!(
        unique.len(),
        View::ALL.len(),
        "cameras must differ per view"
    );
}

#[test]
fn views_render_as_filename_safe_names() {
    assert_eq!(View::Front.to_string(), "front");
    assert_eq!(View::Right.as_str(), "right");
}

/// The concept is a reconstruction reference, so the prompt must exclude
/// the game camera and any scenery that would bake into the texture.
#[test]
fn prompts_forbid_the_game_camera_and_scenery() {
    let prompt = front_prompt("x", "y");
    assert!(prompt.contains("orthographic"));
    assert!(
        prompt.contains("isometric camera"),
        "must exclude the iso angle"
    );
    assert!(prompt.contains("flat"), "lighting must be flat");
}

/// Both profiles came back facing the same way, because "the character's right
/// side" leaves whose right ambiguous. Each must name the frame edge instead.
#[test]
fn the_two_profiles_ask_for_opposite_facings() {
    let left = view_prompt(View::Left, "a survivor", "A-pose");
    let right = view_prompt(View::Right, "a survivor", "A-pose");

    assert!(left.contains("RIGHT edge of the frame"), "got: {left}");
    assert!(right.contains("LEFT edge of the frame"), "got: {right}");
    assert!(
        right.contains("mirror image"),
        "the right view must say it is not a repeat: {right}"
    );
}
