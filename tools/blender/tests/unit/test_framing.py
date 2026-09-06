"""Unit tests for the bake's geometry and scheduling.

`framing` never imports `bpy`, so these run under plain pytest with no Blender.
"""

import itertools
import math
import pathlib

import pytest
from findings import Severity
from framing import (
    BAKE_RULES,
    CAMERA_ELEVATION_DEG,
    DIRECTION_NAMES,
    FRAMING_MARGIN,
    GOLDEN_HEADER,
    KEY_LIGHT_AZIMUTH_DEG,
    KEY_LIGHT_ELEVATION_DEG,
    SAME_BODY,
    BakeSettings,
    Bounds,
    Camera,
    Framing,
    Landmark,
    bone_from_data_path,
    direction_rotation,
    forearm_roll_sign,
    frame_filename,
    frames_are_keys,
    frames_the_action_keys,
    golden_gap,
    golden_landmarks,
    golden_samples,
    golden_text,
    is_forearm,
    key_light_rotation,
    landmark_golden,
    missing_bones,
    off_this_body,
    pin_horizontally,
    project,
    rest_height,
    root_channel_fault,
    root_kept,
    root_travel,
    sampled_frames,
    translation_scale,
    unkeyed_frames,
    worst_axis_travel,
)
from pydantic import ValidationError


def a_framing(lo_z: float = 0.0, hi_z: float = 1.7, radius: float = 0.5) -> Framing:
    """A framing roughly the shape of the survivor, for tests to vary one axis of."""
    return Framing(lo_z=lo_z, hi_z=hi_z, radius=radius)


# --- BakeSettings ---------------------------------------------------------


@pytest.mark.parametrize("directions", [4, 8, 16, 32])
def test_accepts_the_known_direction_rings(directions: int) -> None:
    settings = BakeSettings(directions=directions)
    assert settings.direction_names == DIRECTION_NAMES[directions]
    assert len(settings.direction_names) == directions


@pytest.mark.parametrize("directions", [0, 1, 3, 5, 6, 12, 64])
def test_rejects_unknown_direction_rings(directions: int) -> None:
    with pytest.raises(ValidationError, match="directions must be one of"):
        BakeSettings(directions=directions)


@pytest.mark.parametrize("rate", [0, 61])
def test_rejects_a_sprite_rate_outside_the_sane_range(rate: int) -> None:
    """Rates arrive per animation, inside the `fps` dict, so no field level
    constraint reaches them. The model validator is the only guard."""
    with pytest.raises(ValidationError, match=r"fps must be in 1\.\.=60"):
        BakeSettings(fps={"run": rate})


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("fps", 0),
        ("fps", 61),
        ("size", 8),
        ("trim_start", -0.1),
        ("trim_start", 1.0),
    ],
)
def test_rejects_out_of_range_settings(field: str, value: float) -> None:
    with pytest.raises(ValidationError):
        BakeSettings(**{field: value})


def test_settings_are_frozen_and_reject_unknown_fields() -> None:
    settings = BakeSettings()
    with pytest.raises(ValidationError):
        # Frozen at runtime as well as statically; ty flags the write, which is
        # exactly what this asserts pydantic also does.
        settings.fps = 30  # ty: ignore[invalid-assignment]
    with pytest.raises(ValidationError):
        # A deliberate typo, proving extra="forbid" catches a misspelt field
        # rather than silently ignoring it.
        BakeSettings(directons=8)  # ty: ignore[unknown-argument]


def test_first_direction_faces_the_camera() -> None:
    """Past sixteen the compass runs out of names, so that ring is numbered from
    the same stop."""
    for count, names in DIRECTION_NAMES.items():
        first = "00" if count > 16 else "s"
        assert names[0] == first, f"direction 0 of the {count} ring faces the camera"


def test_the_numbered_ring_counts_round_from_that_stop() -> None:
    """Index and name have to agree, because there is no compass left to notice
    a row landing in the wrong place."""
    assert DIRECTION_NAMES[32] == [f"{index:02d}" for index in range(32)]


# --- Bounds ---------------------------------------------------------------


def test_bounds_size_and_height() -> None:
    bounds = Bounds(lo=(-1.0, -2.0, 0.0), hi=(1.0, 2.0, 1.7))
    assert bounds.size == pytest.approx((2.0, 4.0, 1.7))
    assert bounds.height == pytest.approx(1.7)


# --- Framing --------------------------------------------------------------


def test_framing_rejects_an_empty_vertical_span() -> None:
    with pytest.raises(ValidationError, match="empty vertical span"):
        Framing(lo_z=1.0, hi_z=1.0, radius=0.5)


