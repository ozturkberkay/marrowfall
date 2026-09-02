"""Unit tests for the Finding record and the success sentinel.

`findings` never imports `bpy`, so these run under plain pytest with no
Blender. What Blender itself does with the sentinel is proved in
`docs/research/agent_reports/proof_python_exit_code_coverage.md`.
"""

import json
import pathlib
import sys
import threading

import pytest
from findings import (
    CRASH_BLEND_ENV,
    REPORT_ENV,
    SENTINEL_ENV,
    Comparison,
    Finding,
    Progress,
    Report,
    Severity,
    guard,
    report_path,
    write_report,
    write_sentinel,
)
from pydantic import ValidationError


def a_finding(**overrides: object) -> Finding:
    fields: dict[str, object] = {
        "rule": "clip.swing",
        "severity": Severity.ERROR,
        "subject": "LeftHand",
        "measured": 52.9,
        "limit": 2.0,
        "comparison": Comparison.LE,
        "unit": "degrees",
        "attempt": 1,
        "measured_on": "world space, aligned by seconds from clip start",
        "message": "left hand swings 52.9 degrees from the source",
    }
    fields.update(overrides)
    return Finding(**fields)


# --- the record ---------------------------------------------------------


def test_a_finding_round_trips_through_the_json_the_rust_side_reads() -> None:
    finding = a_finding()
    parsed = Finding.model_validate_json(finding.model_dump_json())
    assert parsed == finding
    assert json.loads(finding.model_dump_json())["comparison"] == "le"


@pytest.mark.parametrize(
    "field",
    [
        "rule",
        "severity",
        "subject",
        "measured",
        "limit",
        "comparison",
        "unit",
        "attempt",
        "measured_on",
        "message",
    ],
)
def test_a_finding_missing_any_required_field_is_rejected(field: str) -> None:
    fields = json.loads(a_finding().model_dump_json())
    del fields[field]
    with pytest.raises(ValidationError):
        Finding(**fields)


def test_a_finding_with_an_unknown_field_is_rejected() -> None:
    with pytest.raises(ValidationError):
        a_finding(severity_override="warning")


@pytest.mark.parametrize("field", ["rule", "subject", "unit", "measured_on", "message"])
def test_a_finding_with_an_empty_text_field_is_rejected(field: str) -> None:
    with pytest.raises(ValidationError):
        a_finding(**{field: "   "})


@pytest.mark.parametrize("value", [float("nan"), float("inf"), float("-inf")])
@pytest.mark.parametrize("field", ["measured", "limit"])
def test_a_gate_can_never_emit_nan(field: str, value: float) -> None:
    with pytest.raises(ValidationError):
        a_finding(**{field: value})


def test_the_first_attempt_is_one() -> None:
    with pytest.raises(ValidationError):
        a_finding(attempt=0)


def test_a_finding_is_immutable() -> None:
    with pytest.raises(ValidationError):
        a_finding().limit = 999.0  # ty: ignore[invalid-assignment]


@pytest.mark.parametrize(
    ("comparison", "measured", "holds"),
    [
        (Comparison.LE, 8.0, True),
        (Comparison.LE, 8.1, False),
        (Comparison.LT, 8.0, False),
        (Comparison.LT, 7.9, True),
        (Comparison.EQ, 8.0, True),
        (Comparison.EQ, 7.9, False),
        (Comparison.GE, 8.0, True),
        (Comparison.GE, 7.9, False),
    ],
)
def test_the_comparison_decides_whether_a_measurement_holds(
    comparison: Comparison, measured: float, holds: bool
) -> None:
    finding = a_finding(comparison=comparison, measured=measured, limit=8.0)
    assert finding.holds is holds


def test_a_fixer_that_changed_nothing_fails_the_strict_comparison() -> None:
    """8 le 8 passes and 8 lt 8 does not. That gap is why the field exists."""
    assert a_finding(comparison=Comparison.LE, measured=8.0, limit=8.0).holds
    assert not a_finding(comparison=Comparison.LT, measured=8.0, limit=8.0).holds


# --- the report ---------------------------------------------------------


@pytest.mark.parametrize(("stage", "item"), [("", "survivor"), ("bake", "  ")])
def test_a_report_with_no_stage_or_no_item_is_rejected(stage: str, item: str) -> None:
    with pytest.raises(ValidationError):
        Report(stage=stage, item=item, attempt=1)


def test_the_first_report_attempt_is_one() -> None:
    with pytest.raises(ValidationError):
        Report(stage="bake", item="survivor", attempt=0)


def test_a_report_has_errors_only_when_an_error_is_present() -> None:
    quiet = Report(
        stage="bake",
        item="survivor",
        attempt=1,
        findings=(
            a_finding(severity=Severity.WARNING),
            a_finding(severity=Severity.INFO),
        ),
    )
    assert not quiet.has_errors

    loud = Report(stage="bake", item="survivor", attempt=1, findings=(a_finding(),))
    assert loud.has_errors


