"""Foot planting: contact detection, the vote, the lock and the leg solve."""

import math
import pathlib

import plant
import pytest
from findings import Comparison, Severity
from plant import Leg
from transfer import quat_degrees, quat_multiply

# The thresholds are stated at this height, so a rig of exactly it scales by 1
# and every number below is the published one.
UNSCALED = plant.REFERENCE_HEIGHT_M

LIMITS = {
    plant.PLANTS.id: 1.0,
    plant.SKATE.id: 0.025,
    plant.PENETRATION.id: 0.005,
}


def at(seconds: float) -> tuple[float, float, float]:
    """One physical path, sampled at whatever rate a test asks for.

    Planted still for the first half second, lifted and moving at 1 m/s for a
    quarter, stopped but still lifted for a quarter, then planted again from
    one second on. The landing has no horizontal speed, so both rates read it
    as a contact on the same frame.
    """
    if seconds < 0.5:
        return (0.0, 0.0, 0.0)
    if seconds < 0.75:
        return ((seconds - 0.5) * 1.0, 0.0, 0.2)
    if seconds < 1.0:
        return (0.25, 0.0, 0.2)
    return (0.25, 0.0, 0.0)


def sampled(fps: int, seconds: float = 2.0) -> list[tuple[float, float, float]]:
    return [at(frame / fps) for frame in range(round(seconds * fps))]


# --- the vote --------------------------------------------------------------


@pytest.mark.parametrize("rate", [8, 12, 15, 20, 24, 30, 48, 60, 120])
def test_the_vote_width_is_odd_and_at_least_three_at_every_rate(rate: int) -> None:
    width = plant.vote_width(rate)

    assert width >= 3
    assert width % 2 == 1


def test_the_two_rates_the_library_runs_slowest_and_fastest_both_vote_over_three() -> (
    None
):
    assert plant.vote_width(8) == 3
    assert plant.vote_width(30) == 3


@pytest.mark.parametrize("width", [0, 1, 2, 4, 6])
def test_a_vote_that_cannot_carry_a_majority_is_refused(width: int) -> None:
    with pytest.raises(ValueError, match="odd and at least 3"):
        plant.voted([True, False, True], width)


def test_the_vote_fills_a_one_frame_gap_and_drops_a_one_frame_spike() -> None:
    assert plant.voted([True, True, False, True, True], 3) == [True] * 5
    assert plant.voted([False, False, True, False, False], 3) == [False] * 5


def test_the_vote_holds_the_clip_at_its_ends_rather_than_shrinking_the_window() -> None:
    assert plant.voted([True, False, False], 3) == [True, False, False]


# --- contact ---------------------------------------------------------------


def test_a_path_with_two_plants_reports_both_run_ranges() -> None:
    runs = plant.plant_runs(sampled(30), 30, 1.0)

    assert runs == [(0, 14), (30, 59)]


def test_a_path_that_never_settles_reports_no_run_at_all() -> None:
    flying = [(step * 0.1, 0.0, 0.5) for step in range(20)]

    assert plant.plant_runs(flying, 30, 1.0) == []


def test_the_same_path_at_eight_and_thirty_gives_the_same_runs() -> None:
    slow, fast = (
        plant.plant_runs(sampled(8), 8, 1.0),
        plant.plant_runs(sampled(30), 30, 1.0),
    )

    assert len(slow) == len(fast) == 2
    for (start, end), (other_start, other_end) in zip(slow, fast, strict=True):
        assert start / 8 == pytest.approx(other_start / 30, abs=1 / 8)
        assert end / 8 == pytest.approx(other_end / 30, abs=1 / 8)


def test_a_foot_creeping_below_the_rate_is_a_contact_at_both_rates() -> None:
    """The threshold is meters per second, so the per-frame step it allows is
    3.75x larger at 8 fps than at 30, and the answer is the same."""

    def creep(fps: int) -> list[tuple[float, float, float]]:
        return [(frame / fps * 0.2, 0.0, 0.0) for frame in range(fps)]

    assert plant.plant_runs(creep(8), 8, 1.0) == [(0, 7)]
    assert plant.plant_runs(creep(30), 30, 1.0) == [(0, 29)]


def test_a_foot_low_at_the_first_frame_and_gone_at_the_second_never_plants() -> None:
    """The first frame has no previous one, so it is read against the next.
    Read against itself it is still by construction."""
    leaving = [(step * 0.3, 0.0, step * 0.3) for step in range(8)]

    assert plant.plant_runs(leaving, 30, 1.0) == []


