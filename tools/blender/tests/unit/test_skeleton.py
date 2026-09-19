"""Unit tests for the skeleton file: the role map, the retargeting chain, the
fingerprints, and the aim table.

`skeleton` never imports `bpy`, so these run under plain pytest with no
Blender.

The aim table is the single source of every bone's constant offset in the
retarget, so most of this file is refusals: a missing row, a row nothing maps,
a broken mirror pair, and a row that is no direction at all.
"""

import math
import pathlib
import tomllib

import pytest
from pydantic import ValidationError
from skeleton import (
    SKULL_TOP,
    Skeleton,
    bare_bone_name,
    mirrored_role,
    unfilled_roles,
)

# `art/skeletons/humanoid.toml`, cut to the roles that make a point: the three
# that disagree between the two conventions, plus one mirrored pair.
SKELETON_TOML = """
canonical = "meshy"
optional_roles = []
stride_segment = ["spine_lower", "spine_upper"]
ground_roles = ["hips", "left_arm"]

[conventions.meshy]
hips = "Hips"
spine_lower = "Spine02"
spine_upper = "Spine"
neck = "neck"
left_arm = "LeftArm"
right_arm = "RightArm"

[conventions.mixamo]
hips = "Hips"
spine_lower = "Spine"
spine_upper = "Spine2"
neck = "Neck"
left_arm = "LeftArm"
right_arm = "RightArm"

[landmarks.meshy]
skull_top = "head_end"

[landmarks.mixamo]
skull_top = "HeadTop_End"

[retarget_chain]
spine_lower = "hips"
spine_upper = "spine_lower"
neck = "spine_upper"
left_arm = "spine_upper"
right_arm = "spine_upper"

[fingerprints]
meshy = ["headfront"]
mixamo = ["HeadTop_End"]

[aim_table]
hips = [0.0, 0.0, 1.0]
spine_lower = [0.0, 0.0, 1.0]
spine_upper = [0.0, 0.0, 1.0]
neck = [0.0, 0.0, 1.0]
left_arm = [1.0, 0.0, -1.0]
right_arm = [-1.0, 0.0, -1.0]
"""

COMMITTED = pathlib.Path(__file__).parents[4] / "art/skeletons/humanoid.toml"


def a_skeleton() -> Skeleton:
    return Skeleton.parse(SKELETON_TOML)


def edited(*replacements: tuple[str, str]) -> Skeleton:
    """The fixture with lines rewritten, which is how each refusal is tested
    one at a time."""
    text = SKELETON_TOML
    for old, new in replacements:
        assert old in text, f"{old!r} is not in the fixture"
        text = text.replace(old, new)
    return Skeleton.parse(text)


# --- Foreign bone names ---------------------------------------------------


@pytest.mark.parametrize(
    ("source", "expected"),
    [
        ("mixamorig:LeftArm", "leftarm"),
        ("Neck", "neck"),
        ("neck", "neck"),
        ("Spine02", "spine02"),
    ],
)
def test_a_bone_name_loses_its_namespace_and_its_case(
    source: str, expected: str
) -> None:
    assert bare_bone_name(source) == expected


def test_a_role_has_at_most_one_mirror() -> None:
    assert mirrored_role("left_arm") == "right_arm"
    assert mirrored_role("right_toe") == "left_toe"
    assert mirrored_role("hips") is None


# --- The role map ---------------------------------------------------------


def test_a_role_map_reads_a_convention_per_provider() -> None:
    roles = a_skeleton()
    assert roles.canonical == "meshy"
    assert roles.convention("mixamo")["spine_lower"] == "Spine"


def test_a_skeleton_file_ignores_the_table_another_reader_owns() -> None:
    """`[profile]` belongs to the Rust rig gates and lives in the same file."""
    roles = Skeleton.parse(SKELETON_TOML + '\n[profile]\nbones = ["Hips"]\n')

    assert sorted(roles.conventions) == ["meshy", "mixamo"]


def test_a_skeleton_file_with_no_conventions_at_all_is_refused() -> None:
    with pytest.raises(ValidationError, match="conventions"):
        Skeleton.parse('canonical = "meshy"\n')


