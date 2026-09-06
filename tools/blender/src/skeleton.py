"""The skeleton file, as the retarget reads it.

`art/skeletons/<skeleton>.toml` says which bone fills each anatomical role in
every naming convention a clip can arrive in, which role hangs under which
while a clip is transferred, and where each role's bone must point. Matching
by role rather than by name is what stops this rig's `Spine`, its highest,
taking the motion of Mixamo's `Spine`, its lowest.

The aim table is the single source of every bone's constant offset, so a wrong
row produces a confidently wrong clip and nothing downstream can tell. It is
therefore validated on the way in: every role has a row, no row names a role
nothing maps, and a left row is the exact reflection of its right one. The
third check needs a rig and lives in Rust, as `rig.aim_table`, which reads
each aim against the rest pose the rig itself carries.

`[profile]` in the same file belongs to the Rust rig gates and is ignored
here.

Free of `bpy`, so it is unit tested with no Blender.
"""

import math
import tomllib
from collections.abc import Container, Iterable

from framing import Frozen, Vec3
from pydantic import model_validator

LEFT = "left_"
RIGHT = "right_"


def mirrored_role(role: str) -> str | None:
    """The same role on the other side, for a role that has one."""
    if role.startswith(LEFT):
        return RIGHT + role.removeprefix(LEFT)
    if role.startswith(RIGHT):
        return LEFT + role.removeprefix(RIGHT)
    return None


def bare_bone_name(name: str) -> str:
    """`mixamorig:LeftArm` -> `leftarm`: no namespace, no case.

    The form two rigs' bone names are compared in.
    """
    return name.rsplit(":", 1)[-1].lower()


def unfilled_roles(names: dict[str, str], bones: Iterable[str]) -> list[str]:
    """Roles this rig has no bone for, so a retarget cannot drive them."""
    available = {bare_bone_name(bone) for bone in bones}
    return sorted(
        role for role, bone in names.items() if bare_bone_name(bone) not in available
    )


OTHER_READERS = frozenset({"profile"})
"""Tables of a skeleton file that belong to another reader: `[profile]` to the
Rust rig gates. Named one by one, so a misspelled table is still refused
rather than dropped."""