def test_a_taller_rig_scales_the_thresholds_it_is_read_against() -> None:
    assert plant.scale_of(UNSCALED) == 1.0
    assert plant.scale_of(UNSCALED / 2) == 0.5

    with pytest.raises(ValueError, match="no height at all"):
        plant.scale_of(0.0)


def test_a_foot_just_over_the_scaled_ceiling_is_not_a_contact() -> None:
    half = plant.scale_of(UNSCALED / 2)
    just_under = [(0.0, 0.0, plant.CONTACT_HEIGHT_M * half * 0.99)] * 5
    just_over = [(0.0, 0.0, plant.CONTACT_HEIGHT_M * half * 1.01)] * 5

    assert plant.plant_runs(just_under, 30, half) == [(0, 4)]
    assert plant.plant_runs(just_over, 30, half) == []


# --- the lock --------------------------------------------------------------

SLIDE = 0.0
"""`clip.foot_contact.skate`'s limit, zero here so every run is held."""


def test_the_lock_holds_a_run_at_its_first_frame_and_leaves_the_rest_alone() -> None:
    path = [(step * 0.01, step * 0.02, 0.0) for step in range(12)]

    held = plant.locked(path, [(4, 7)], travels=True, slide=SLIDE)

    for frame in (4, 5, 6, 7):
        assert held[frame][0] == pytest.approx(path[4][0], abs=1e-9)
        assert held[frame][1] == pytest.approx(path[4][1], abs=1e-9)
        assert held[frame][2] == path[frame][2]
    for frame in (0, 1, 10, 11):
        assert held[frame] == path[frame]


def test_the_ramp_reaches_the_lock_in_two_frames_and_leaves_it_in_two() -> None:
    path = [(step * 0.1, 0.0, 0.0) for step in range(12)]

    held = plant.locked(path, [(4, 7)], travels=True, slide=SLIDE)

    # Two thirds of the way in one frame out, one third two frames out, and
    # nothing at all three frames out, either side of the run.
    assert held[1][0] == pytest.approx(0.1, abs=1e-9)
    assert held[2][0] == pytest.approx(0.2 + (0.4 - 0.2) / 3, abs=1e-9)
    assert held[3][0] == pytest.approx(0.3 + (0.4 - 0.3) * 2 / 3, abs=1e-9)
    assert held[8][0] == pytest.approx(0.8 + (0.4 - 0.8) * 2 / 3, abs=1e-9)
    assert held[9][0] == pytest.approx(0.9 + (0.4 - 0.9) / 3, abs=1e-9)
    assert held[10][0] == pytest.approx(1.0, abs=1e-9)


def test_two_runs_sharing_a_frame_take_the_stronger_ramp() -> None:
    path = [(step * 0.1, 0.0, 0.0) for step in range(10)]

    held = plant.locked(path, [(0, 2), (5, 7)], travels=True, slide=SLIDE)

    # Frame 3 is one frame out of the first run and two out of the second, so
    # the first run's ramp is the stronger and holds it back towards zero.
    assert held[3][0] == pytest.approx(0.3 + (0.0 - 0.3) * 2 / 3, abs=1e-9)
    # And frame 4 is the other way around, so the second run pulls it forward.
    assert held[4][0] == pytest.approx(0.4 + (0.5 - 0.4) * 2 / 3, abs=1e-9)


def test_an_in_place_clip_is_never_locked_because_its_ground_moves() -> None:
    path = [(step * 0.01, step * 0.02, 0.0) for step in range(12)]

    assert plant.locked(path, [(4, 7)], travels=False, slide=SLIDE) == path
    assert plant.locked(path, [(4, 7)], travels=True, slide=SLIDE) != path


def test_a_run_the_skate_gate_already_accepts_is_left_where_it_is() -> None:
    """Holding a foot costs the leg its own direction, so a drift the
    published limit accepts is not worth the trade."""
    path = [(step * 0.001, 0.0, 0.0) for step in range(12)]

    assert plant.drift(path, (4, 7)) == pytest.approx(0.003)
    assert plant.locked(path, [(4, 7)], travels=True, slide=0.025) == path
    assert plant.locked(path, [(4, 7)], travels=True, slide=0.001) != path


def test_the_drift_inside_a_run_is_read_from_its_first_frame() -> None:
    path = [(0.0, 0.0, 0.0), (0.03, 0.04, 0.0), (0.0, 0.0, 0.5)]

    assert plant.drift(path, (0, 2)) == pytest.approx(0.05)


