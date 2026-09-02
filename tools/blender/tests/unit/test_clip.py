"""The two rules the retarget reports on its own output.

Both are invariants the transfer holds by construction, so each one gets a
negative here rather than only in a hand-run Blender mutation: a rule with no
control that can fail in CI is a rule nobody has seen fail.
"""

import pytest
from clip import CONSTANT, LINEAR, Channel, Defects, defects

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
