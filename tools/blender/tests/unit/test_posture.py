"""The posture readings: what a clip did to a body, in numbers.

Every case here is a known answer on a synthetic body, because that is the
only way to tell a reading that is right from one that is merely plausible.
A chain tilted 30 degrees forward has to read 30.0, and a limb in line with
itself has to read 0.0.

`posture` never imports `bpy`, so these run under plain pytest with no
Blender.
"""

import math

import posture
import pytest
from skeleton import SKULL_TOP
from transfer import Mat4

CHILD_AXIS = (0.0, 1.0, 0.0)
"""Which of a bone's own axes points at its child, as `[profile]` names it."""


def at(x: float, y: float, z: float) -> Mat4:
    """A joint at one place, its own axes square to the world's."""
    return (
        (1.0, 0.0, 0.0, x),
        (0.0, 1.0, 0.0, y),
        (0.0, 0.0, 1.0, z),
        (0.0, 0.0, 0.0, 1.0),
    )


def axis_down(x: float, y: float, z: float) -> Mat4:
    """A joint at one place whose own +Y points at the floor.

    A quarter turn about X, so the frame stays right handed.
    """
    return (
        (1.0, 0.0, 0.0, x),
        (0.0, 0.0, 1.0, y),
        (0.0, -1.0, 0.0, z),
        (0.0, 0.0, 0.0, 1.0),
    )


def forward_of(joint: Mat4, length: float, degrees: float) -> Mat4:
    """A joint `length` from another, leaning `degrees` forward of straight up.

    Forward is minus Y in Blender, which is the sign every pitch here reads.
    """
    angle = math.radians(degrees)
    return at(
        joint[0][3],
        joint[1][3] - length * math.sin(angle),
        joint[2][3] + length * math.cos(angle),
    )


STANDING: dict[str, Mat4] = {
    "hips": at(0.0, 0.0, 0.9),
    "neck": at(0.0, 0.0, 1.4),
    "head": at(0.0, 0.0, 1.5),
    SKULL_TOP: at(0.0, 0.0, 1.7),
    "left_arm": at(0.2, 0.0, 1.35),
    "right_arm": at(-0.2, 0.0, 1.35),
    "left_forearm": at(0.2, 0.0, 1.05),
    "right_forearm": at(-0.2, 0.0, 1.05),
    "left_hand": axis_down(0.2, 0.0, 0.8),
    "right_hand": axis_down(-0.2, 0.0, 0.8),
}
"""A body standing square: every chain straight up, both arms hanging, and
every hand bone pointing its own axis along the forearm above it. Every
reading of it is 0 by construction, so each case below moves one joint."""


def posed(**edits: Mat4) -> posture.Posed:
    """The standing body with some joints moved, at frame 0."""
    return posture.Posed(frame=0, world=STANDING | edits)


def read(**edits: Mat4) -> dict[str, float | None]:
    return posture.readings(posed(**edits), CHILD_AXIS)


# --- The body that reads zero ---------------------------------------------


def test_a_body_standing_square_reads_zero_everywhere() -> None:
    values = read()

    assert values["hips height"] == pytest.approx(0.9)
    for label, value in values.items():
        if label != "hips height":
            assert value == pytest.approx(0.0, abs=1e-9), label


def test_every_reading_is_labeled_and_ordered_the_way_the_table_says() -> None:
    """The printed columns and the numbers come from one table, so a reading
    cannot be added to one and left out of the other."""
    assert tuple(read()) == tuple(reading.label for reading in posture.READINGS)


# --- Pitch: skull, neck, spine --------------------------------------------


@pytest.mark.parametrize("degrees", [30.0, 12.3, 0.0, -30.0])
def test_a_head_tilted_forward_reads_that_many_degrees(degrees: float) -> None:
    top = forward_of(STANDING["head"], 0.2, degrees)

    assert read(**{SKULL_TOP: top})["skull pitch"] == pytest.approx(degrees)


def test_a_neck_leaning_forward_reads_positive() -> None:
    values = read(head=forward_of(STANDING["neck"], 0.1, 34.9))

    assert values["neck pitch"] == pytest.approx(34.9)


def test_a_spine_leaning_back_reads_negative() -> None:
    values = read(neck=forward_of(STANDING["hips"], 0.5, -8.0))

    assert values["spine lean"] == pytest.approx(-8.0)


# --- The shoulder line ----------------------------------------------------


def test_a_level_shoulder_line_reads_zero() -> None:
    assert read()["shoulder roll"] == pytest.approx(0.0, abs=1e-9)


def test_the_roll_is_positive_when_his_left_arm_joint_rides_higher() -> None:
    """Left and right are his, so a positive roll is his left shoulder up."""
    lifted = read(left_arm=at(0.2, 0.0, 1.55))["shoulder roll"]
    dropped = read(left_arm=at(0.2, 0.0, 1.15))["shoulder roll"]

    assert lifted == pytest.approx(26.5650, abs=1e-4)
    assert dropped == pytest.approx(-26.5650, abs=1e-4)


# --- The arms -------------------------------------------------------------


def test_a_hanging_arm_reads_zero_from_straight_down() -> None:
    assert read()["left upper arm"] == pytest.approx(0.0, abs=1e-9)


