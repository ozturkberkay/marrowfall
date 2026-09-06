"""What the retarget reports about the clip it just wrote.

The two counted rules are invariants the transfer holds by construction, so
each one gets a negative here rather than only in a hand-run Blender mutation:
a rule with no control that can fail in CI is a rule nobody has seen fail.

The sidecar is the other half. `clip.swing` and `clip.twist` measure the
delivered GLB against the vendor file, and Rust cannot open an FBX, so what
this module records is the only way the source reaches those two rules.
"""

import json
from pathlib import Path

import pytest
from clip import (
    CONSTANT,
    FLOOR_SNAP,
    FPS_GRID,
    FPS_GRID_RANGE,
    LINEAR,
    STRIDE,
    STRIDE_RATIO,
    Channel,
    Defects,
    Fit,
    Ground,
    SourceMotion,
    counted,
    defects,
    floor_lift,
    ground_from,
    on_the_floor,
    on_the_grid,
    placed,
    rest_floor,
    source_motion,
    stride,
    stride_ratio,
    whole_range,
)
from findings import Severity
from pydantic import ValidationError
from transfer import IDENTITY, Mat4

QUARTER_TURN: Mat4 = (
    (0.0, 0.0, 1.0, 0.0),
    (0.0, 1.0, 0.0, 0.0),
    (-1.0, 0.0, 0.0, 0.0),
    (0.0, 0.0, 0.0, 1.0),
)
"""A quarter turn about +Y, written out."""

TURNED = (0.7071067811865476, 0.0, 0.7071067811865475, 0.0)
"""The same rotation as `w, x, y, z`, which is what the sidecar carries."""

FRAMES = range(1, 22)
"""What a bought Mixamo clip spans: frames 1 to 21."""


def a_channel(
    bone: str = "Hips",
    property_name: str = "rotation_quaternion",
    interpolations: tuple[str, ...] = (LINEAR, LINEAR),
    extrapolation: str = CONSTANT,
    frames: tuple[float, ...] = (1.0, 21.0),
) -> Channel:
    return Channel(
        data_path=f'pose.bones["{bone}"].{property_name}',
        interpolations=interpolations,
        extrapolation=extrapolation,
        frames=frames,
    )


def test_a_clip_the_transfer_wrote_has_no_defect() -> None:
    counted = defects([a_channel(), a_channel(bone="Spine")], FRAMES)

    assert counted == {"Hips": Defects(), "Spine": Defects()}


def test_every_bone_is_counted_even_when_it_is_clean() -> None:
    """A rule that reports nothing when it passes cannot be told from a rule
    that never ran."""
    counted = defects([a_channel(bone="LeftHand")], FRAMES)

    assert counted["LeftHand"].not_linear == 0
    assert counted["LeftHand"].outside_range == 0


def test_a_bezier_key_is_a_channel_that_is_not_a_straight_line() -> None:
    bent = a_channel(interpolations=(LINEAR, "BEZIER"))

    assert defects([bent, a_channel(property_name="location")], FRAMES) == {
        "Hips": Defects(not_linear=1)
    }


def test_a_pose_that_is_not_held_outside_the_clip_is_the_same_defect() -> None:
    """Extrapolation is per curve rather than per key, and a linear ramp off
    the end of a clip walks the character away from its last pose."""
    ramped = a_channel(extrapolation="LINEAR")

    assert defects([ramped], FRAMES) == {"Hips": Defects(not_linear=1)}


def test_a_key_before_the_source_range_is_counted() -> None:
    """retarget_bvh leaves its own reference pose keyed at frame 0, which is
    one frame outside a bought clip's own 1 to 21."""
    early = a_channel(interpolations=(LINEAR,) * 3, frames=(0.0, 1.0, 21.0))

    assert defects([early], FRAMES) == {"Hips": Defects(outside_range=1)}


def test_a_key_after_the_source_range_is_counted_too() -> None:
    late = a_channel(interpolations=(LINEAR,) * 3, frames=(1.0, 21.0, 22.0))

    assert defects([late], FRAMES) == {"Hips": Defects(outside_range=1)}


def test_both_counts_add_up_across_a_bone_s_channels() -> None:
    counted = defects(
        [
            a_channel(interpolations=(LINEAR, "BEZIER")),
            a_channel(property_name="location", extrapolation="LINEAR"),
            a_channel(
                property_name="scale",
                interpolations=(LINEAR,) * 3,
                frames=(0.0, 1.0, 21.0),
            ),
        ],
        FRAMES,
    )

    assert counted == {"Hips": Defects(not_linear=2, outside_range=1)}


