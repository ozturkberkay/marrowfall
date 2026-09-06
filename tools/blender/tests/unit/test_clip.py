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
    FPS_GRID,
    FPS_GRID_RANGE,
    LINEAR,
    Channel,
    Defects,
    SourceMotion,
    counted,
    defects,
    on_the_grid,
    source_motion,
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


def test_the_sidecar_carries_each_role_s_rotation_at_each_frame() -> None:
    motion = source_motion(
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
    motion = source_motion(
        rest={"hips": IDENTITY},
        frames={7: {"hips": IDENTITY}, 8: {"hips": IDENTITY}},
        fps=20.0,
        fps_base=1.0,
    )

    assert [frame.seconds for frame in motion.frames] == [0.0, 0.05]


def test_the_frames_come_out_in_order_whatever_order_they_went_in() -> None:
    motion = source_motion(
        rest={"hips": IDENTITY},
        frames={3: {"hips": IDENTITY}, 1: {"hips": IDENTITY}, 2: {"hips": IDENTITY}},
        fps=10.0,
        fps_base=1.0,
    )

    assert [frame.seconds for frame in motion.frames] == pytest.approx([0.0, 0.1, 0.2])


def test_a_clip_with_no_frame_is_refused() -> None:
    with pytest.raises(ValueError, match="no frame"):
        source_motion(rest={"hips": IDENTITY}, frames={}, fps=24.0, fps_base=1.0)


def test_a_rate_that_is_not_positive_is_refused() -> None:
    """The rate turns frames into seconds, and the Rust side aligns the two
    clips on those seconds."""
    with pytest.raises(ValueError, match="rate is positive"):
        source_motion(rest={"hips": IDENTITY}, frames={1: {}}, fps=0.0, fps_base=1.0)


def test_a_scene_rate_with_a_fractional_base_is_refused() -> None:
    """Blender stores 29.97 as 30 over 1.001. Nothing here has been measured
    on such a rate, so it is refused rather than fitted."""
    with pytest.raises(ValueError, match="29.970 fps.*fractional rate"):
        source_motion(
            rest={"hips": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=30.0,
            fps_base=1.001,
        )


def test_a_base_of_no_length_is_refused_rather_than_divided_by() -> None:
    with pytest.raises(ValueError, match="rate is positive"):
        source_motion(
            rest={"hips": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=24.0,
            fps_base=0.0,
        )


def test_a_frame_that_leaves_a_role_out_is_refused() -> None:
    """A role missing from one frame would leave that frame unmeasured."""
    with pytest.raises(ValidationError, match="disagrees with the rest pose"):
        source_motion(
            rest={"hips": IDENTITY, "head": IDENTITY},
            frames={1: {"hips": IDENTITY}},
            fps=24.0,
            fps_base=1.0,
        )


def test_a_source_motion_built_by_hand_with_no_frame_is_refused() -> None:
    with pytest.raises(ValidationError, match="no frame"):
        SourceMotion(rest={"hips": (1.0, 0.0, 0.0, 0.0)}, frames=())


def test_the_sidecar_is_written_where_the_runner_asked(tmp_path: Path) -> None:
    motion = source_motion(
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