def test_a_misspelled_table_is_refused_rather_than_dropped() -> None:
    """Only the table another reader owns is ignored, so a typo still fails
    instead of silently changing nothing."""
    with pytest.raises(ValidationError, match="conventionz"):
        edited(("[conventions.meshy]", "[conventionz.meshy]"))


def test_a_canonical_convention_that_is_not_declared_is_refused() -> None:
    with pytest.raises(ValidationError, match="canonical convention 'maya'"):
        edited(('canonical = "meshy"', 'canonical = "maya"'))


def test_a_convention_that_leaves_a_role_out_is_refused_by_name() -> None:
    with pytest.raises(ValidationError, match="mixamo.*neck"):
        edited(('neck = "Neck"\n', ""))


def test_a_convention_that_invents_a_role_is_refused_by_name() -> None:
    with pytest.raises(ValidationError, match="mixamo.*tail"):
        edited(('neck = "Neck"', 'neck = "Neck"\ntail = "Tail"'))


def test_an_unknown_convention_names_the_ones_that_exist() -> None:
    with pytest.raises(ValueError, match="unknown.*'meshy', 'mixamo'"):
        a_skeleton().convention("maya")


MIXAMO_BONES = [
    "mixamorig:Hips",
    "mixamorig:Spine",
    "mixamorig:Spine2",
    "Neck",
    "mixamorig:LeftArm",
    "mixamorig:RightArm",
]


def test_a_source_that_fills_every_role_leaves_none_unfilled() -> None:
    roles = a_skeleton()
    assert unfilled_roles(roles.convention("mixamo"), MIXAMO_BONES) == []


def test_a_source_missing_a_bone_names_the_role_it_leaves_undriven() -> None:
    """What a clip labeled with the wrong convention looks like."""
    roles = a_skeleton()
    assert unfilled_roles(roles.convention("meshy"), MIXAMO_BONES) == ["spine_lower"]


# --- Optional roles -------------------------------------------------------


def test_a_source_convention_may_leave_an_optional_role_out() -> None:
    """A source with no shoulders or no toes still drives everything else."""
    roles = edited(
        ("optional_roles = []", 'optional_roles = ["neck"]'),
        ('neck = "Neck"\n', ""),
    )

    assert "neck" not in roles.convention("mixamo")
    assert "neck" in roles.roles, "the canonical convention still defines it"


def test_an_optional_role_that_is_not_a_role_is_refused() -> None:
    with pytest.raises(ValidationError, match="optional_roles names \\['tail'\\]"):
        edited(("optional_roles = []", 'optional_roles = ["tail"]'))


def test_the_top_of_the_chain_cannot_be_optional() -> None:
    """Leave the hips out and the chain has nothing to hang from."""
    with pytest.raises(ValidationError, match="top of the chain"):
        edited(("optional_roles = []", 'optional_roles = ["hips"]'))


# --- The stride segment ---------------------------------------------------


def test_the_top_of_the_chain_is_the_role_that_carries_travel() -> None:
    """One role hangs under nothing, and the validator refuses any other
    number, so `source.traveling` cannot be reading the wrong bone."""
    assert Skeleton.parse(COMMITTED.read_text()).chain_top == "hips"


def test_the_stride_segment_is_the_pair_root_travel_is_sized_by() -> None:
    assert a_skeleton().stride_segment == ("spine_lower", "spine_upper")


def test_a_stride_segment_naming_something_that_is_not_a_role_is_refused() -> None:
    with pytest.raises(ValidationError, match="stride_segment names"):
        edited(
            (
                'stride_segment = ["spine_lower", "spine_upper"]',
                'stride_segment = ["left_arm", "tail"]',
            )
        )


def test_a_stride_segment_of_one_joint_twice_is_refused() -> None:
    """It would measure a length of zero, and `translation_scale` would then
    refuse the clip with a message about the wrong rig."""
    with pytest.raises(ValidationError, match="one joint twice"):
        edited(
            (
                'stride_segment = ["spine_lower", "spine_upper"]',
                'stride_segment = ["left_arm", "left_arm"]',
            )
        )


def test_the_ground_roles_are_the_joints_the_floor_snap_reads() -> None:
    assert a_skeleton().ground_roles == ("hips", "left_arm")