def test_a_curve_that_drives_no_bone_is_not_a_subject_of_either_rule() -> None:
    """A curve on the object itself would travel the whole character, and
    `clip.object_transform` is the rule that refuses one."""
    assert (
        defects(
            [
                Channel(
                    data_path="location",
                    interpolations=("BEZIER",),
                    extrapolation="LINEAR",
                    frames=(99.0,),
                )
            ],
            FRAMES,
        )
        == {}
    )


@pytest.mark.parametrize(
    ("interpolations", "extrapolation", "straight"),
    [
        ((LINEAR, LINEAR), CONSTANT, True),
        ((), CONSTANT, True),
        ((LINEAR, "BEZIER"), CONSTANT, False),
        ((LINEAR, LINEAR), "LINEAR", False),
    ],
)
def test_a_channel_knows_whether_it_is_a_straight_line(
    interpolations: tuple[str, ...], extrapolation: str, straight: bool
) -> None:
    channel = a_channel(interpolations=interpolations, extrapolation=extrapolation)

    assert channel.straight is straight


# --- the source motion sidecar ---------------------------------------------


def a_sidecar(
    rest: dict[str, Mat4],
    frames: dict[int, dict[str, Mat4]],
    fps: float = 24.0,
    fps_base: float = 1.0,
) -> SourceMotion:
    """`source_motion` with the two lengths filled in, so a test about the
    frames does not restate them. `walk_back`'s own vendor readings."""
    return source_motion(
        rest, frames, fps, fps_base, travel=SOURCE_TRAVEL, stride_segment=SOURCE_FEMUR
    )


SOURCE_TRAVEL = 1.4140
"""How far `walk_back`'s vendor clip travels, in meters."""

SOURCE_FEMUR = 0.4060
"""The Mixamo rig's own stride segment at rest, in meters."""


def test_the_sidecar_carries_the_two_lengths_no_rotation_can_hold() -> None:
    """`clip.stride` and `clip.stride_ratio` read the fit against the vendor
    file, and no Rust reader opens one, so its lengths ride here."""
    motion = a_sidecar(rest={"hips": IDENTITY}, frames={1: {"hips": IDENTITY}})

    assert (motion.travel, motion.stride_segment) == (SOURCE_TRAVEL, SOURCE_FEMUR)


def test_the_sidecar_carries_each_role_s_rotation_at_each_frame() -> None:
    motion = a_sidecar(
        rest={"hips": IDENTITY},
        frames={1: {"hips": QUARTER_TURN}, 2: {"hips": IDENTITY}},
        fps=24.0,
        fps_base=1.0,
    )

    assert motion.rest == {"hips": (1.0, 0.0, 0.0, 0.0)}
    assert len(motion.frames) == 2
    assert motion.frames[0].rotations["hips"] == pytest.approx(TURNED)


def test_the_first_frame_of_the_source_sits_at_zero_seconds() -> None:
    """A Mixamo clip runs frames 1 to 21 and a Meshy one starts at 0, so the
    sidecar counts from the clip's own start rather than from frame 1."""
    motion = a_sidecar(
        rest={"hips": IDENTITY},
        frames={7: {"hips": IDENTITY}, 8: {"hips": IDENTITY}},
        fps=20.0,
        fps_base=1.0,
    )

    assert [frame.seconds for frame in motion.frames] == [0.0, 0.05]


def test_the_frames_come_out_in_order_whatever_order_they_went_in() -> None:
    motion = a_sidecar(
        rest={"hips": IDENTITY},
        frames={3: {"hips": IDENTITY}, 1: {"hips": IDENTITY}, 2: {"hips": IDENTITY}},
        fps=10.0,
        fps_base=1.0,
    )

    assert [frame.seconds for frame in motion.frames] == pytest.approx([0.0, 0.1, 0.2])


def test_a_clip_with_no_frame_is_refused() -> None:
    with pytest.raises(ValueError, match="no frame"):
        a_sidecar(rest={"hips": IDENTITY}, frames={}, fps=24.0, fps_base=1.0)


def test_a_rate_that_is_not_positive_is_refused() -> None:
    """The rate turns frames into seconds, and the Rust side aligns the two
    clips on those seconds."""
    with pytest.raises(ValueError, match="rate is positive"):
        a_sidecar(rest={"hips": IDENTITY}, frames={1: {}}, fps=0.0, fps_base=1.0)