# --- the leg solve ---------------------------------------------------------


def a_leg() -> Leg:
    """A right leg bent forward at the knee, in Blender Z-up world space."""
    return Leg(
        rest=((1.0, 0.0, 0.0, 0.0),) * 4,
        pose=((1.0, 0.0, 0.0, 0.0),) * 4,
        joints=((0.0, 0.0, 0.9), (0.05, -0.1, 0.55), (0.0, 0.0, 0.1)),
    )


def test_the_solve_reaches_a_target_inside_the_legs_own_length() -> None:
    leg = a_leg()

    reached = plant.bend(leg, (0.03, -0.02, 0.0))

    assert reached.shortfall == pytest.approx(0.0, abs=1e-6)
    assert reached.ankle[0] == pytest.approx(0.03, abs=1e-6)
    assert reached.ankle[1] == pytest.approx(-0.02, abs=1e-6)


def test_the_solve_keeps_both_bone_lengths() -> None:
    leg = a_leg()
    thigh = math.dist(leg.joints[0], leg.joints[1])
    shin = math.dist(leg.joints[1], leg.joints[2])

    reached = plant.bend(leg, (0.1, -0.05, 0.0))

    assert math.dist(leg.joints[0], reached.knee) == pytest.approx(thigh, abs=1e-9)
    assert math.dist(reached.knee, reached.ankle) == pytest.approx(shin, abs=1e-9)


def test_a_target_further_than_the_leg_reaches_reports_the_gap_and_not_a_nan() -> None:
    leg = a_leg()

    reached = plant.bend(leg, (5.0, 0.0, 0.0))

    assert math.isfinite(reached.shortfall)
    assert reached.shortfall > 4.0
    assert all(math.isfinite(part) for part in reached.ankle)


def test_a_target_the_leg_is_folded_around_reports_the_gap_and_not_a_nan() -> None:
    leg = a_leg()

    reached = plant.bend(leg, (0.0, 0.0, 0.79))

    assert math.isfinite(reached.shortfall)
    assert reached.shortfall > 0.0


def test_a_target_on_the_hip_itself_reports_the_gap_and_not_a_nan() -> None:
    """A point the leg hangs from has no direction to reach it along."""
    leg = a_leg()

    reached = plant.bend(leg, (0.0, 0.0, 0.8))

    assert math.isfinite(reached.shortfall)
    assert reached.shortfall > 0.0
    assert all(math.isfinite(part) for part in reached.knee)


def test_a_straight_leg_still_solves() -> None:
    straight = Leg(
        rest=((1.0, 0.0, 0.0, 0.0),) * 4,
        pose=((1.0, 0.0, 0.0, 0.0),) * 4,
        joints=((0.0, 0.0, 0.9), (0.0, 0.0, 0.5), (0.0, 0.0, 0.1)),
    )

    reached = plant.bend(straight, (0.0, 0.0, 0.0))

    assert reached.shortfall == pytest.approx(0.0, abs=1e-6)


def test_the_keys_leave_the_pose_alone_when_nothing_moves() -> None:
    leg = a_leg()

    keys = plant.keys(leg, plant.bend(leg, (0.0, 0.0, 0.0)))

    for key in keys:
        assert quat_degrees(key) == pytest.approx(0.0, abs=1e-6)


def test_the_keys_turn_the_two_leg_bones_and_hold_the_foot_where_it_pointed() -> None:
    leg = a_leg()

    upper, lower, foot = plant.keys(leg, plant.bend(leg, (0.2, 0.0, 0.0)))

    assert quat_degrees(upper) > 1.0
    assert quat_degrees(lower) > 1.0
    # This leg rests with every bone unturned, so the three local keys compose
    # straight back to the foot's own world orientation. Holding that is what
    # carries the toe with the ankle instead of swinging it.
    composed = quat_multiply(upper, quat_multiply(lower, foot))
    assert quat_degrees(composed) == pytest.approx(0.0, abs=1e-9)


# --- the findings ----------------------------------------------------------


@pytest.fixture
def under_a_report(monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path) -> None:
    """Every finding carries the attempt off the path the runner set."""
    monkeypatch.setenv("MARROWFALL_REPORT", str(tmp_path / "retarget.run.1.json"))


