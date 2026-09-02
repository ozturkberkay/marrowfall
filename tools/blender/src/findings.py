"""What a Blender script reports back, and the proof that it finished.

Every gate in the pipeline emits the same record. `check/mod.rs` builds them
in Rust, the scripts here write the same JSON, and the Rust side parses it
back, so one contract covers both.

Blender exits 0 when a script raises from a handler, a thread or `atexit`
(`docs/research/agent_reports/proof_python_exit_code_coverage.md`), so the exit
code cannot say whether a run finished. `guard` answers that instead: it writes
a success sentinel as the very last act, and the Rust side asserts the file
exists.

A script here never decides an exit code from its own findings. It measures,
writes its report through `write_report`, and finishes. The Rust runner reads
that report and decides.

One hole is left on purpose: an `atexit` handler registered before `guard`
runs also runs after `write_sentinel`, so a failure inside it is not seen.
Nothing here registers one.

Free of `bpy`, so it is unit tested with no Blender.
"""

import atexit
import enum
import json
import math
import os
import pathlib
import sys
import threading
import traceback
from collections.abc import Callable, Iterable
from types import TracebackType

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

SENTINEL_ENV = "MARROWFALL_SENTINEL"
"""Where to write the success sentinel. Set by `blender.rs`."""

CRASH_BLEND_ENV = "MARROWFALL_CRASH_BLEND"
"""Where to save the scene if the script raises. Set by `blender.rs`."""

REPORT_ENV = "MARROWFALL_REPORT"
"""Where to write this run's findings. Set by `blender.rs`."""


class Severity(enum.StrEnum):
    """Only an error can stop the pipeline."""

    ERROR = "error"
    WARNING = "warning"
    INFO = "info"
    """Measured, and inside its limit."""
    SKIPPED = "skipped"
    """Not measured: a spec field switched this rule off."""


class Comparison(enum.StrEnum):
    """How a measurement is read against its limit."""

    LE = "le"
    """At most."""
    LT = "lt"
    """Strictly less than."""
    EQ = "eq"
    """Exactly."""
    GE = "ge"
    """At least."""

    def holds(self, measured: float, limit: float) -> bool:
        match self:
            case Comparison.LE:
                return measured <= limit
            case Comparison.LT:
                return measured < limit
            case Comparison.EQ:
                return measured == limit
            case Comparison.GE:
                return measured >= limit


class Finding(BaseModel):
    """One measurement against one limit.

    `comparison` is required: without it, `mesh.cleanup_effective` passes a
    fixer that changed nothing, at 8 le 8. `measured_on` is required too,
    because precise numbers on the wrong representation is how 171 holes read
    as 13,368.
    """

    model_config = ConfigDict(frozen=True, extra="forbid")

    rule: str
    """Stable rule id, such as `clip.swing`."""
    severity: Severity
    subject: str
    """Bone, file, frame, object or direction."""
    measured: float
    limit: float
    comparison: Comparison
    unit: str
    attempt: int = Field(ge=1)
    """Which regeneration produced this."""
    measured_on: str
    """The representation and the space, named."""
    message: str

    @field_validator("rule", "subject", "unit", "measured_on", "message")
    @classmethod
    def must_say_something(cls, value: str) -> str:
        if not value.strip():
            raise ValueError("must not be empty")
        return value

    @field_validator("measured", "limit")
    @classmethod
    def must_be_a_real_number(cls, value: float) -> float:
        # A gate reports an undefined measurement as an error with a stated
        # message. NaN would ride through every comparison as "not worse".
        if not math.isfinite(value):
            raise ValueError(f"must be finite, got {value}")
        return value

    @property
    def holds(self) -> bool:
        """Whether the measurement is inside its limit."""
        return self.comparison.holds(self.measured, self.limit)