def test_framing_rejects_a_negative_radius() -> None:
    with pytest.raises(ValidationError):
        Framing(lo_z=0.0, hi_z=1.0, radius=-0.1)


def test_height_footprint_and_center() -> None:
    framing = a_framing(lo_z=0.2, hi_z=1.9, radius=0.6)
    assert framing.height == pytest.approx(1.7)
    assert framing.footprint == pytest.approx(1.2)
    assert framing.center == pytest.approx((0.0, 0.0, 1.05))


def test_ortho_scale_accounts_for_depth_projected_onto_the_vertical_axis() -> None:
    """A tilted camera projects depth onto the image's vertical axis too.

    Sizing from height alone is what let extended poses clip against the edge,
    so the scale must exceed a naive height*margin.
    """
    framing = a_framing(lo_z=0.0, hi_z=1.7, radius=0.9)
    elevation = math.radians(CAMERA_ELEVATION_DEG)
    expected = (
        max(
            framing.footprint,
            framing.height * math.cos(elevation)
            + framing.footprint * math.sin(elevation),
        )
        * FRAMING_MARGIN
    )
    assert framing.ortho_scale == pytest.approx(expected)
    assert framing.ortho_scale > framing.height * math.cos(elevation) * FRAMING_MARGIN


def test_ortho_scale_covers_a_wide_pose_horizontally() -> None:
    """A character reaching wider than it is tall must still fit across."""
    framing = a_framing(lo_z=0.0, hi_z=1.0, radius=3.0)
    assert framing.ortho_scale >= framing.footprint


def test_ortho_scale_leaves_headroom() -> None:
    framing = a_framing()
    unpadded = framing.ortho_scale / FRAMING_MARGIN
    assert framing.ortho_scale > unpadded
    assert FRAMING_MARGIN > 1.0


def test_camera_sits_below_and_behind_looking_down() -> None:
    framing = a_framing()
    x, y, z = framing.camera_location
    assert x == pytest.approx(0.0)
    assert y < 0.0, "camera pulls back along -Y"
    assert z > framing.center[2], "camera sits above the character's midpoint"
    # rot_x of 90 deg looks horizontally; less than that tilts downward.
    assert framing.camera_rotation[0] < math.radians(90.0)
    assert framing.camera_rotation[1:] == (0.0, 0.0)


def test_camera_distance_never_collapses_for_a_tiny_subject() -> None:
    framing = a_framing(lo_z=0.0, hi_z=0.01, radius=0.01)
    _, y, _ = framing.camera_location
    assert abs(y) >= 0.5, "distance is floored so the camera cannot sit inside"


def test_merged_framing_covers_both() -> None:
    idle = a_framing(lo_z=0.0, hi_z=1.7, radius=0.4)
    run = a_framing(lo_z=-0.1, hi_z=1.6, radius=0.9)
    merged = idle.merged(run)
    assert merged.lo_z == pytest.approx(-0.1)
    assert merged.hi_z == pytest.approx(1.7)
    assert merged.radius == pytest.approx(0.9)
    assert merged.ortho_scale >= max(idle.ortho_scale, run.ortho_scale)


def test_merging_is_order_independent() -> None:
    a = a_framing(lo_z=0.0, hi_z=1.7, radius=0.4)
    b = a_framing(lo_z=-0.2, hi_z=1.5, radius=0.8)
    assert a.merged(b).model_dump() == b.merged(a).model_dump()


# --- Lighting and rotation ------------------------------------------------


def test_key_light_comes_from_screen_upper_left() -> None:
    rot_x, rot_y, rot_z = key_light_rotation()
    assert rot_x == pytest.approx(math.radians(90.0 - KEY_LIGHT_ELEVATION_DEG)), (
        "light tilts down from above"
    )
    assert rot_y == 0.0
    assert rot_z == pytest.approx(math.radians(KEY_LIGHT_AZIMUTH_DEG))


@pytest.mark.parametrize("count", [4, 8, 16, 32])
def test_direction_rotation_walks_a_full_turn_clockwise(count: int) -> None:
    angles = [direction_rotation(i, count) for i in range(count)]
    assert angles[0] == 0.0, "direction 0 is unrotated"
    assert all(b < a for a, b in itertools.pairwise(angles))
    assert angles[-1] == pytest.approx(-2.0 * math.pi * (count - 1) / count)


# Compass bearing of every name the 4, 8 and 16 rings use, clockwise from north.
BEARINGS = {
    "n": 0.0,
    "nne": 22.5,
    "ne": 45.0,
    "ene": 67.5,
    "e": 90.0,
    "ese": 112.5,
    "se": 135.0,
    "sse": 157.5,
    "s": 180.0,
    "ssw": 202.5,
    "sw": 225.0,
    "wsw": 247.5,
    "w": 270.0,
    "wnw": 292.5,
    "nw": 315.0,
    "nnw": 337.5,
}


