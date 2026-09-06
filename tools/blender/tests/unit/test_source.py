"""What a vendor clip is, measured before anything is fitted to it.

Two of the five rules gate, so each one gets a negative here: `travels` typed
wrong has to fail whichever way round it was typed, and a clip whose own rate
is not the declared one has to fail before the retarget spends a minute on it.
The other three record, and what is tested there is that they record.

`source` never imports `bpy`, so these run under plain pytest with no Blender.
"""

import math
import pathlib

import pytest
import source
from findings import Severity
from transfer import Mat4

LIMITS = {
    "source.fps_declared": 0.0,
    "source.traveling": 0.02,
    "source.in_place": 0.02,
    "source.wander": 1000.0,
    "source.child_axis": 180.0,
    "source.posture": 180.0,
}
"""What the runner publishes, read off the rule list in `check/source.rs`."""


def at(x: float, y: float, z: float) -> Mat4:
    """A joint at one place, its own axes square to the world's."""
    return (
        (1.0, 0.0, 0.0, x),
        (0.0, 1.0, 0.0, y),
        (0.0, 0.0, 1.0, z),
        (0.0, 0.0, 0.0, 1.0),
    )


REST: dict[str, Mat4] = {
    "hips": at(0.0, 0.0, 1.0),
    "spine_lower": at(0.0, 1.0, 1.0),
    "neck": at(0.0, 0.0, 1.5),
    "head": at(0.0, 0.0, 1.7),
    "left_arm": at(0.5, 0.0, 1.4),
    "left_forearm": at(0.8, 0.0, 1.4),
}
"""A rig whose `hips` points its own +Y straight at `spine_lower`, so
`source.child_axis` reads 0 there and every angle below is a difference the
test put in on purpose."""


def a_clip(**edits: object) -> source.Clip:
    """One vendor file as Blender read it, ready to vary one field at a time."""
    fields: dict[str, object] = {
        "declared_fps": 30,
        "travels": True,
        "scene_fps": 30.0,
        "key_frames": tuple(float(frame) for frame in range(1, 22)),
        "hips": ((0.0, 0.0, 1.0), (1.0, 0.0, 1.0), (2.3117, 0.0, 1.0)),
        "axis": (0.0, 1.0, 0.0),
        "rest": REST,
        "children": {"hips": "spine_lower", "neck": "head"},
        "posture": (source.Posed(frame=1, world=REST),),
    }
    return source.Clip(**(fields | edits))


@pytest.fixture(autouse=True)
def under_a_report(monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path) -> None:
    """Every finding carries the attempt off the path the runner set."""
    monkeypatch.setenv("MARROWFALL_REPORT", str(tmp_path / "fetch.strafe_left.1.json"))


def only(findings: list, rule: str):
    return [finding for finding in findings if finding.rule == rule]


# --- the rate -------------------------------------------------------------


def test_a_clip_keyed_on_whole_frames_runs_at_the_scene_s_own_rate() -> None:
    """Which is an FBX: the importer sets the scene from the file."""
    assert source.rate(30.0, [1.0, 2.0, 3.0]) == 30.0


def test_a_thirty_fps_clip_read_at_twenty_four_still_reads_thirty() -> None:
    """glTF stores key times in seconds and the importer converts them at the
    scene's rate, so the spacing carries the file's own rate either way."""
    assert source.rate(24.0, [0.0, 0.8, 1.6, 2.4]) == pytest.approx(30.0)


def test_a_duplicated_key_cannot_halve_the_rate() -> None:
    """The median spacing, not the mean."""
    assert source.rate(24.0, [1.0, 1.0, 2.0, 3.0, 4.0]) == 24.0


@pytest.mark.parametrize(
    ("scene_fps", "keys"), [(24.0, [1.0]), (24.0, []), (0.0, [1.0, 2.0])]
)
def test_a_clip_with_no_spacing_has_no_rate(
    scene_fps: float, keys: list[float]
) -> None:
    assert source.rate(scene_fps, keys) is None