def test_an_empty_report_has_no_errors() -> None:
    assert not Report(stage="bake", item="survivor", attempt=1).has_errors


def test_a_report_refuses_a_finding_from_another_attempt() -> None:
    with pytest.raises(ValidationError, match="attempt"):
        Report(
            stage="concept",
            item="survivor",
            attempt=2,
            findings=(a_finding(attempt=1),),
        )


def test_a_report_writes_the_json_the_rust_side_parses(
    tmp_path: pathlib.Path,
) -> None:
    report = Report(
        stage="concept", item="survivor", attempt=2, findings=(a_finding(attempt=2),)
    )
    path = tmp_path / "nested" / "concept.survivor.2.json"
    report.write(path)
    assert json.loads(path.read_text())["findings"][0]["comparison"] == "le"


# --- the sentinel -------------------------------------------------------


def test_the_sentinel_is_written_only_after_a_clean_run(
    tmp_path: pathlib.Path,
) -> None:
    path = tmp_path / "bake.survivor.1.sentinel.json"
    progress = Progress()
    progress.completed = True
    write_sentinel(path, progress)
    assert json.loads(path.read_text()) == {"ok": True}


@pytest.mark.parametrize(
    "progress",
    [Progress(), Progress(completed=True, failures=["RuntimeError: boom"])],
)
def test_an_unfinished_or_failed_run_leaves_no_sentinel(
    tmp_path: pathlib.Path, progress: Progress
) -> None:
    path = tmp_path / "bake.survivor.1.sentinel.json"
    write_sentinel(path, progress)
    assert not path.exists()