@pytest.mark.parametrize("count", [4, 8, 16])
def test_direction_names_follow_the_way_the_model_turns(count: int) -> None:
    """The ring's names must agree with `direction_rotation`'s sign.

    Index 0 faces the camera, which reads as south on screen. A negative Z angle
    turns the model clockwise, which increases the compass bearing. The other
    naming mirrors every diagonal and swaps east with west, and it leaves south
    and north looking correct. So nothing looks wrong until a character walks
    sideways.
    """
    for index, name in enumerate(DIRECTION_NAMES[count]):
        turned = (180.0 - math.degrees(direction_rotation(index, count))) % 360.0
        assert BEARINGS[name] == pytest.approx(turned), (
            f"index {index} is named {name!r} but the bake turns it to {turned} deg"
        )


@pytest.mark.parametrize("count", [4, 8, 16, 32])
def test_direction_zero_is_unrotated(count: int) -> None:
    """Index 0 is the character as exported, which faces the camera. Adding an
    offset here once turned every sprite around."""
    assert direction_rotation(0, count) == 0.0


# --- Frame naming ---------------------------------------------------------


@pytest.mark.parametrize(
    ("index", "expected"),
    [(0, "run_se_00.png"), (7, "run_se_07.png"), (21, "run_se_21.png")],
)
def test_frame_filenames_are_zero_padded(index: int, expected: str) -> None:
    """`cargo art`'s packer parses these names, so the shape is load-bearing."""
    assert frame_filename("run", "se", index) == expected


def test_frame_filenames_sort_in_playback_order() -> None:
    names = [frame_filename("idle", "s", i) for i in range(12)]
    assert names == sorted(names), "zero padding must keep lexical order == time"


# --- Frame sampling ------------------------------------------------------


def test_frame_count_follows_duration_at_the_requested_rate() -> None:
    # 2 seconds of animation at 24 scene fps, sampled at 12 -> 24 frames.
    frames = sampled_frames(1.0, 49.0, scene_fps=24, fps=12, trim_start=0.0)
    assert len(frames) == 24


@pytest.mark.parametrize(("fps", "expected"), [(6, 12), (12, 24), (24, 48)])
def test_sample_rate_scales_the_frame_count(fps: int, expected: int) -> None:
    frames = sampled_frames(1.0, 49.0, scene_fps=24, fps=fps, trim_start=0.0)
    assert len(frames) == expected


def test_a_longer_animation_gets_more_frames_at_the_same_rate() -> None:
    short = sampled_frames(0.0, 12.0, scene_fps=24, fps=12, trim_start=0.0)
    long = sampled_frames(0.0, 48.0, scene_fps=24, fps=12, trim_start=0.0)
    assert len(long) > len(short), "sampling at a rate must not stretch a short clip"


def test_the_final_frame_is_excluded_so_a_loop_does_not_stutter() -> None:
    frames = sampled_frames(0.0, 24.0, scene_fps=24, fps=12, trim_start=0.0)
    assert frames[0] == 0
    assert 24 not in frames, "the last frame duplicates the first in a loop"


def test_trim_start_skips_a_leading_fraction() -> None:
    full = sampled_frames(0.0, 100.0, scene_fps=24, fps=12, trim_start=0.0)
    trimmed = sampled_frames(0.0, 100.0, scene_fps=24, fps=12, trim_start=0.25)
    assert trimmed[0] == 25
    assert len(trimmed) < len(full)


def test_a_zero_length_animation_still_yields_one_frame() -> None:
    assert sampled_frames(7.0, 7.0, scene_fps=24, fps=12, trim_start=0.0) == [7]


def test_missing_scene_fps_falls_back_rather_than_dividing_by_zero() -> None:
    frames = sampled_frames(0.0, 24.0, scene_fps=0, fps=12, trim_start=0.0)
    assert len(frames) == 12


def test_frames_are_ascending_and_within_range() -> None:
    frames = sampled_frames(5.0, 55.0, scene_fps=24, fps=12, trim_start=0.1)
    assert frames == sorted(frames)
    assert all(5 <= f <= 55 for f in frames)


# --- Bone naming ---------------------------------------------------------


@pytest.mark.parametrize(
    "name",
    ["LeftForeArm", "forearm.L", "RightForeArm", "lowerarm_r", "mixamo:LeftForeArm"],
)
def test_recognises_forearm_bones_across_rig_conventions(name: str) -> None:
    assert is_forearm(name)