def test_the_declared_rate_holds_when_the_file_agrees() -> None:
    finding = only(source.findings(a_clip(), LIMITS), "source.fps_declared")[0]

    assert (finding.severity, finding.measured) == (Severity.INFO, 0.0)
    assert "30 fps" in finding.message


def test_a_library_that_declares_the_wrong_rate_is_refused() -> None:
    """The design's own negative: `source_fps` 24 against a 30 fps file."""
    finding = only(
        source.findings(a_clip(declared_fps=24), LIMITS), "source.fps_declared"
    )[0]

    assert (finding.severity, finding.measured) == (Severity.ERROR, 6.0)


def test_a_clip_with_one_key_reports_an_undefined_rate_rather_than_a_nan() -> None:
    finding = only(
        source.findings(a_clip(key_frames=(1.0,)), LIMITS), "source.fps_declared"
    )[0]

    assert finding.severity is Severity.ERROR
    assert finding.unit == "undefined measurements"


# --- the travel pair ------------------------------------------------------


def test_travel_is_where_the_hips_end_up_and_not_how_far_they_wandered() -> None:
    """A run cycle in place sways 0.028 m sideways and comes back exactly.
    Calling that travel would declare every in-place clip a traveling one."""
    swayed = [(0.0, 0.0, 1.0), (0.028, 0.0, 1.0), (0.0, 0.0, 1.0)]

    assert source.travel(swayed) == 0.0


def test_travel_ignores_the_bob() -> None:
    """Fact 5 measures a run's at 0.043 m, twice this rule's whole threshold."""
    assert source.travel([(0.0, 0.0, 1.0), (0.0, 0.0, 1.05)]) == 0.0


def test_a_clip_with_no_frame_has_no_travel_to_measure() -> None:
    with pytest.raises(ValueError, match="no frame"):
        source.travel(())


def test_a_traveling_clip_declared_so_holds_and_skips_the_other_half() -> None:
    findings = source.findings(a_clip(), LIMITS)
    travels = only(findings, "source.traveling")[0]
    in_place = only(findings, "source.in_place")[0]

    assert (travels.severity, round(travels.measured, 4)) == (Severity.INFO, 2.3117)
    assert in_place.severity is Severity.SKIPPED


def test_an_in_place_export_declared_traveling_is_refused() -> None:
    """Half of the symmetry: the clip never leaves the origin."""
    findings = source.findings(a_clip(hips=((0.0, 0.0, 1.0), (0.0, 0.0, 1.0))), LIMITS)

    assert only(findings, "source.traveling")[0].severity is Severity.ERROR


def test_a_traveling_export_declared_in_place_is_refused() -> None:
    """And the other half, so a mistyped flag cannot skip the gate."""
    findings = source.findings(a_clip(travels=False), LIMITS)

    assert only(findings, "source.in_place")[0].severity is Severity.ERROR
    assert only(findings, "source.traveling")[0].severity is Severity.SKIPPED


def test_wander_is_the_furthest_the_hips_get_and_not_where_they_ended() -> None:
    """The reading `travel` cancels. Nothing downstream can take it: the bake
    pins the horizontal axes onto the first frame, so `clip.root_travel` only
    ever sees a residual."""
    out_and_back = [(0.0, 0.0, 1.0), (0.6, 0.0, 1.0), (0.0, 0.0, 1.0)]

    assert source.travel(out_and_back) == 0.0
    assert source.wander(out_and_back) == pytest.approx(0.6)


def test_wander_ignores_the_bob_the_same_way_travel_does() -> None:
    assert source.wander([(0.0, 0.0, 1.0), (0.0, 0.0, 1.6)]) == 0.0


def test_a_clip_with_no_frame_has_no_path_to_measure() -> None:
    with pytest.raises(ValueError, match="no frame"):
        source.wander(())