@pytest.mark.usefixtures("under_a_report")
def test_a_foot_that_plants_once_a_cycle_reports_information() -> None:
    finding = plant.plants("LeftToeBase", [(0, 5)], True, plant.PLANTS.at(LIMITS))

    assert finding.severity is Severity.INFO
    assert finding.measured == 1.0
    assert finding.comparison is Comparison.GE
    assert (
        finding.message == "LeftToeBase plants 1 time(s) over the clip, on frames 0..5"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_foot_that_never_plants_is_an_error_and_never_a_skate_of_zero() -> None:
    finding = plant.plants("RightToeBase", [], True, plant.PLANTS.at(LIMITS))

    assert finding.severity is Severity.ERROR
    assert finding.measured == 0.0
    assert (
        finding.message
        == "RightToeBase never comes to rest on the ground over the clip"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_clip_the_library_declares_in_place_switches_both_stance_rules_off() -> None:
    counted = plant.plants("LeftToeBase", [], False, plant.PLANTS.at(LIMITS))
    drifted = plant.skate("LeftToeBase", [], [], False, plant.SKATE.at(LIMITS))

    assert counted.severity is Severity.SKIPPED
    assert [finding.severity for finding in drifted] == [Severity.SKIPPED]
    assert "travels: false" in counted.message
    assert "travels: false" in drifted[0].message


@pytest.mark.usefixtures("under_a_report")
def test_the_skate_of_each_run_is_reported_under_its_own_subject() -> None:
    findings = plant.skate(
        "LeftToeBase", [(0, 1), (2, 3)], [0.001, 0.03], True, plant.SKATE.at(LIMITS)
    )

    assert [finding.subject for finding in findings] == [
        "LeftToeBase run 1",
        "LeftToeBase run 2",
    ]
    assert [finding.severity for finding in findings] == [Severity.INFO, Severity.ERROR]
    assert (
        findings[1].message
        == "LeftToeBase drifts 0.0300 m over frames 2..3, where it is planted"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_foot_with_no_run_has_no_drift_to_read_and_says_so() -> None:
    findings = plant.skate("RightToeBase", [], [], True, plant.SKATE.at(LIMITS))

    assert [finding.severity for finding in findings] == [Severity.ERROR]
    assert findings[0].unit == "undefined measurements"
    assert (
        findings[0].message
        == "RightToeBase never plants, so it has no stance to be read across"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_sole_above_the_ground_reports_the_clearance_it_keeps() -> None:
    finding = plant.penetration(
        "LeftToeBase",
        [0.004, 0.002, 0.007],
        [0.009, 0.008, 0.010],
        [0, 1, 2],
        plant.PENETRATION.at(LIMITS),
    )

    assert finding.severity is Severity.INFO
    assert finding.measured == pytest.approx(-0.002)
    assert finding.message == (
        "the sole of LeftToeBase under the toe gets to 0.0020 m over the "
        "ground at frame 1"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_sole_under_the_ground_is_an_error_at_the_depth_it_reaches() -> None:
    finding = plant.penetration(
        "LeftToeBase",
        [0.004, 0.003, 0.007],
        [0.009, -0.02, 0.010],
        [0, 1, 2],
        plant.PENETRATION.at(LIMITS),
    )

    assert finding.severity is Severity.ERROR
    assert finding.measured == pytest.approx(0.02)
    # The depth alone says nothing about where the foot is wrong, so the
    # message names which of the two sole points sank.
    assert finding.message == (
        "the sole of LeftToeBase under the ankle sinks 0.0200 m below the "
        "ground at frame 1"
    )


@pytest.mark.usefixtures("under_a_report")
def test_a_sole_pair_at_one_height_reads_the_toe_and_not_the_ankle() -> None:
    """The toe is read first, so two points at one height name the toe rather
    than whichever the iteration happened to reach last."""
    finding = plant.penetration(
        "LeftToeBase", [-0.01], [-0.01], [4], plant.PENETRATION.at(LIMITS)
    )

    assert "under the toe" in finding.message


@pytest.mark.usefixtures("under_a_report")
def test_a_foot_with_no_frame_has_no_sole_to_read() -> None:
    finding = plant.penetration("LeftToeBase", [], [], [], plant.PENETRATION.at(LIMITS))

    assert finding.severity is Severity.ERROR
    assert finding.unit == "undefined measurements"


def test_every_rule_here_asks_the_runner_for_its_own_published_limit() -> None:
    for rule in plant.RULES:
        assert rule.at(LIMITS).limit == LIMITS[rule.id]
        with pytest.raises(KeyError, match=rule.id):
            rule.at({})