def test_a_ground_role_that_is_not_a_role_is_refused() -> None:
    with pytest.raises(ValidationError, match="ground_roles names"):
        edited(
            (
                'ground_roles = ["hips", "left_arm"]',
                'ground_roles = ["hips", "flipper"]',
            )
        )


def test_the_same_ground_role_twice_is_refused() -> None:
    """The lowest of one joint and itself is that joint, so the second row
    would add nothing and hide a missing foot."""
    with pytest.raises(ValidationError, match="names hips twice"):
        edited(
            (
                'ground_roles = ["hips", "left_arm"]',
                'ground_roles = ["hips", "hips"]',
            )
        )


def test_a_skeleton_with_no_ground_role_is_refused() -> None:
    with pytest.raises(ValidationError, match="no ground role"):
        edited(('ground_roles = ["hips", "left_arm"]', "ground_roles = []"))


# --- The retargeting chain ------------------------------------------------


def test_the_chain_has_one_top_and_it_is_the_hips() -> None:
    roles = a_skeleton()

    assert set(roles.roles) - set(roles.retarget_chain) == {"hips"}
    assert roles.retarget_chain["neck"] == "spine_upper"


def test_a_chain_row_naming_something_that_is_not_a_role_is_refused() -> None:
    for broken in ('tail = "hips"', 'neck = "tail"'):
        with pytest.raises(ValidationError, match="not roles"):
            edited(('neck = "spine_upper"', broken))


def test_a_chain_with_two_tops_or_none_is_refused() -> None:
    with pytest.raises(ValidationError, match="no role above them"):
        edited(('spine_lower = "hips"\n', ""))
    with pytest.raises(ValidationError, match="no role above them"):
        edited(('spine_lower = "hips"', 'spine_lower = "hips"\nhips = "neck"'))


def test_a_cycle_in_the_chain_is_refused() -> None:
    with pytest.raises(ValidationError, match="cycle"):
        edited(
            ('spine_lower = "hips"', 'spine_lower = "spine_upper"'),
            ('neck = "spine_upper"', 'neck = "hips"'),
        )


# --- The skip-unmapped-ancestor walk --------------------------------------


def test_the_walk_gives_the_role_above_when_the_source_fills_it() -> None:
    roles = a_skeleton()

    assert roles.chain_parent("neck", roles.roles) == "spine_upper"
    assert roles.chain_parent("spine_lower", roles.roles) == "hips"


def test_the_walk_steps_over_a_role_the_source_leaves_out() -> None:
    """A three bone spine driving a four bone one, which is what the chain is
    for."""
    roles = a_skeleton()

    assert roles.chain_parent("neck", {"hips", "spine_lower"}) == "spine_lower"
    assert roles.chain_parent("neck", {"hips"}) == "hips"
    assert roles.chain_parent("left_arm", {"hips"}) == "hips"


def test_the_walk_ends_at_the_top() -> None:
    roles = a_skeleton()

    assert roles.chain_parent("hips", roles.roles) is None
    assert roles.chain_parent("neck", set()) is None, "nothing above is filled"


def test_the_walk_ends_even_when_the_chain_holds_a_cycle() -> None:
    """The validator refuses a cycle, and the walk does not trust that: the
    dict is still a dict, and a walk that never ends is no answer at all."""
    roles = a_skeleton()
    roles.retarget_chain["hips"] = "neck"

    assert roles.chain_parent("neck", {"nothing"}) is None


# --- Fingerprints ---------------------------------------------------------


def test_every_convention_carries_a_fingerprint_of_its_own() -> None:
    roles = a_skeleton()

    assert roles.fingerprints == {"meshy": ("headfront",), "mixamo": ("HeadTop_End",)}


def test_a_convention_with_no_fingerprint_is_refused() -> None:
    with pytest.raises(ValidationError, match="every convention"):
        edited(('mixamo = ["HeadTop_End"]\n', ""))
    with pytest.raises(ValidationError, match="no fingerprint bone"):
        edited(('mixamo = ["HeadTop_End"]', "mixamo = []"))


