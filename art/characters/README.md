# Characters

One directory per character, holding everything that is hand-authored about
it and every file too expensive to reproduce.

```text
art/characters/survivor/
├── spec.ron      # hand-authored: what the character is
├── spec.lock     # machine-owned: what has been built, and which paid task made it
├── concept/      # four AI views, committed because they cannot be regenerated
└── model.glb     # the rigged, skinned character the bake reads
```

Everything else a run produces is derived and gitignored, under
`art/staging/<name>/`:

| File | What it is |
| --- | --- |
| `bare.glb` | the mesh as the `model` stage downloaded it, before rigging. Every `mesh.*` file gate reads this one |
| `clean.glb` | what the fixer wrote from it, and what the rigging call is sent. Read against the same gates, less the two a cleaned file cannot answer |

The split matters: `model.glb` is already skinned, and editing geometry under
a skin desyncs the weights, so the cleanup runs on `bare.glb` and rigging
happens again afterwards.

## The two flags that edit geometry

```ron
subject: Subject(
    kind: Humanoid,
    cleanup: true,   # weld, drop debris and fill holes before rigging
    symmetry: true,  # mirror the mesh, and hold the mirror rules to it
),
```

Both are false unless a spec says otherwise, and `cargo art new` turns them on
for a humanoid only. They are independent. `cleanup` decides whether the
Blender fixer runs at all. `symmetry` decides whether that fixer mirrors the
mesh, and whether `mesh.mirror`, `rig.mirror_length` and
`rig.mirror_direction` measure or report `skipped` on the declaration: a
monster can be asymmetric on purpose, and a one-armed thing with a tail must
not fail a rule written for a human.

Symmetrize keeps one half and mirrors it, **including the UVs**, so an
asymmetric texture detail is duplicated and flipped. The model contact sheet
is where that is reviewed.

`cargo art check` measures whatever of these is on disk and writes one report
per rule set under `art/staging/reports/`: `mesh`, `cleaned`, and `cleanup`
for the pair. `crates/xtask-art/README.md` has the rest of the pipeline.
