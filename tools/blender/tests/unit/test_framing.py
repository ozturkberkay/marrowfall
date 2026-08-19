"""Unit tests for the bake's geometry and scheduling.

`framing` never imports `bpy`, so these run under plain pytest with no Blender.
"""

import itertools
import math

import pytest
from framing import (
    BIND_POSE_TOLERANCE_DEG,
    CAMERA_ELEVATION_DEG,
    DIRECTION_NAMES,
    FRAMING_MARGIN,
    KEY_LIGHT_AZIMUTH_DEG,
    KEY_LIGHT_ELEVATION_DEG,
    BakeSettings,
    Bounds,
    Framing,
    Vec3,
    bind_pose_mismatch,
    bone_direction_angle,
    bone_from_data_path,
    direction_rotation,
    forearm_roll_sign,
    frame_filename,
    is_forearm,
    key_light_rotation,
    missing_bones,
    sampled_frames,
)
from pydantic import ValidationError


def a_framing(lo_z: float = 0.0, hi_z: float = 1.7, radius: float = 0.5) -> Framing:
    """A framing roughly the shape of the survivor, for tests to vary one axis of."""
    return Framing(lo_z=lo_z, hi_z=hi_z, radius=radius)


# --- BakeSettings ---------------------------------------------------------


@pytest.mark.parametrize("directions", [4, 8])
def test_accepts_the_known_direction_rings(directions: int) -> None:
    settings = BakeSettings(directions=directions)
    assert settings.direction_names == DIRECTION_NAMES[directions]
    assert len(settings.direction_names) == directions


@pytest.mark.parametrize("directions", [0, 1, 3, 5, 6, 12, 16])
def test_rejects_unknown_direction_rings(directions: int) -> None:
    with pytest.raises(ValidationError, match="directions must be one of"):
        BakeSettings(directions=directions)


@pytest.mark.parametrize("rate", [0, 61])
def test_rejects_a_sprite_rate_outside_the_sane_range(rate: int) -> None:
    """Rates arrive per animation, inside the `fps` dict, so no field level
    constraint can reach them; the model validator is the only guard."""
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
    for names in DIRECTION_NAMES.values():
        assert names[0] == "s", "direction 0 must face the camera"


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


@pytest.mark.parametrize("count", [4, 8])
def test_direction_rotation_walks_a_full_turn_clockwise(count: int) -> None:
    angles = [direction_rotation(i, count) for i in range(count)]
    assert angles[0] == 0.0, "direction 0 is unrotated"
    assert all(b < a for a, b in itertools.pairwise(angles))
    assert angles[-1] == pytest.approx(-2.0 * math.pi * (count - 1) / count)


@pytest.mark.parametrize("count", [4, 8])
def test_direction_names_follow_the_way_the_model_turns(count: int) -> None:
    """The ring's names must agree with `direction_rotation`'s sign.

    Index 0 faces the camera, which reads as south on screen, and a negative Z
    angle turns the model clockwise, which increases the compass bearing.
    Naming the ring the other way leaves south and north looking correct while
    mirroring every diagonal and swapping east with west, so nothing looks
    wrong until a character walks sideways.
    """
    bearings = {
        "n": 0.0,
        "ne": 45.0,
        "e": 90.0,
        "se": 135.0,
        "s": 180.0,
        "sw": 225.0,
        "w": 270.0,
        "nw": 315.0,
    }
    for index, name in enumerate(DIRECTION_NAMES[count]):
        turned = (180.0 - math.degrees(direction_rotation(index, count))) % 360.0
        assert bearings[name] == pytest.approx(turned), (
            f"index {index} is named {name!r} but the bake turns it to {turned} deg"
        )


@pytest.mark.parametrize("count", [4, 8])
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


# --- Bind pose ------------------------------------------------------------

T_POSE = {"LeftArm": (1.0, 0.0, 0.0), "Hips": (0.0, 0.0, 1.0)}
A_POSE = {"LeftArm": (0.38, 0.07, -0.92), "Hips": (0.0, 0.0, 1.0)}


@pytest.mark.parametrize(
    ("a", "b", "expected"),
    [
        ((1.0, 0.0, 0.0), (1.0, 0.0, 0.0), 0.0),
        ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), 90.0),
        ((1.0, 0.0, 0.0), (-1.0, 0.0, 0.0), 180.0),
        ((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 0.0),
    ],
)
def test_bone_direction_angle(a: Vec3, b: Vec3, expected: float) -> None:
    assert bone_direction_angle(a, b) == pytest.approx(expected, abs=1e-6)


def test_scale_does_not_affect_the_angle() -> None:
    assert bone_direction_angle((2.0, 0.0, 0.0), (0.5, 0.0, 0.0)) == pytest.approx(0.0)


def test_matching_bind_poses_report_nothing() -> None:
    assert bind_pose_mismatch(T_POSE, T_POSE) == []


def test_a_tpose_animation_on_an_apose_rig_is_caught() -> None:
    """The failure that made the survivor flail: 64 degrees of arm offset."""
    off = bind_pose_mismatch(T_POSE, A_POSE)
    assert [name for name, _ in off] == ["LeftArm"], "Hips match, so only the arm"
    assert off[0][1] == pytest.approx(67.0, abs=1.5)


def test_small_differences_are_tolerated() -> None:
    """Two rigs reconstructed separately land a degree or two apart."""
    nearly = {"LeftArm": (1.0, 0.0, -0.05), "Hips": (0.0, 0.0, 1.0)}
    assert bind_pose_mismatch(T_POSE, nearly) == []


def test_worst_offender_is_reported_first() -> None:
    off = bind_pose_mismatch(
        {"a": (1.0, 0.0, 0.0), "b": (1.0, 0.0, 0.0)},
        {"a": (0.0, 1.0, 0.0), "b": (0.7, 0.7, 0.0)},
    )
    assert [name for name, _ in off] == ["a", "b"]


def test_bones_absent_from_either_rig_are_ignored() -> None:
    # missing_bones already reports those, with a better message.
    assert bind_pose_mismatch({"only_here": (1.0, 0.0, 0.0)}, A_POSE) == []


def test_the_tolerance_is_wide_enough_for_noise_and_narrow_enough_for_a_pose() -> None:
    assert 5.0 < BIND_POSE_TOLERANCE_DEG < 60.0