@pytest.mark.parametrize("name", ["Hips", "Spine", "LeftHand", "UpperArm.L", "Head"])
def test_other_bones_are_not_forearms(name: str) -> None:
    assert not is_forearm(name)


@pytest.mark.parametrize(
    "name", ["LeftForeArm", "forearm.L", "forearm_l", "leftLowerArm"]
)
def test_left_forearms_roll_positive(name: str) -> None:
    assert forearm_roll_sign(name) == 1.0


@pytest.mark.parametrize("name", ["RightForeArm", "forearm.R", "forearm_r"])
def test_right_forearms_roll_negative(name: str) -> None:
    assert forearm_roll_sign(name) == -1.0


def test_the_two_arms_roll_in_opposite_directions() -> None:
    """Both palms must turn inward, which means mirrored signs."""
    assert forearm_roll_sign("LeftForeArm") == -forearm_roll_sign("RightForeArm")


@pytest.mark.parametrize(
    ("data_path", "expected"),
    [
        ('pose.bones["Hips"].location', "Hips"),
        ('pose.bones["LeftForeArm"].rotation_quaternion', "LeftForeArm"),
        ('pose.bones["mixamo:Spine"].scale', "mixamo:Spine"),
        ("location", None),
        ("rotation_euler", None),
        ('nodes["Background"].inputs[0]', None),
    ],
)
def test_extracts_the_bone_from_an_fcurve_data_path(
    data_path: str, expected: str | None
) -> None:
    assert bone_from_data_path(data_path) == expected


def test_no_missing_bones_when_the_rigs_match() -> None:
    assert missing_bones({"Hips", "Spine"}, {"Hips", "Spine", "Head"}) == []


def test_reports_bones_the_character_does_not_have() -> None:
    """A non-empty result means a mismatched rig, which bakes a frozen sprite."""
    missing = missing_bones({"Hips", "Tail", "Wing"}, {"Hips", "Spine"})
    assert missing == ["Tail", "Wing"], "sorted, so the error message is stable"


# --- Rig proportions ------------------------------------------------------


def test_rest_height_is_the_vertical_span_of_the_rest_bones() -> None:
    points = [(0.0, 0.0, 0.03), (0.5, -0.2, 1.7), (-0.5, 0.2, 0.9)]
    assert rest_height(points) == pytest.approx(1.67)


def test_rest_height_ignores_width_and_depth() -> None:
    """A wide stance is not a tall character."""
    assert rest_height([(-9.0, -9.0, 0.0), (9.0, 9.0, 1.0)]) == pytest.approx(1.0)


def test_a_single_bone_rig_measures_as_no_height() -> None:
    """`translation_scale` is what refuses it, with a message that says why."""
    assert rest_height([(0.0, 0.0, 1.0)]) == 0.0


def test_an_armature_with_no_bones_measures_as_no_height() -> None:
    assert rest_height([]) == 0.0


def test_a_clip_authored_on_this_body_is_left_alone() -> None:
    assert translation_scale(1.7, 1.7) == pytest.approx(1.0)


@pytest.mark.parametrize(
    ("source", "target", "expected"),
    [(1.7, 0.85, 0.5), (0.85, 1.7, 2.0), (1.7, 2.04, 1.2)],
)
def test_a_clip_is_sized_by_the_ratio_of_the_two_rigs(
    source: float, target: float, expected: float
) -> None:
    assert translation_scale(source, target) == pytest.approx(expected)


@pytest.mark.parametrize("source", [0.0, -1.7, math.nan, math.inf])
def test_an_unmeasurable_source_rig_is_refused(source: float) -> None:
    with pytest.raises(ValueError, match="cannot size a clip"):
        translation_scale(source, 1.7)


def test_an_unmeasurable_character_is_refused() -> None:
    with pytest.raises(ValueError, match="cannot size a clip"):
        translation_scale(1.7, math.nan)


@pytest.mark.parametrize("target", [0.1, 17.0])
def test_a_ratio_that_could_only_be_the_wrong_rig_is_refused(target: float) -> None:
    """Half to five times covers a child and a giant; past that is a bad file."""
    with pytest.raises(ValueError, match="wrong rig"):
        translation_scale(1.7, target)


# --- the strip, and what it leaves behind ---------------------------------


@pytest.fixture
def under_a_report(monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path) -> None:
    monkeypatch.setenv("MARROWFALL_REPORT", str(tmp_path / "bake.survivor.1.json"))


def test_pinning_holds_the_horizontal_and_keeps_every_height() -> None:
    """A run's bob is animation, not travel: flattening it too would leave a
    jump permanently on the ground."""
    path = [(1.0, 2.0, 0.0), (3.0, 5.0, 0.1), (4.0, 9.0, -0.2)]

    assert pin_horizontally(path) == [
        (1.0, 2.0, 0.0),
        (1.0, 2.0, 0.1),
        (1.0, 2.0, -0.2),
    ]