def test_the_excursion_is_recorded_rather_than_gated() -> None:
    """Measured: `idle` 0.0112 m, `run` 0.0276, `walk_back` 1.2712 and
    `strafe_left.fbx` 2.3117. An in-place cycle and a strafe are two orders
    apart, so no one threshold reads both, and this rule records instead."""
    wandered = only(source.findings(a_clip(), LIMITS), "source.wander")[0]

    assert round(wandered.measured, 4) == 2.3117
    assert wandered.severity is Severity.INFO
    assert wandered.limit == 1000.0


# --- the two rules that record --------------------------------------------


def test_each_bone_s_own_axis_is_recorded_against_its_mapped_child() -> None:
    """Never gated: a vendor skeleton is not ours to regenerate, so a limit
    that could fail would fail forever. Mixamo's `Neck` reads 16.933 degrees
    off the direction to its own `Head` and its `Hips` 7.051."""
    findings = only(source.findings(a_clip(), LIMITS), "source.child_axis")

    assert [(f.subject, round(f.measured, 3)) for f in findings] == [
        ("hips", 0.0),
        ("neck", 90.0),
    ]
    assert all(f.severity is Severity.INFO for f in findings)


def test_a_child_the_file_does_not_have_is_left_out_rather_than_guessed() -> None:
    clip = a_clip(children={"hips": "spine_lower", "neck": "left_hand"})

    assert [
        f.subject for f in only(source.findings(clip, LIMITS), "source.child_axis")
    ] == ["hips"]


def test_two_joints_at_the_same_place_have_no_direction_between_them() -> None:
    clip = a_clip(rest=REST | {"spine_lower": at(0.0, 0.0, 1.0)})

    hips = only(source.findings(clip, LIMITS), "source.child_axis")[0]
    assert hips.severity is Severity.ERROR
    assert hips.unit == "undefined measurements"


def test_the_posture_is_read_as_joints_and_not_as_bone_axes() -> None:
    """Fact 17: `strafe_left` is authored with the head 34 to 37 degrees
    forward while its head bone points 1.0 degrees off vertical, so a bone
    axis would miss the hunch entirely."""
    leaning = REST | {"head": at(0.0, 0.2, 1.7)}
    clip = a_clip(posture=(source.Posed(frame=11, world=leaning),))

    findings = only(source.findings(clip, LIMITS), "source.posture")
    head = next(f for f in findings if f.subject.startswith("head pitch"))
    assert head.subject == "head pitch at frame 11"
    assert head.measured == pytest.approx(math.degrees(math.atan2(0.2, 0.2)))
    assert head.severity is Severity.INFO


def test_a_reading_the_clip_has_no_roles_for_is_left_out() -> None:
    clip = a_clip(posture=(source.Posed(frame=1, world={"neck": REST["neck"]}),))

    assert only(source.findings(clip, LIMITS), "source.posture") == []


@pytest.mark.parametrize(
    ("frames", "expected"),
    [(range(1, 22), (1, 11, 21)), (range(2), (0, 1)), (range(3, 4), (3,))],
)
def test_a_posture_is_read_at_the_two_ends_and_the_middle(
    frames: range, expected: tuple[int, ...]
) -> None:
    assert source.sampled_frames(frames) == expected


def test_a_clip_with_no_frame_has_no_posture_to_read() -> None:
    with pytest.raises(ValueError, match="no frame"):
        source.sampled_frames(range(0))


def test_a_direction_of_no_length_is_no_direction_at_all() -> None:
    assert source.degrees_between((0.0, 0.0, 0.0), (0.0, 0.0, 1.0)) is None


def test_every_rule_reports_on_every_clip() -> None:
    """A rule that measures nothing cannot be told from one that never ran."""
    reported = {finding.rule for finding in source.findings(a_clip(), LIMITS)}

    assert reported == {rule.id for rule in source.RULES}