def test_a_scene_rate_with_a_fractional_base_is_refused() -> None:
    """Blender stores 29.97 as 30 over 1.001. Nothing here has been measured
    on such a rate, so it is refused rather than fitted."""
    with pytest.raises(ValueError, match="29.970 fps.*fractional rate"):
        a_sidecar(
            rest={"hips": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=30.0,
            fps_base=1.001,
        )


def test_a_base_of_no_length_is_refused_rather_than_divided_by() -> None:
    with pytest.raises(ValueError, match="rate is positive"):
        a_sidecar(
            rest={"hips": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=24.0,
            fps_base=0.0,
        )


def test_a_frame_that_leaves_a_role_out_is_refused() -> None:
    """A role missing from one frame would leave that frame unmeasured."""
    with pytest.raises(ValidationError, match="disagrees with the rest pose"):
        a_sidecar(
            rest={"hips": IDENTITY, "head": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=24.0,
            fps_base=1.0,
        )


def test_a_source_motion_built_by_hand_with_no_frame_is_refused() -> None:
    with pytest.raises(ValidationError, match="no frame"):
        SourceMotion(
            rest={"hips": (1.0, 0.0, 0.0, 0.0)},
            frames=(),
            travel=SOURCE_TRAVEL,
            stride_segment=SOURCE_FEMUR,
        )


def test_the_sidecar_is_written_where_the_runner_asked(tmp_path: Path) -> None:
    motion = a_sidecar(
        rest={"hips": IDENTITY},
        frames={1: {"hips": IDENTITY}},
        fps=24.0,
        fps_base=1.0,
    )
    path = tmp_path / "reports" / "retarget.run.1.source.json"

    motion.write(path)

    assert json.loads(path.read_text())["rest"] == {"hips": [1.0, 0.0, 0.0, 0.0]}


# --- the findings themselves ----------------------------------------------


@pytest.fixture
def under_a_report(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    """Every finding carries the attempt off the path the runner set."""
    monkeypatch.setenv("MARROWFALL_REPORT", str(tmp_path / "retarget.run.1.json"))


@pytest.mark.usefixtures("under_a_report")
def test_every_bone_reports_on_both_counted_rules() -> None:
    """Including the clean ones: a rule that goes quiet when it passes cannot
    be told from a rule that never ran."""
    findings = counted([a_channel(), a_channel(bone="Spine")], FRAMES)

    assert [(f.rule, f.subject) for f in findings] == [
        ("clip.interpolation", "Hips"),
        ("clip.interpolation", "Spine"),
        ("clip.reference_pose_key", "Hips"),
        ("clip.reference_pose_key", "Spine"),
    ]
    assert all(f.severity is Severity.INFO for f in findings)


@pytest.mark.usefixtures("under_a_report")
def test_a_bezier_channel_is_an_error_the_counting_never_decided() -> None:
    findings = counted([a_channel(interpolations=("BEZIER",))], FRAMES)

    broken = next(f for f in findings if f.rule == "clip.interpolation")
    assert (broken.severity, broken.measured) == (Severity.ERROR, 1.0)


@pytest.mark.usefixtures("under_a_report")
def test_every_key_of_a_clip_on_its_own_grid_reads_nothing() -> None:
    rule = FPS_GRID.at({"clip.fps_grid": 1e-4})

    findings = on_the_grid([1.0, 2.0, 3.0], 30, rule)

    assert [f.subject for f in findings] == [
        "key at frame 1",
        "key at frame 2",
        "key at frame 3",
    ]
    assert all(f.measured == 0.0 for f in findings)


@pytest.mark.usefixtures("under_a_report")
def test_a_thirty_fps_clip_read_at_twenty_four_is_rejected_key_by_key() -> None:
    """Fact 6, exactly: the shipped `strafe_left.glb` spans 0.8 to 16.8 in a
    24 fps scene, and rounding that to 1 to 17 drops four frames."""
    rule = FPS_GRID.at({"clip.fps_grid": 1e-4})

    findings = on_the_grid([0.8, 1.8, 2.8, 16.8], 24, rule)

    assert [round(f.measured, 4) for f in findings] == [0.2, 0.2, 0.2, 0.2]
    assert all(f.severity is Severity.ERROR for f in findings)


@pytest.mark.usefixtures("under_a_report")
def test_the_sampled_frames_are_the_source_s_own_key_times() -> None:
    rule = FPS_GRID_RANGE.at({"clip.fps_grid.range": 0.0})

    holds = whole_range(range(1, 22), [float(f) for f in range(1, 22)], rule)

    assert (holds.severity, holds.measured) == (Severity.INFO, 0.0)
    assert holds.subject == "frames 1..21"


@pytest.mark.usefixtures("under_a_report")
def test_rounding_an_off_grid_range_reports_the_frames_it_dropped() -> None:
    """21 keys at 0.8 to 16.8 round to frames 1 to 17, which is 17 samples."""
    rule = FPS_GRID_RANGE.at({"clip.fps_grid.range": 0.0})

    lost = whole_range(range(1, 18), [0.8 + step for step in range(21)], rule)

    assert (lost.severity, lost.measured) == (Severity.ERROR, 4.0)


# --- The floor, and the stride --------------------------------------------


FLOOR_LIMIT = {"clip.floor_snap": 0.005}
STRIDE_LIMIT = {"clip.stride": 2.0}
RATIO_LIMIT = {"clip.stride_ratio": 100.0}


FLOOR = 0.0307
"""Where the committed rig's rest pose puts its lowest toe joint, in meters.
The ball of the foot, not the sole, which is why the floor is not zero."""


def a_clip_on_the_ground() -> list[Ground]:
    """Two toes over two frames, the left one 0.06 m under the floor."""
    return [
        Ground(bone="LeftToeBase", frame=1, height=FLOOR + 0.02),
        Ground(bone="LeftToeBase", frame=2, height=FLOOR - 0.06),
        Ground(bone="RightToeBase", frame=1, height=FLOOR + 0.01),
        Ground(bone="RightToeBase", frame=2, height=FLOOR + 0.30),
    ]


def test_the_lift_is_what_the_lowest_frame_of_any_toe_needs() -> None:
    assert floor_lift(a_clip_on_the_ground(), FLOOR) == pytest.approx(0.06)


def test_a_clip_that_drives_no_toe_has_nothing_to_lift() -> None:
    assert floor_lift([], FLOOR) == 0.0


@pytest.mark.usefixtures("under_a_report")
def test_a_snapped_clip_leaves_nothing_under_its_lowest_toe() -> None:
    rule = FLOOR_SNAP.at(FLOOR_LIMIT)

    sits = on_the_floor(
        [Ground(bone="LeftToeBase", frame=7, height=FLOOR - 1e-9)], FLOOR, rule
    )

    assert (sits.severity, sits.subject) == (Severity.INFO, "LeftToeBase at frame 7")
    assert sits.measured == pytest.approx(1e-9)


@pytest.mark.usefixtures("under_a_report")
def test_a_clip_with_the_snap_step_removed_is_rejected() -> None:
    """The negative: 0.06 m of toe under the floor, which is what the fit
    reads before it is lifted."""
    rule = FLOOR_SNAP.at(FLOOR_LIMIT)

    sunk = on_the_floor(a_clip_on_the_ground(), FLOOR, rule)

    assert (sunk.severity, sunk.subject) == (
        Severity.ERROR,
        "LeftToeBase at frame 2",
    )
    assert sunk.measured == pytest.approx(0.06)
    assert sunk.message == (
        "LeftToeBase at frame 2 is the lowest any ground joint of the clip "
        "gets, -0.0600 m from the rest height the snap aims at"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_toe_left_hanging_above_the_floor_is_rejected_too() -> None:
    """The other side: nothing in the clip ever reaches the ground, which is
    what a taller source rig leaves once its hips are copied over."""
    rule = FLOOR_SNAP.at(FLOOR_LIMIT)

    floating = on_the_floor(
        [Ground(bone="RightToeBase", frame=3, height=FLOOR + 0.0773)], FLOOR, rule
    )

    assert floating.severity is Severity.ERROR
    assert floating.measured == pytest.approx(0.0773)


@pytest.mark.usefixtures("under_a_report")
def test_a_clip_that_drives_no_toe_reports_undefined_rather_than_zero() -> None:
    rule = FLOOR_SNAP.at(FLOOR_LIMIT)

    nothing = on_the_floor([], FLOOR, rule)

    assert nothing.severity is Severity.ERROR
    assert nothing.unit == "undefined measurements"
    assert "no ground joint" in nothing.message


@pytest.mark.usefixtures("under_a_report")
def test_a_fit_that_travels_what_the_femur_asks_for_holds() -> None:
    """T5's hand measurement: 2.3117 m of source travel at a femur ratio of
    0.8815 comes to 2.0378 m."""
    rule = STRIDE.at(STRIDE_LIMIT)

    sized = stride("strafe_left", 2.0378, 2.3117, 0.8815, travels=True, rule=rule)

    assert (sized.severity, sized.subject) == (Severity.INFO, "strafe_left")
    assert sized.measured == pytest.approx(0.0018, abs=1e-4)
    assert sized.message == (
        "strafe_left travels 2.0378 m against the 2.0378 m its source's "
        "2.3117 m comes to at a femur ratio of 0.8815"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_fit_whose_keys_are_scaled_five_percent_too_far_is_rejected() -> None:
    rule = STRIDE.at(STRIDE_LIMIT)

    stretched = stride(
        "strafe_left", 2.0378 * 1.05, 2.3117, 0.8815, travels=True, rule=rule
    )

    assert stretched.severity is Severity.ERROR
    assert stretched.measured == pytest.approx(5.0, abs=0.01)


@pytest.mark.usefixtures("under_a_report")
def test_a_clip_the_library_declares_in_place_is_skipped_on_its_flag() -> None:
    rule = STRIDE.at(STRIDE_LIMIT)

    idle = stride("run", 0.0, 0.0, 1.0, travels=False, rule=rule)

    assert idle.severity is Severity.SKIPPED
    assert idle.message == (
        "the library declares travels: false, so run has no source travel to "
        "be sized against"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_travelling_clip_whose_source_stands_still_is_undefined() -> None:
    """`travels: true` on a source that never moves leaves no travel to take
    a ratio of, and a relative difference against zero is not a number."""
    rule = STRIDE.at(STRIDE_LIMIT)

    nothing = stride("strafe_left", 2.0, 0.0, 0.8815, travels=True, rule=rule)

    assert nothing.severity is Severity.ERROR
    assert nothing.unit == "undefined measurements"


@pytest.mark.usefixtures("under_a_report")
def test_the_femur_ratio_is_on_record_beside_the_two_lengths_it_came_from() -> None:
    rule = STRIDE_RATIO.at(RATIO_LIMIT)

    recorded = stride_ratio("strafe_left", 0.4123, 0.4677, 0.8815, rule)

    assert (recorded.severity, recorded.measured) == (Severity.INFO, 0.8815)
    assert recorded.message == (
        "our stride segment is 0.4123 m against the source's 0.4677 m, so "
        "every length of strafe_left is sized by 0.8815"
    )


def a_rest_pose(left: float, right: float) -> dict[str, Mat4]:
    """Two toe roles at the heights a rig's rest pose puts them."""
    return {
        "left_toe": at_height(left),
        "right_toe": at_height(right),
        "hips": at_height(0.96),
    }


def at_height(meters: float) -> Mat4:
    return (
        (1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0, meters),
        (0.0, 0.0, 0.0, 1.0),
    )


def test_the_floor_is_the_lower_of_the_two_resting_toes() -> None:
    """The committed rig's own numbers: its two toe joints rest 0.36 mm apart
    and the lower one is where the character stands."""
    floor = rest_floor(a_rest_pose(0.031081, 0.030723), ("left_toe", "right_toe"))

    assert floor == pytest.approx(0.030723)


def test_a_rig_that_fills_no_ground_role_has_no_floor() -> None:
    assert rest_floor(a_rest_pose(0.03, 0.03), ("left_flipper",)) == 0.0


def test_every_toe_of_every_frame_becomes_one_record() -> None:
    paths = {
        "LeftToeBase": [(0.0, 0.0, 0.05), (0.0, 0.0, 0.03)],
        "RightToeBase": [(0.0, 0.0, 0.04), (0.0, 0.0, 0.06)],
    }

    ground = ground_from(paths, [1, 2])

    assert [(g.bone, g.frame, g.height) for g in ground] == [
        ("LeftToeBase", 1, 0.05),
        ("LeftToeBase", 2, 0.03),
        ("RightToeBase", 1, 0.04),
        ("RightToeBase", 2, 0.06),
    ]


def test_a_path_that_is_not_as_long_as_the_frame_range_is_refused() -> None:
    with pytest.raises(ValueError, match="argument"):
        ground_from({"LeftToeBase": [(0.0, 0.0, 0.05)]}, [1, 2])


@pytest.mark.usefixtures("under_a_report")
def test_one_fit_reports_all_three_placement_rules() -> None:
    findings = placed(a_fit(), FLOOR_LIMIT | STRIDE_LIMIT | RATIO_LIMIT)

    assert [f.rule for f in findings] == [
        "clip.floor_snap",
        "clip.stride",
        "clip.stride_ratio",
    ]
    assert all(f.severity is Severity.INFO for f in findings)


def a_fit() -> Fit:
    """A correct fit of `strafe_left`: on the floor, and 2.0378 m across."""
    return Fit(
        name="strafe_left",
        ground=(Ground(bone="LeftToeBase", frame=4, height=FLOOR),),
        floor=FLOOR,
        travel=(2.0378, 2.3117),
        segment=(0.4123, 0.4677),
        ratio=0.8815,
        travels=True,
    )