class Report(BaseModel):
    """Every Finding one stage attempt produced, for one item."""

    model_config = ConfigDict(frozen=True, extra="forbid")

    stage: str
    item: str
    """What the stage ran on: a character, a clip. One stage runs many times."""
    attempt: int = Field(ge=1)
    findings: tuple[Finding, ...] = ()

    @field_validator("stage", "item")
    @classmethod
    def must_say_something(cls, value: str) -> str:
        if not value.strip():
            raise ValueError("must not be empty")
        return value

    @model_validator(mode="after")
    def every_finding_belongs_to_this_attempt(self) -> "Report":
        for finding in self.findings:
            if finding.attempt != self.attempt:
                raise ValueError(
                    f"{finding.rule} is from attempt {finding.attempt}, "
                    f"this report is attempt {self.attempt}"
                )
        return self

    @property
    def has_errors(self) -> bool:
        return any(f.severity is Severity.ERROR for f in self.findings)

    def write(self, path: pathlib.Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(self.model_dump_json(indent=2) + "\n")


class Progress:
    """How far a guarded run got. Read by the sentinel writer, last of all."""

    def __init__(
        self, *, completed: bool = False, failures: list[str] | None = None
    ) -> None:
        self.completed = completed
        self.failures = failures if failures is not None else []


def write_sentinel(path: pathlib.Path, progress: Progress) -> None:
    """Record that the run finished. Its absence is the failure signal."""
    if not progress.completed or progress.failures:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"ok": True}))


def guard(
    body: Callable[[], None],
    save_blend: Callable[[pathlib.Path], None] | None = None,
) -> None:
    """Run a Blender script and report honestly whether it finished.

    Every path out of `body` other than a clean return leaves no sentinel, so
    the Rust side refuses the run.
    """
    progress = Progress()
    _record_every_reported_exception(progress)
    sentinel = _path_from_env(SENTINEL_ENV)
    if sentinel is not None:
        # Registered first, so it runs last: an exception from any other exit
        # handler is recorded before this decides.
        atexit.register(write_sentinel, sentinel, progress)

    try:
        body()
    except Exception:  # noqa: BLE001
        traceback.print_exc()
        blend = _path_from_env(CRASH_BLEND_ENV)
        if save_blend is not None and blend is not None:
            save_blend(blend)
        sys.exit(1)

    progress.completed = True
    if progress.failures:
        sys.exit("error: " + "; ".join(progress.failures))


def write_report(findings: Iterable[Finding]) -> Report:
    """Write this run's findings where the runner asked.

    The stage, the item and the attempt are read off that path, so a script
    cannot label its report as another run's.
    """
    path = report_path()
    stage, item, attempt = _header_of(path)
    report = Report(stage=stage, item=item, attempt=attempt, findings=tuple(findings))
    report.write(path)
    return report


def report_path() -> pathlib.Path:
    """Where this run writes its findings.

    The Rust runner owns every report name, so no script derives one. A run
    with a sentinel but no report is a wiring fault, not a hand run.
    """
    path = _path_from_env(REPORT_ENV)
    if path is not None:
        return path
    for name in (SENTINEL_ENV, CRASH_BLEND_ENV):
        if _path_from_env(name) is not None:
            raise RuntimeError(f"{name} is set but {REPORT_ENV} is not")
    raise RuntimeError(f"{REPORT_ENV} is unset, so run this through `cargo art`")


def _header_of(path: pathlib.Path) -> tuple[str, str, int]:
    """`bake.survivor.1.json` names the stage, the item and the attempt."""
    stage, _, rest = path.stem.partition(".")
    item, _, attempt = rest.partition(".")
    if not (stage and item and attempt.isdecimal()):
        raise RuntimeError(
            f"{path.name} is not <stage>.<item>.<attempt>.json, so this run "
            "has no header to report under"
        )
    return stage, item, int(attempt)


def _path_from_env(name: str) -> pathlib.Path | None:
    """A path the Rust runner set, or None when a human ran the script."""
    value = os.environ.get(name)
    return pathlib.Path(value) if value else None


def _record_every_reported_exception(progress: Progress) -> None:
    """Hook the three places Python reports an exception it did not raise.

    Blender keeps going after a handler, a thread or an exit handler raises,
    and its exit code stays 0, so these are the only witnesses.
    """
    previous_exception = sys.excepthook
    previous_unraisable = sys.unraisablehook
    previous_thread = threading.excepthook

    def on_exception(
        kind: type[BaseException],
        value: BaseException,
        trace: TracebackType | None,
    ) -> None:
        progress.failures.append(f"{kind.__name__}: {value}")
        previous_exception(kind, value, trace)

    def on_unraisable(unraisable: "sys.UnraisableHookArgs") -> None:
        progress.failures.append(
            f"{unraisable.exc_type.__name__}: {unraisable.exc_value}"
        )
        previous_unraisable(unraisable)

    def on_thread(args: threading.ExceptHookArgs) -> None:
        progress.failures.append(f"{args.exc_type.__name__}: {args.exc_value}")
        previous_thread(args)

    sys.excepthook = on_exception
    sys.unraisablehook = on_unraisable
    threading.excepthook = on_thread