def test_a_fingerprint_two_conventions_share_is_refused() -> None:
    """It tells them apart from nothing. Compared with the namespace and the
    case stripped, so a Mixamo namespace cannot smuggle a duplicate through."""
    with pytest.raises(ValidationError, match="apart from nothing"):
        edited(('mixamo = ["HeadTop_End"]', 'mixamo = ["mixamorig:headfront"]'))


def test_a_fingerprint_for_a_convention_that_is_not_declared_is_refused() -> None:
    with pytest.raises(ValidationError, match="every convention"):
        edited(('mixamo = ["HeadTop_End"]', 'maya = ["HeadTop_End"]'))


# --- The aim table --------------------------------------------------------


def test_an_aim_is_read_as_a_unit_direction() -> None:
    """A row is a direction and not a unit vector, so `[1, 0, -1]` is the arm
    45 degrees below horizontal."""
    roles = a_skeleton()

    assert roles.aim("hips") == (0.0, 0.0, 1.0)
    part = math.sqrt(0.5)
    assert roles.aim("left_arm") == pytest.approx((part, 0.0, -part))
    assert roles.aim("right_arm") == pytest.approx((-part, 0.0, -part))


def test_a_role_with_no_aim_row_is_refused() -> None:
    """Refused rather than filled from the source's own rest pose: a silent
    fallback is how the code this replaces left seven bones uncorrected."""
    with pytest.raises(ValidationError, match="no row for \\['neck'\\]"):
        edited(("neck = [0.0, 0.0, 1.0]\n", ""))


def test_an_aim_row_no_convention_maps_is_refused() -> None:
    with pytest.raises(ValidationError, match="aim_table names \\['tail'\\]"):
        edited(
            ("neck = [0.0, 0.0, 1.0]", "neck = [0.0, 0.0, 1.0]\ntail = [0.0, 1.0, 0.0]")
        )


def test_asking_for_a_role_the_table_does_not_aim_says_so() -> None:
    with pytest.raises(ValueError, match="no aim row for role 'tail'"):
        a_skeleton().aim("tail")


@pytest.mark.parametrize(
    "broken", ["[0.0, 0.0, 0.0]", "[nan, 0.0, 1.0]", "[inf, 0.0, 1.0]"]
)
def test_a_row_that_is_no_direction_at_all_is_refused(broken: str) -> None:
    with pytest.raises(ValidationError, match="no direction"):
        edited(("hips = [0.0, 0.0, 1.0]", f"hips = {broken}"))


@pytest.mark.parametrize("broken", ["[0.0, 1.0]", "[0.0, 0.0, 1.0, 0.0]", '"up"'])
def test_a_row_that_is_not_three_numbers_is_refused(broken: str) -> None:
    with pytest.raises(ValidationError, match="aim_table"):
        edited(("hips = [0.0, 0.0, 1.0]", f"hips = {broken}"))


def test_a_mirror_pair_that_is_not_a_reflection_is_refused() -> None:
    # The X part not negated, which is the sign error the check exists for.
    with pytest.raises(ValidationError, match="reflection across X = 0"):
        edited(("right_arm = [-1.0, 0.0, -1.0]", "right_arm = [1.0, 0.0, -1.0]"))
    # And a Y part that differs, which a mirror must not change.
    with pytest.raises(ValidationError, match="reflection across X = 0"):
        edited(("right_arm = [-1.0, 0.0, -1.0]", "right_arm = [-1.0, 1.0, -1.0]"))


def test_a_sided_role_with_no_mirror_row_is_refused() -> None:
    """A convention with no right side leaves the left row nothing to
    reflect, which is a different fault from a row that is simply missing."""
    with pytest.raises(ValidationError, match="no mirror row 'right_arm'"):
        edited(
            ('right_arm = "RightArm"\n', ""),
            ('right_arm = "spine_upper"\n', ""),
            ("right_arm = [-1.0, 0.0, -1.0]\n", ""),
        )


# --- Landmarks ------------------------------------------------------------


def test_a_landmark_is_read_per_convention_the_way_a_role_is() -> None:
    roles = a_skeleton()

    assert roles.landmarks_of("meshy")[SKULL_TOP] == "head_end"
    assert roles.landmarks_of("mixamo")[SKULL_TOP] == "HeadTop_End"