def test_the_header_comes_off_the_path_the_runner_set(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> None:
    path = tmp_path / "download.strafe_left.2.json"
    monkeypatch.setenv(REPORT_ENV, str(path))

    report = write_report([a_finding(attempt=2)])

    assert (report.stage, report.item, report.attempt) == ("download", "strafe_left", 2)
    assert json.loads(path.read_text())["item"] == "strafe_left"


def test_a_script_cannot_label_its_report_as_another_run(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> None:
    """There is no argument for the header, so there is nothing to get wrong."""
    monkeypatch.setenv(REPORT_ENV, str(tmp_path / "bake.survivor.1.json"))

    report = write_report([])

    assert report.stage == "bake"
    assert report.item == "survivor"


@pytest.mark.parametrize(
    "name", ["bake.json", "bake.survivor.json", "bake.survivor.x.json", ".1.json"]
)
def test_a_report_path_that_carries_no_header_is_refused(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path, name: str
) -> None:
    monkeypatch.setenv(REPORT_ENV, str(tmp_path / name))

    with pytest.raises(RuntimeError, match="no header"):
        write_report([])


def test_the_runner_owns_the_report_path(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> None:
    monkeypatch.setenv(REPORT_ENV, str(tmp_path / "bake.survivor.1.json"))
    assert report_path() == tmp_path / "bake.survivor.1.json"


def test_a_run_with_a_sentinel_but_no_report_path_is_a_wiring_fault(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> None:
    monkeypatch.delenv(REPORT_ENV, raising=False)
    monkeypatch.setenv(SENTINEL_ENV, str(tmp_path / "s.json"))

    with pytest.raises(RuntimeError, match=REPORT_ENV):
        report_path()


def test_a_hand_run_script_is_told_to_use_cargo_art(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for name in (REPORT_ENV, SENTINEL_ENV, CRASH_BLEND_ENV):
        monkeypatch.delenv(name, raising=False)

    with pytest.raises(RuntimeError, match="cargo art"):
        report_path()


class Recorder:
    """Stands in for `atexit`, which pytest cannot run mid-test.

    It also stands in for the three exception hooks, whose real versions
    belong to pytest and must not see these deliberate failures.
    """

    def __init__(self) -> None:
        self.registered: list[tuple[object, ...]] = []
        self.reported: list[object] = []

    def register(self, function: object, *args: object) -> None:
        self.registered.append((function, *args))

    def run(self) -> None:
        for function, *args in reversed(self.registered):
            function(*args)  # ty: ignore[call-non-callable]

    def report(self, *args: object) -> None:
        self.reported.append(args)


@pytest.fixture
def guarded(
    monkeypatch: pytest.MonkeyPatch, tmp_path: pathlib.Path
) -> tuple[Recorder, pathlib.Path, pathlib.Path]:
    """A `guard` whose hooks and exit handlers cannot leak into other tests."""
    sentinel = tmp_path / "bake.survivor.1.sentinel.json"
    blend = tmp_path / "bake.survivor.1.blend"
    monkeypatch.setenv(SENTINEL_ENV, str(sentinel))
    monkeypatch.setenv(CRASH_BLEND_ENV, str(blend))
    monkeypatch.setenv(REPORT_ENV, str(tmp_path / "bake.survivor.1.json"))
    recorder = Recorder()
    monkeypatch.setattr(sys, "excepthook", recorder.report)
    monkeypatch.setattr(sys, "unraisablehook", recorder.report)
    monkeypatch.setattr(threading, "excepthook", recorder.report)
    monkeypatch.setattr("findings.atexit", recorder)
    return recorder, sentinel, blend


def test_a_clean_script_leaves_a_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    recorder, sentinel, _ = guarded
    guard(lambda: None)
    recorder.run()
    assert sentinel.exists()


def test_a_script_raising_at_its_top_level_leaves_no_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    recorder, sentinel, blend = guarded
    saved: list[pathlib.Path] = []

    def boom() -> None:
        raise RuntimeError("boom")

    with pytest.raises(SystemExit) as exit_info:
        guard(boom, save_blend=saved.append)
    recorder.run()

    assert exit_info.value.code == 1
    assert not sentinel.exists()
    assert saved == [blend], "the .blend is the diagnostic for a failed run"


def test_a_script_calling_sys_exit_leaves_no_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    """`sys.exit` is a usage error, and SystemExit skips `except Exception`."""
    recorder, sentinel, _ = guarded
    with pytest.raises(SystemExit):
        guard(lambda: sys.exit("error: pass --character GLB"))
    recorder.run()
    assert not sentinel.exists()


def test_a_script_raising_from_a_blender_handler_leaves_no_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    """Blender swallows this one and exits 0, so only the sentinel sees it."""
    recorder, sentinel, _ = guarded

    def body() -> None:
        # What Blender does with a handler that raises: report it, carry on.
        sys.excepthook(RuntimeError, RuntimeError("boom"), None)

    with pytest.raises(SystemExit) as exit_info:
        guard(body)
    recorder.run()

    assert "boom" in str(exit_info.value.code)
    assert not sentinel.exists()
    assert recorder.reported, "the report must still reach the previous hook"


def test_a_script_raising_on_a_thread_leaves_no_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    recorder, sentinel, _ = guarded

    def boom() -> None:
        raise RuntimeError("boom")

    def body() -> None:
        thread = threading.Thread(target=boom)
        thread.start()
        thread.join()

    with pytest.raises(SystemExit):
        guard(body)
    recorder.run()
    assert not sentinel.exists()


def test_a_script_raising_during_shutdown_leaves_no_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    """An `atexit` or `unregister` failure surfaces as an unraisable."""
    recorder, sentinel, _ = guarded

    class Unraisable:
        exc_type = RuntimeError
        exc_value = RuntimeError("boom")

    guard(lambda: None)
    sys.unraisablehook(Unraisable())  # ty: ignore[invalid-argument-type]
    recorder.run()
    assert not sentinel.exists()


def test_a_failure_in_a_later_exit_handler_still_removes_the_sentinel(
    guarded: tuple[Recorder, pathlib.Path, pathlib.Path],
) -> None:
    """`write_sentinel` is registered first, so it runs last.

    Registered last instead, it would decide before the handler that fails,
    and Blender exits 0 on an `atexit` failure. That ordering is the whole
    sentinel design.
    """
    recorder, sentinel, _ = guarded

    def failing_exit_handler() -> None:
        class Unraisable:
            exc_type = RuntimeError
            exc_value = RuntimeError("unregister blew up")

        sys.unraisablehook(Unraisable())  # ty: ignore[invalid-argument-type]

    def body() -> None:
        # What a Blender addon registers on the way out.
        recorder.register(failing_exit_handler)

    guard(body)
    recorder.run()

    assert not sentinel.exists()
    assert recorder.registered[0][0] is write_sentinel, (
        "the sentinel writer must be registered first, so LIFO runs it last"
    )


def test_a_hand_run_script_needs_no_sentinel(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Only the Rust runner sets the paths, and only it asserts them."""
    monkeypatch.delenv(SENTINEL_ENV, raising=False)
    monkeypatch.delenv(CRASH_BLEND_ENV, raising=False)
    recorder = Recorder()
    monkeypatch.setattr(sys, "excepthook", recorder.report)
    monkeypatch.setattr(sys, "unraisablehook", recorder.report)
    monkeypatch.setattr(threading, "excepthook", recorder.report)
    monkeypatch.setattr("findings.atexit", recorder)
    saved: list[pathlib.Path] = []

    def boom() -> None:
        raise RuntimeError("boom")

    with pytest.raises(SystemExit):
        guard(boom, save_blend=saved.append)
    recorder.run()
    assert recorder.registered == []
    assert saved == []