def test_pinning_nothing_is_nothing() -> None:
    assert pin_horizontally([]) == []


def test_travel_is_the_worst_frame_and_not_the_last_one() -> None:
    """An endpoint difference cancels a symmetric excursion, which is the
    pattern this design condemns: this path ends where it started."""
    out_and_back = [(0.0, 0.0, 0.0), (0.5, -0.3, 0.1), (0.0, 0.0, 0.0)]

    assert worst_axis_travel(out_and_back) == (0.5, 0.3, 0.1)


def test_a_clip_with_no_frame_has_no_travel_to_measure() -> None:
    with pytest.raises(ValueError, match="no frame"):
        worst_axis_travel([])


BAKE_LIMITS = {"clip.root_travel": 0.02, "clip.root_bob": 0.15}


@pytest.mark.usefixtures("under_a_report")
def test_each_axis_is_read_by_the_rule_that_published_a_limit_for_it() -> None:
    """Two rules, because the strip pins the horizontal axes and keeps the
    vertical one. The bob measures 0.0089 to 0.0535 m on the committed clips,
    which is past the 0.02 m the horizontal pair allows."""
    findings = root_travel(
        "run", "Hips", [(0.0, 0.0, 0.0), (0.0, 0.0, 0.0535)], BAKE_LIMITS
    )

    assert [f.subject for f in findings] == ["run x", "run y", "run z"]
    assert [f.rule for f in findings] == [
        "clip.root_travel",
        "clip.root_travel",
        "clip.root_bob",
    ]
    assert [f.severity for f in findings] == [Severity.INFO] * 3
    assert "0.0535" in findings[2].message


@pytest.mark.usefixtures("under_a_report")
def test_a_root_that_still_slides_horizontally_is_an_error() -> None:
    """0.0428 m on X is what pinning the root's own channels 0 and 1 leaves
    on a left strafe."""
    findings = root_travel(
        "strafe_left", "Hips", [(0.0, 0.0, 0.0), (0.0428, 0.017, 0.0377)], BAKE_LIMITS
    )

    assert findings[0].severity is Severity.ERROR
    assert findings[1].severity is Severity.INFO, "0.017 m is inside the limit"


@pytest.mark.usefixtures("under_a_report")
def test_a_root_sunk_a_third_of_a_meter_is_an_error_on_the_up_axis() -> None:
    """The `[synth]` negative for the bob gate: 0.2911 m is what those same
    channels leave on Z, against a real bob of 0.0377."""
    findings = root_travel(
        "strafe_left", "Hips", [(0.0, 0.0, 0.0), (0.0, 0.0, 0.3)], BAKE_LIMITS
    )

    assert findings[2].severity is Severity.ERROR
    assert findings[2].rule == "clip.root_bob"


@pytest.mark.usefixtures("under_a_report")
def test_keeping_the_root_motion_skips_both_rules_on_every_axis() -> None:
    """A declared flag switched them off, which is the one thing a number
    cannot say."""
    findings = root_kept("run", "Hips", BAKE_LIMITS)

    assert [f.subject for f in findings] == ["run x", "run y", "run z"]
    assert [f.severity for f in findings] == [Severity.SKIPPED] * 3
    assert "--keep-root-motion" in findings[0].message


def test_both_bake_rules_carry_the_ids_the_rust_registry_publishes() -> None:
    assert [rule.id for rule in BAKE_RULES] == ["clip.root_travel", "clip.root_bob"]


def test_a_bone_with_no_location_channel_is_simply_not_keyed() -> None:
    assert root_channel_fault("Hips", "run", []) is None


def test_three_channels_of_one_length_are_what_the_pin_reads() -> None:
    assert root_channel_fault("Hips", "run", [21, 21, 21]) is None


@pytest.mark.parametrize("found", [[21], [21, 21]])
def test_a_root_keyed_on_some_of_its_axes_is_refused_by_name(
    found: list[int],
) -> None:
    """Skipping the bone would leave it traveling with nothing reporting it."""
    fault = root_channel_fault("Hips", "run", found)

    assert fault is not None
    assert "Hips" in fault and f"{len(found)} of 3" in fault


def test_location_channels_of_different_lengths_are_refused() -> None:
    """A pin reads all three channels of one key, so key 20 of X against key
    20 of a shorter Y is a coordinate that does not exist."""
    fault = root_channel_fault("Hips", "run", [21, 21, 20])

    assert fault is not None
    assert "[20, 21]" in fault