def test_an_unknown_convention_has_no_landmarks() -> None:
    with pytest.raises(ValueError, match="unknown.*'meshy', 'mixamo'"):
        a_skeleton().landmarks_of("maya")


def test_a_convention_with_no_landmark_row_is_refused() -> None:
    with pytest.raises(ValidationError, match="every convention in"):
        edited(('[landmarks.mixamo]\nskull_top = "HeadTop_End"\n', ""))


def test_a_convention_that_names_no_skull_top_is_refused() -> None:
    """A landmark this table leaves out is a reading that silently goes
    missing on one rig and not on another."""
    with pytest.raises(ValidationError, match="landmarks.mixamo names no"):
        edited(('skull_top = "HeadTop_End"', 'chin = "Chin"'))


# --- The committed file ---------------------------------------------------


def test_the_committed_skeleton_file_loads() -> None:
    roles = Skeleton.parse(COMMITTED.read_text())

    assert roles.canonical == "standard"
    assert len(roles.roles) == 22, "22 roles, no fingers"
    assert len(roles.aim_table) == 22, "one aim per role, torso included"
    assert set(roles.roles) - set(roles.retarget_chain) == {"hips"}
    assert roles.stride_segment == ("left_upper_leg", "left_leg")
    assert roles.ground_roles == ("left_toe", "right_toe")
    assert roles.optional_roles == (), "all three conventions fill every role"
    # Meshy numbers its spine from the top, so its lowest spine bone is not
    # the one our own `Spine` names.
    assert roles.convention("meshy")["spine_lower"] == "Spine02"
    assert roles.convention("standard")["spine_lower"] == "Spine"


def test_every_committed_convention_names_the_top_of_its_skull() -> None:
    roles = Skeleton.parse(COMMITTED.read_text())

    assert {name: marks[SKULL_TOP] for name, marks in roles.landmarks.items()} == {
        "meshy": "head_end",
        "standard": "head_end",
        "mixamo": "HeadTop_End",
    }


def test_the_committed_aim_table_aims_the_torso_up_and_the_toes_forward() -> None:
    """Blender Z-up, the character facing -Y, so +X is his left."""
    roles = Skeleton.parse(COMMITTED.read_text())

    for role in ("hips", "spine_lower", "spine_middle", "spine_upper", "neck", "head"):
        assert roles.aim(role) == (0.0, 0.0, 1.0), role
    assert roles.aim("left_shoulder") == (1.0, 0.0, 0.0)
    assert roles.aim("right_shoulder") == (-1.0, 0.0, 0.0)
    assert roles.aim("left_upper_leg") == (0.0, 0.0, -1.0)
    assert roles.aim("left_toe") == (0.0, -1.0, 0.0)


def test_every_committed_mirror_pair_is_an_exact_reflection() -> None:
    roles = Skeleton.parse(COMMITTED.read_text())
    pairs = 0

    for role, aim in roles.aim_table.items():
        if (other := mirrored_role(role)) is None or not role.startswith("left_"):
            continue
        mirror = roles.aim_table[other]
        assert aim == (-mirror[0], mirror[1], mirror[2]), role
        pairs += 1
    assert pairs == 8, "eight sided roles a side"


def test_the_committed_chain_hangs_both_legs_and_the_spine_off_the_hips() -> None:
    roles = Skeleton.parse(COMMITTED.read_text())

    assert roles.retarget_chain["spine_lower"] == "hips"
    assert roles.retarget_chain["left_upper_leg"] == "hips"
    assert roles.retarget_chain["left_shoulder"] == "spine_upper"
    # The whole point of the chain: a source with no shoulder still drives
    # the arm, from the spine.
    assert roles.chain_parent("left_arm", roles.roles) == "left_shoulder"
    assert (
        roles.chain_parent("left_arm", roles.roles - {"left_shoulder"}) == "spine_upper"
    )


def test_the_committed_profile_is_left_to_its_own_reader() -> None:
    """The Rust rig gates own `[profile]`, and this reader must not care what
    it holds."""
    tables = tomllib.loads(COMMITTED.read_text())

    assert "profile" in tables, "the file does carry one"
    assert not hasattr(Skeleton.parse(COMMITTED.read_text()), "profile")