class Skeleton(Frozen):
    """One skeleton file, minus the tables another reader owns."""

    canonical: str
    """The convention the canonical rig itself is named in."""
    conventions: dict[str, dict[str, str]]
    """Convention name to role to bone name."""
    retarget_chain: dict[str, str]
    """Role to the role above it, which is not the rig's own bone hierarchy:
    a source with three spine bones drives a target with four."""
    aim_table: dict[str, Vec3]
    """Role to the absolute world direction its bone must point in, in Blender
    Z-up space. These are the rows as written, so reading one here gives a
    vector that is not unit length. `aim` is the accessor that normalizes."""
    fingerprints: dict[str, tuple[str, ...]]
    """Convention to bones that only it has, so two identical role tables can
    still be told apart."""
    optional_roles: tuple[str, ...] = ()
    """Roles a convention other than the canonical one may leave out."""
    stride_segment: tuple[str, str]
    """The two roles whose joints root travel is sized by. A femur, because
    total height carries the head and the feet and neither takes a step."""

    @classmethod
    def parse(cls, text: str) -> "Skeleton":
        """Reads one skeleton file."""
        tables = tomllib.loads(text)
        return cls(**{k: v for k, v in tables.items() if k not in OTHER_READERS})

    @property
    def chain_top(self) -> str:
        """The one role every other role hangs under, which is the role that
        carries a clip's travel. The validator refuses a skeleton with any
        other number of tops, so this cannot be ambiguous."""
        return next(iter(self.roles - set(self.retarget_chain)))

    @property
    def roles(self) -> set[str]:
        """Every role this skeleton has, which the canonical convention
        defines because it is the one rig that must fill them all."""
        return set(self.conventions[self.canonical])

    @model_validator(mode="after")
    def the_canonical_convention_must_be_declared(self) -> "Skeleton":
        if self.canonical not in self.conventions:
            known = sorted(self.conventions)
            raise ValueError(
                f"canonical convention {self.canonical!r} is not in {known}"
            )
        return self

    @model_validator(mode="after")
    def every_convention_must_fill_every_required_role(self) -> "Skeleton":
        """A role no convention but ours knows about cannot be retargeted."""
        for name, convention in self.conventions.items():
            absent = self.roles - set(convention) - set(self.optional_roles)
            invented = set(convention) - self.roles
            if absent or invented:
                raise ValueError(
                    f"convention {name!r} disagrees about "
                    f"{sorted(absent | invented)}, and only "
                    f"{sorted(self.optional_roles)} may be left out"
                )
        return self

    @model_validator(mode="after")
    def the_stride_segment_must_be_two_of_this_skeletons_roles(self) -> "Skeleton":
        if unknown := sorted(set(self.stride_segment) - self.roles):
            raise ValueError(f"stride_segment names {unknown}, which are not roles")
        if self.stride_segment[0] == self.stride_segment[1]:
            raise ValueError(
                f"stride_segment is {list(self.stride_segment)}, which is one "
                f"joint twice and so has no length"
            )
        return self

    @model_validator(mode="after")
    def every_optional_role_must_be_a_role(self) -> "Skeleton":
        if unknown := sorted(set(self.optional_roles) - self.roles):
            raise ValueError(f"optional_roles names {unknown}, which are not roles")
        return self

    @model_validator(mode="after")
    def the_chain_must_reach_one_top_from_every_role(self) -> "Skeleton":
        """Every role but one hangs under another, and the walk has to end."""
        if unknown := sorted(
            (set(self.retarget_chain) | set(self.retarget_chain.values())) - self.roles
        ):
            raise ValueError(f"retarget_chain names {unknown}, which are not roles")
        if len(tops := sorted(self.roles - set(self.retarget_chain))) != 1:
            raise ValueError(
                f"retarget_chain leaves {tops} with no role above them, and a "
                f"chain has exactly one top"
            )
        for role in self.retarget_chain:
            at, steps = role, 0
            while (above := self.retarget_chain.get(at)) is not None:
                at, steps = above, steps + 1
                if steps > len(self.retarget_chain):
                    raise ValueError(f"the chain above {role!r} is a cycle")
        if tops[0] in self.optional_roles:
            raise ValueError(
                f"optional_roles names {tops[0]!r}, which is the top of the chain"
            )
        return self

    @model_validator(mode="after")
    def every_convention_needs_a_fingerprint_of_its_own(self) -> "Skeleton":
        """A fingerprint that two conventions share tells them apart from
        nothing."""
        if sorted(self.fingerprints) != sorted(self.conventions):
            raise ValueError(
                f"fingerprints covers {sorted(self.fingerprints)}, and every "
                f"convention in {sorted(self.conventions)} needs one"
            )
        seen: dict[str, str] = {}
        for name, bones in self.fingerprints.items():
            if not bones:
                raise ValueError(f"convention {name!r} has no fingerprint bone")
            for bone in bones:
                bare = bare_bone_name(bone)
                if (owner := seen.setdefault(bare, name)) != name:
                    raise ValueError(
                        f"{bone!r} fingerprints {name!r} and {owner!r}, so it "
                        f"tells them apart from nothing"
                    )
        return self

    @model_validator(mode="after")
    def every_role_needs_an_aim_and_every_pair_an_exact_mirror(self) -> "Skeleton":
        """A missing row is refused rather than filled from the source's own
        rest pose: a silent fallback is how the code this replaces left seven
        bones uncorrected."""
        if absent := sorted(self.roles - set(self.aim_table)):
            raise ValueError(
                f"aim_table has no row for {absent}, and every role needs one"
            )
        if invented := sorted(set(self.aim_table) - self.roles):
            raise ValueError(f"aim_table names {invented}, which no convention maps")
        for role, aim in self.aim_table.items():
            # `not any` is the all-zero row, which points nowhere.
            if not all(map(math.isfinite, aim)) or not any(aim):
                raise ValueError(f"aim_table.{role} is {aim}, which is no direction")
        # A sign error on one row is invisible until a clip comes out wrong,
        # so the two sides are held to an exact reflection, not a tolerance.
        for role, aim in self.aim_table.items():
            if not role.startswith(LEFT):
                continue
            other = RIGHT + role.removeprefix(LEFT)
            if (mirror := self.aim_table.get(other)) is None:
                raise ValueError(f"aim_table.{role} has no mirror row {other!r}")
            if aim != (-mirror[0], mirror[1], mirror[2]):
                raise ValueError(
                    f"aim_table.{role} is {aim} and {other} is {mirror}, which "
                    f"is not its reflection across X = 0"
                )
        return self

    def convention(self, name: str) -> dict[str, str]:
        """One convention's role to bone name map."""
        if (convention := self.conventions.get(name)) is None:
            known = sorted(self.conventions)
            raise ValueError(f"unknown bone naming convention {name!r}, known: {known}")
        return convention

    def aim(self, role: str) -> Vec3:
        """Where one role's bone must point, as a unit direction."""
        if (aim := self.aim_table.get(role)) is None:
            raise ValueError(f"no aim row for role {role!r}")
        length = math.sqrt(sum(part * part for part in aim))
        return (aim[0] / length, aim[1] / length, aim[2] / length)

    def chain_parent(self, role: str, filled: Container[str]) -> str | None:
        """The nearest role above `role` that `filled` holds, or None at the
        top.

        A source rig that leaves a role out is stepped over rather than
        treated as the parent, which is what lets a three bone spine drive a
        four bone one.
        """
        at = self.retarget_chain.get(role)
        # One step per row at most. The validator refuses a cycle, and this
        # does not trust that: a walk that never ends is no answer at all.
        for _ in range(len(self.retarget_chain)):
            if at is None or at in filled:
                return at
            at = self.retarget_chain.get(at)
        return None