def test_three_empty_location_channels_are_refused_too() -> None:
    fault = root_channel_fault("Hips", "run", [0, 0, 0])

    assert fault is not None
    assert "[0]" in fault


# --- the body a clip was authored for -------------------------------------


def test_a_clip_fitted_to_this_body_is_no_distance_from_it() -> None:
    """The retarget already sized every length by the femur, so a clip that
    reaches the bake is on this rig and the bake scales nothing."""
    assert off_this_body(1.7, 1.7) == 0.0


@pytest.mark.parametrize(
    ("source", "target", "expected"),
    [(1.7, 1.87, 0.1), (1.7, 1.53, 0.1), (2.0, 1.0, 0.5)],
)
def test_a_clip_authored_on_another_rig_is_a_distance_from_this_one(
    source: float, target: float, expected: float
) -> None:
    assert off_this_body(source, target) == pytest.approx(expected)


def test_the_f32_a_glb_stores_is_still_the_same_body() -> None:
    """What the three committed clips really read against the committed
    character: 1.227e-6, which is the `f32` a joint position is stored in."""
    assert off_this_body(166.516_913_65, 166.516_709_33) < SAME_BODY


def test_a_rig_a_tenth_of_a_percent_out_is_another_body() -> None:
    assert off_this_body(1.7, 1.7017) > SAME_BODY


# --- which frames the bake renders, and whether anyone authored them ------

GOLDEN_LIMITS = {"bake.sampled_frames_are_keys": 0.0, "bake.landmark_golden": 1.0}


def a_channel(frames: list[float]) -> list[float]:
    """One F-curve's key times, as `bake_sprites` hands them over."""
    return frames


def test_a_frame_any_channel_keys_is_a_frame_somebody_authored() -> None:
    assert frames_the_action_keys([a_channel([0.0, 46.0]), a_channel([0.0, 1.0])]) == {
        0,
        1,
        46,
    }


def test_an_action_with_no_curve_at_all_keys_nothing() -> None:
    assert frames_the_action_keys([]) == set()


def test_the_frames_nothing_authored_are_the_ones_left_over() -> None:
    assert unkeyed_frames([0, 3, 6], {0, 1, 2, 3}) == [6]


@pytest.mark.usefixtures("under_a_report")
def test_every_rendered_frame_of_a_densely_keyed_clip_is_a_key() -> None:
    """What `idle` really reads: 15 rendered frames of the 47 its action
    keys."""
    finding = frames_are_keys(
        "idle",
        [round(frame * 46 / 15) for frame in range(15)],
        [a_channel([float(frame) for frame in range(47)])],
        GOLDEN_LIMITS,
    )

    assert finding.severity is Severity.INFO
    assert finding.measured == 0.0
    assert finding.message == "idle renders 15 frame(s) of the 47 its action keys"


@pytest.mark.usefixtures("under_a_report")
def test_an_action_with_every_other_key_deleted_is_refused() -> None:
    """The `[synth]` negative: half the poses the bake renders would then be
    an interpolation of two nobody authored."""
    finding = frames_are_keys(
        "idle",
        list(range(6)),
        [a_channel([0.0, 2.0, 4.0])],
        GOLDEN_LIMITS,
    )

    assert finding.severity is Severity.ERROR
    assert finding.measured == 3.0
    assert "[1, 3, 5] are keyed by nothing" in finding.message


# --- the bake camera, as a pixel mapper -----------------------------------


SHOULDER_HEIGHT = 1.4


def a_camera(size: int = 512, ortho_scale: float = 2.0) -> Camera:
    """A camera looking along +Y from 4 m back, level and aimed at shoulder
    height: +X is right across the image and +Z is up it."""
    return Camera(
        location=(0.0, -4.0, SHOULDER_HEIGHT),
        right=(1.0, 0.0, 0.0),
        up=(0.0, 0.0, 1.0),
        ortho_scale=ortho_scale,
        size=size,
    )


def test_what_the_camera_points_at_lands_in_the_middle_of_the_frame() -> None:
    assert project((0.0, 0.0, SHOULDER_HEIGHT), a_camera()) == (256, 256)


def test_a_quarter_of_the_ortho_width_is_a_quarter_across_the_canvas() -> None:
    """The width the canvas covers is `ortho_scale`, so half a meter of a two
    meter view is a quarter of 512 pixels."""
    assert project((0.5, 0.0, SHOULDER_HEIGHT), a_camera()) == (384, 256)
    assert project((-0.5, 0.0, SHOULDER_HEIGHT), a_camera()) == (128, 256)


def test_rows_count_down_from_the_top_the_way_an_image_does() -> None:
    assert project((0.0, 0.0, SHOULDER_HEIGHT + 0.5), a_camera()) == (256, 128)
    assert project((0.0, 0.0, SHOULDER_HEIGHT - 0.5), a_camera()) == (256, 384)