def test_an_arm_held_out_sideways_reads_ninety() -> None:
    values = read(left_forearm=at(0.5, 0.0, 1.35))

    assert values["left upper arm"] == pytest.approx(90.0)


def test_a_straight_arm_reads_no_elbow_bend() -> None:
    assert read()["right elbow bend"] == pytest.approx(0.0, abs=1e-9)


def test_a_forearm_folded_forward_reads_its_own_angle() -> None:
    values = read(left_hand=axis_down(0.2, -0.25, 1.05))

    assert values["left elbow bend"] == pytest.approx(90.0)


def test_a_wrist_in_line_with_its_forearm_reads_zero() -> None:
    assert read()["left wrist bend"] == pytest.approx(0.0, abs=1e-9)


def test_a_wrist_across_its_forearm_reads_ninety() -> None:
    """The hand bone's own axis, not the joint below it: a hand is the last
    joint of the chain and has nothing below it to point at."""
    values = read(left_hand=at(0.2, 0.0, 0.8))

    assert values["left wrist bend"] == pytest.approx(90.0)


# --- Readings that do not exist -------------------------------------------


def test_a_rig_with_no_skull_top_reads_nothing_rather_than_zero() -> None:
    world = {name: m for name, m in STANDING.items() if name != SKULL_TOP}

    values = posture.readings(posture.Posed(frame=0, world=world), CHILD_AXIS)

    assert values["skull pitch"] is None
    assert values["neck pitch"] == pytest.approx(0.0, abs=1e-9)


def test_a_rig_with_no_left_arm_reads_nothing_on_that_side() -> None:
    """The shoulder line needs both arm joints, so half a pair reads
    nothing rather than half an answer."""
    arm = {"left_arm", "left_forearm", "left_hand"}
    world = {name: m for name, m in STANDING.items() if name not in arm}

    values = posture.readings(posture.Posed(frame=0, world=world), CHILD_AXIS)

    for label in ("shoulder roll", "left upper arm", "left elbow bend"):
        assert values[label] is None, label
    assert values["left wrist bend"] is None
    assert values["right elbow bend"] == pytest.approx(0.0, abs=1e-9)


def test_two_joints_in_the_same_place_have_no_angle_to_read() -> None:
    values = read(**{SKULL_TOP: STANDING["head"]})

    assert values["skull pitch"] is None


# --- Min, max and mean ----------------------------------------------------


def test_a_span_is_the_lowest_the_highest_and_the_mean() -> None:
    span = posture.span([38.2, 45.4, 40.4])

    assert span is not None
    assert (span.lowest, span.highest) == (38.2, 45.4)
    assert span.mean == pytest.approx(41.3333, abs=1e-4)


def test_a_span_skips_the_frames_that_read_nothing() -> None:
    span = posture.span([None, 10.0, None, 20.0])

    assert span is not None
    assert (span.lowest, span.highest, span.mean) == (10.0, 20.0, 15.0)


def test_a_reading_nothing_ever_answered_has_no_span() -> None:
    assert posture.span([None, None]) is None


# --- Which frames ---------------------------------------------------------


def test_every_frame_is_read_when_none_is_asked_for() -> None:
    assert posture.chosen_frames(range(4), None) == (0, 1, 2, 3)


def test_only_the_frames_asked_for_are_read() -> None:
    assert posture.chosen_frames(range(48), "0,12,24") == (0, 12, 24)


def test_a_frame_this_clip_does_not_have_is_refused_by_number() -> None:
    with pytest.raises(ValueError, match="frame 99 is not in this clip, 0 to 47"):
        posture.chosen_frames(range(48), "0,99")


def test_a_frame_that_is_not_a_number_is_refused() -> None:
    with pytest.raises(ValueError, match="whole numbers, got 'half'"):
        posture.chosen_frames(range(48), "0,half")


def test_a_clip_with_no_frame_has_no_posture_to_read() -> None:
    with pytest.raises(ValueError, match="no frame"):
        posture.chosen_frames(range(0), None)


# --- The printed report ---------------------------------------------------


def test_the_report_prints_one_row_per_frame_and_one_summary_per_reading() -> None:
    text = posture.report(
        "idle.glb on humanoid.glb, standard naming",
        [posed(), posture.Posed(frame=7, world=STANDING)],
        CHILD_AXIS,
    )
    lines = text.splitlines()

    assert lines[0] == "idle.glb on humanoid.glb, standard naming"
    assert [line.split()[0] for line in lines if line.startswith("    ")] == ["0", "7"]
    for reading in posture.READINGS:
        assert reading.label in text, reading.label
        assert reading.column in text, reading.column


def test_the_report_says_what_the_numbers_are_in() -> None:
    text = posture.report("a title", [posed()], CHILD_AXIS)

    assert "Degrees" in text
    assert "meters" in text


def test_the_report_says_n_a_where_a_rig_has_no_joint_to_read() -> None:
    world = {name: m for name, m in STANDING.items() if name != SKULL_TOP}

    text = posture.report("a title", [posture.Posed(frame=0, world=world)], CHILD_AXIS)

    assert "n/a" in text
    assert text.count("n/a") == 4, "one cell, then its min, max and mean"