def test_moving_along_the_line_of_sight_moves_nothing_on_screen() -> None:
    """Orthographic, so depth is not a scale."""
    camera = a_camera()
    assert project((0.0, 3.0, 1.0), camera) == project((0.0, -3.0, 1.0), camera)


def test_an_arm_rotated_thirty_degrees_moves_it_far_off_its_pixel() -> None:
    """The `[synth]` negative for the golden: a wrist 0.6 m out from the
    shoulder, turned 30 degrees about the body's own axis before it is
    projected, moves 21 px on a 512 px canvas covering 2 m."""
    wrist = (0.6, 0.0, SHOULDER_HEIGHT)
    turned = (
        0.6 * math.cos(math.radians(30.0)),
        0.6 * math.sin(math.radians(30.0)),
        SHOULDER_HEIGHT,
    )
    camera = a_camera()

    assert project(wrist, camera) == (410, 256)
    assert project(turned, camera) == (389, 256)
    assert golden_gap(
        [a_landmark(bone="LeftHand", x=389, y=256)],
        [a_landmark(bone="LeftHand", x=410, y=256)],
    ) == (
        21,
        "LeftHand at frame 0 is 21 px off, (389, 256) against (410, 256)",
    )


# --- the landmark golden --------------------------------------------------


def a_landmark(frame: int = 0, bone: str = "Hips", x: int = 256, y: int = 301):
    return Landmark(frame=frame, bone=bone, x=x, y=y)


def test_a_golden_records_the_first_the_middle_and_the_last_frame() -> None:
    assert golden_samples(15) == [0, 7, 14]
    assert golden_samples(20) == [0, 10, 19]


@pytest.mark.parametrize(("count", "expected"), [(0, []), (1, [0]), (2, [0, 1])])
def test_a_clip_too_short_for_three_records_what_it_has(
    count: int, expected: list[int]
) -> None:
    assert golden_samples(count) == expected


def test_a_golden_is_a_header_and_one_line_per_joint() -> None:
    """The columns are wide enough for `RightShoulder` and for a four digit
    canvas, so nothing in this repository shifts them."""
    text = golden_text([a_landmark(), a_landmark(bone="RightShoulder", x=1024, y=99)])

    assert text.splitlines() == [
        GOLDEN_HEADER,
        "0      Hips              256  301",
        "0      RightShoulder    1024   99",
    ]
    assert text.endswith("\n")


def test_the_header_sits_in_the_same_columns_as_a_row() -> None:
    """A header one character wide of the rows labels the wrong field, which
    is what a reviewer reads the golden by."""
    rows = golden_text([a_landmark()]).splitlines()

    assert len(rows[0]) == len(rows[1]) == 33
    assert rows[0].index("y") == rows[1].index("301") + len("301") - 1


def test_a_golden_reads_back_as_the_landmarks_it_was_written_from() -> None:
    marks = [a_landmark(), a_landmark(frame=7, bone="LeftHand", x=198, y=288)]

    assert golden_landmarks(golden_text(marks)) == marks


def test_the_header_and_anything_else_that_is_not_a_row_is_skipped() -> None:
    assert golden_landmarks(f"{GOLDEN_HEADER}\n\n0 Hips 1 2\nrubbish\n") == [
        a_landmark(x=1, y=2)
    ]


def test_a_run_that_lands_on_every_recorded_pixel_is_no_distance_from_it() -> None:
    marks = [a_landmark(), a_landmark(bone="LeftHand", x=198, y=288)]

    assert golden_gap(marks, list(marks)) == (
        0,
        "every joint is on the pixel the golden records",
    )


def test_the_worst_joint_is_what_the_gap_reports() -> None:
    marks = [a_landmark(), a_landmark(bone="LeftHand", x=198, y=288)]
    moved = [a_landmark(x=258), a_landmark(bone="LeftHand", x=198, y=268)]

    worst, where = golden_gap(marks, moved)

    assert worst == 20
    assert where == "LeftHand at frame 0 is 20 px off, (198, 288) against (198, 268)"


def test_a_golden_of_other_joints_has_no_distance_from_this_pose() -> None:
    """A 26 joint pose has no distance from a 24 joint record, and saying it
    does would compare whatever happened to line up."""
    assert golden_gap([a_landmark()], [a_landmark(bone="Neck")]) is None
    assert golden_gap([a_landmark()], [a_landmark(), a_landmark(frame=7)]) is None


@pytest.mark.usefixtures("under_a_report")
def test_a_missing_golden_is_an_error_and_never_an_auto_accept(
    tmp_path: pathlib.Path,
) -> None:
    finding = landmark_golden(
        "idle_s", tmp_path / "idle_s.txt", [a_landmark()], GOLDEN_LIMITS, update=False
    )

    assert finding.severity is Severity.ERROR
    assert finding.unit == "undefined measurements"
    assert "there is no golden at" in finding.message
    assert "MARROWFALL_UPDATE_GOLDENS=1 writes one" in finding.message


@pytest.mark.usefixtures("under_a_report")
def test_a_run_inside_the_limit_reports_the_joint_that_moved_most(
    tmp_path: pathlib.Path,
) -> None:
    golden = tmp_path / "idle_s.txt"
    golden.write_text(golden_text([a_landmark()]))

    finding = landmark_golden(
        "idle_s", golden, [a_landmark(x=257)], GOLDEN_LIMITS, update=False
    )

    assert finding.severity is Severity.INFO
    assert finding.measured == 1.0
    assert finding.message == (
        "of the 1 joints of idle_s, Hips at frame 0 is 1 px off, (257, 301) "
        "against (256, 301)"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_joint_two_pixels_off_its_golden_is_refused(tmp_path: pathlib.Path) -> None:
    golden = tmp_path / "idle_s.txt"
    golden.write_text(golden_text([a_landmark()]))

    finding = landmark_golden(
        "idle_s", golden, [a_landmark(y=303)], GOLDEN_LIMITS, update=False
    )

    assert finding.severity is Severity.ERROR
    assert finding.measured == 2.0


@pytest.mark.usefixtures("under_a_report")
def test_a_golden_of_another_pose_is_undefined_rather_than_a_distance(
    tmp_path: pathlib.Path,
) -> None:
    golden = tmp_path / "idle_s.txt"
    golden.write_text(golden_text([a_landmark(bone="Neck")]))

    finding = landmark_golden(
        "idle_s", golden, [a_landmark()], GOLDEN_LIMITS, update=False
    )

    assert finding.severity is Severity.ERROR
    assert finding.unit == "undefined measurements"
    assert "records another set of joints or frames" in finding.message


@pytest.mark.usefixtures("under_a_report")
def test_rewriting_a_golden_measures_nothing_at_all(tmp_path: pathlib.Path) -> None:
    """Reading back a file this run just wrote would be the run agreeing with
    itself, so the update is a skip: a declared flag switched the rule off."""
    golden = tmp_path / "goldens" / "idle_s.txt"

    finding = landmark_golden(
        "idle_s", golden, [a_landmark()], GOLDEN_LIMITS, update=True
    )

    assert finding.severity is Severity.SKIPPED
    assert finding.measured == 0.0
    assert "MARROWFALL_UPDATE_GOLDENS=1 rewrote idle_s.txt" in finding.message
    assert golden_landmarks(golden.read_text()) == [a_landmark()]


# --- the goldens this repository committed --------------------------------


def committed_goldens() -> list[pathlib.Path]:
    root = pathlib.Path(__file__).resolve().parents[4]
    found = sorted((root / "art/goldens/survivor").glob("*.txt"))
    assert len(found) == 6, "three clips, two directions each"
    return found


@pytest.mark.parametrize("path", committed_goldens(), ids=lambda path: path.stem)
def test_a_committed_golden_records_a_body_the_right_way_up(
    path: pathlib.Path,
) -> None:
    """The calibration of the projection itself, on all six committed files:
    a camera basis read before the scene was evaluated projected the depth
    where the height belongs, and it put the head 18 px under the hips."""
    marks = golden_landmarks(path.read_text())
    assert len(marks) == 72, "24 joints at three sampled frames"

    for frame in sorted({mark.frame for mark in marks}):
        pose = [mark for mark in marks if mark.frame == frame]
        where = f"frame {frame} of {path.stem}"
        row = {mark.bone: mark.y for mark in pose}
        assert row["Head"] < row["Hips"], where
        # The lowest joint of a body on the ground is a toe. Not the highest:
        # a run swings a forearm past the head, which `run_s` frame 19 does.
        assert max(pose, key=lambda mark: mark.y).bone in {
            "LeftToeBase",
            "RightToeBase",
        }, where


@pytest.mark.parametrize("path", committed_goldens(), ids=lambda path: path.stem)
def test_every_committed_landmark_is_inside_the_canvas_it_was_projected_on(
    path: pathlib.Path,
) -> None:
    """512 px, which is `spec.bake.render_size` for the survivor."""
    for mark in golden_landmarks(path.read_text()):
        assert 0 <= mark.x < 512, f"{mark} in {path.stem}"
        assert 0 <= mark.y < 512, f"{mark} in {path.stem}"
