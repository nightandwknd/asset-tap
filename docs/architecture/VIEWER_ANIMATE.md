# 3D viewer + animation workbench

Read this before changing the 3D tab, the rig, or clips. It is the write
contract for the native `three-d` workbench as it shipped: what the viewer
is, what Bind and Bake are allowed to write, and how packs relate to the
embedded skeleton. The consumer loop is in the
[animation guide](../../site/content/docs/guides/animation.md).

## What it is

The 3D tab is an inspector. Animation is an optional panel the author
opens — even on a humanoid. The app does not classify chest vs character.

The viewer shows the asset. The panel overlays a rig, previews clips, and
bakes animation. It is not a model editor and not a full animation editor.
Displayed mesh vertices stay at rest except during clip playback. Rig is
overlay only: dragging a joint never deforms the mesh.

Bind follows Meshy / Mixamo: the arranged heads define the skeleton and
weights are computed from them, so moving a knee moves where the leg
bends. A head off the mesh is refused by name (`BindError::OffMesh`) — a
bone with no mesh near it gets ~0 weight and drives nothing. Auto-fit
only reseeds the overlay for review; it does not write.

We do not invent bones or paint weights. Fingers stay excluded
(`is_bind_bone`). Generic inspect and playback of an already-skinned GLB
must not require a clip pack.

**Out of scope:** arbitrary skeleton construction, invented bones, mesh
sculpting, weight painting, general keyframe authoring, a full animation
editor, and hosted editing.

- **Host:** egui + glow + `three-d` in one window (Linux X11 and Wayland
  included).
- **`three-d` + egui own presentation:** camera, lighting, inspection,
  playback mesh updates, skeleton overlay, markers, local undo, chrome.
- **Rust core owns asset operations:** fit, weights, retarget, validation,
  writes, metadata. One core serves CLI, MCP, and desktop.
- **Playback is CPU-evaluated** (`SkinnedClip`) and uploaded as positions.
  Do not stand up a second skinning runtime.

## Canonical skeleton and clip packs

**One canonical humanoid, 52 joints, VRM 1.0 names**
(`core/src/rig/canon.rs`). Every rig operation speaks these joints; a
pack's own bone names are a lookup at the edges (`BoneScheme`). The rest
pose is embedded as parent-local TRS, so **Rig and Bind work with nothing
downloaded** — a pack adds animations, never bones.

VRM rather than a pack's scheme: Quaternius renamed the Universal
Animation Library's rig from Blender Rigify (`DEF-hips`, `DEF-spine.001`)
to the Unreal convention (`pelvis`, `spine_01`) with the rest geometry
unchanged. Adopting a pack's names would pin us to a convention upstream
then abandoned. VRM is a published spec, maps 1:1 onto the 52 joints, and
gives every later pack a documented mapping. It also names the nodes a
`VRMC_vrm.humanoid` block would point at.

Legacy `DEF-*` support was removed, not kept as a fallback. Carrying it
meant carrying `three_sanitize` (dots in `DEF-spine.001`). Assets fitted
under that scheme are no longer recognized and must be re-fitted.

`root` is in the glTF skin because the chain needs a parent, but no clip
animates it and its bone segment runs through the pelvis. It is not a
bind bone. Weighting it anchors a large fraction of a standing figure to
a joint that never moves.

Packs live one directory deep under `packs_root()`:

```text
<packs_root>/ual1/pack.glb + pack.json
<packs_root>/ual2/pack.glb + pack.json
```

- `pack.json` caches clip names. `list_clips()` runs while the GUI
  builds its list; re-parsing multi-megabyte GLBs per frame is not an
  option. The GUI caches further in `App::clip_catalog`.
- Install picks the library from a download tree by **choosing the glTF
  with the most animations**, which skips the mannequin meshes that ship
  alongside it. `_RM` root-motion variants are ignored.
- Catalogs merge across installed packs; the first pack to claim a name
  wins, which matters only for `A_TPose`.
- Alias order is authoritative, not pack order: `run` must mean the same
  clip regardless of how a library sorts its animations.
- Clip **ids** are the pack's animation names, verbatim. Only the display
  name is cleaned up (CamelCase split, `Fwd` → `Forward`, `A_TPose` →
  `T-Pose`); the raw id stays on hover. `Loop` is kept: `Jump_Loop` sits
  beside `Jump_Start` and `Jump_Land`.
- The free Standard libraries live in repo `packs/` (trimmed GLBs) and
  ship as a hashed GitHub Release artifact (`clip-packs.zip` +
  `clip-packs-manifest.json`). They are **not** in the binary.
  `clip download` / Download animation packs installs missing ids only;
  `--force` refreshes packs stamped by a previous download and never
  replaces a `clip install` / Source pack.
- Paid Source tiers install the same way (`clip install --from`, or
  **Add pack...**). The id drops the tier suffix, so an upgrade replaces
  that pack rather than duplicating its clips.
  [UAL](https://quaternius.com/packs/universalanimationlibrary.html),
  [UAL 2](https://quaternius.com/packs/universalanimationlibrary2.html).

**Root motion is out of scope.** `_RM` variants translate the root and
walk the model out of the viewport. In-place loops are the default for a
generic exported asset; engines apply movement themselves.

## Workflow and write contract

```text
Inspect (default): Grid / Axes / Reset. No fit, clips, or bake.
  → author opens Animate (same viewer, optional panel)
  → unfitted: land in Rig (pose the skeleton on a frozen mesh)
  → Auto-fit (guess pose, stay in Rig) → Bind or Undo / Cancel
  → Bind (first time: generate weights)
  → click a clip to play (no write) → tick a set → Bake
  → already fitted: clip list; Rig refine keeps weights
  → author closes Animate: inspect again; no write
```

- **Preserve the asset.** Bind adds or updates the rig without rebuilding
  unrelated data. Bake changes animation only. Keep mesh parts,
  primitives, materials, transforms, attributes, and texture bytes.
  Reject unsupported input before a destructive write.
- **Order is Rig → Bind → clips.** Opening Animate does not auto-write.
  Unfitted lands in Rig with the shipped skeleton scaled to the mesh
  (`default_bind_markers`). Auto-fit reseeds the overlay and stays in
  Rig. Bind skins to the arranged heads and refuses any head off the
  mesh. The toast reports how many joints moved from the auto-fit.
  The gate is `has_bind_bones()` (prior Bind or CLI `--rig`).
  Already-rigged opens on the clip list.
- **Committed heads match arranged markers.** While dragging, markers are
  independent (children do not follow). Bind applies arranged worlds
  parent-first via `set_world_head`. The mesh on screen does not move.
- **Animate is an optional panel, not inspect chrome.** Rig and clip
  chrome apply only while it is open. A pack being installed, or a mesh
  already fitted, does not open it. Closing it stops playback and hides
  bones; it writes nothing.
- **Playback is transient.** It does not edit rest or write files. Bones
  is display-only. Selecting a clip never rewrites `model.glb`.
- **Bake means animation write.** Bind performs the skin write. Auto-fit
  does not.
- **Bake is declarative and multi-clip.** One model carries N animations.
  The file's `animations` become exactly the ticked set, each re-sourced
  from its pack. Bake is idempotent. Opening a previously baked model
  pre-checks those names. Unticking removes. `prune_unused` reclaims
  dropped accessors.
- **Two selections, not one.** Which clip is playing (one) is separate
  from which clips are exported (many). Click a row to play; tick it to
  include it in the write.
- **Bake is never a destroy button.** An empty set would strip animation,
  which is a different intent. Bake is disabled at zero. Removing
  everything is an explicit **Clear animation** that confirms first. It
  stays visible (disabled) when the mesh is fitted but has no clips, so
  the action is not mistaken for a missing control. The panel states the
  delta (`3 in model -> 2 after bake (-1)`) before the write.
- **Failure preserves a usable asset.** Stage beside the destination,
  validate, then rename. Metadata (`stamp_bind_step`) runs after the
  model write; a stamp failure leaves a valid GLB. Never silently apply
  pending edits to another asset.
- **CLI / MCP match the panel.** `--rig` and MCP `generate` `bind`
  mean fit + optional clips. `bind --fit-only` writes skinned rest.
  A rigged mesh keeps its skeleton and weights unless `--refit` is
  passed. There is no standalone MCP bind tool.

A foreign skin we cannot name is announced rather than silently replaced.
`is_fitted` means rigged _by us_. Bind still overwrites that skin (we
cannot animate a skeleton we cannot name), but Rig warns first.

## Controls

Shipped orbit camera: left-drag orbit, Shift-left or middle-drag pan,
wheel zoom, trackpad two-finger orbit with modifier zoom/pan. Rig
suspends orbit while dragging a joint and recovers on release, cancel,
and focus loss. Frame, Front, and Side belong with Rig only.

Joints are colored by body part (`HumanBone::group`) and labeled L/R
(`HumanBone::side`). The legend sits in the Animate column, not over the
viewport. All placeable bind joints stay draggable. Chrome and Bind
refusals use Mixamo nouns (clavicle, shoulder) so the collarbone is not
labeled "shoulder"; the GLB still stores VRM names.

```text
Inspect:    [Animate] | Grid / Axes / Reset
Rig:        Auto-fit | Undo Redo | Front Side Frame | Bind Cancel
Clips:      Rig | Bones | clip list (click play, tick export) | Bake
```

Drag sinks a joint to the limb midline over the mesh (`ray_midline`) and
follows the pointer off it. Bind is where being off the mesh has to
matter. Auto-fit retreats an off-mesh seed along the bone toward its
parent so a successful Auto-fit is a pose Bind will take.

## Ownership

| Layer         | Owns                                                              | Must not assume                     |
| ------------- | ----------------------------------------------------------------- | ----------------------------------- |
| Native viewer | Rendering, lighting, camera, inspect, playback, skeleton, markers | Every asset is humanoid or writable |
| egui chrome   | Auto-fit / Rig / Bind / Bake / clip catalog, pending-edit prompts | Viewer state authorizes file writes |
| Rust core     | Fit, weights, retarget, validated writes, bundle metadata         | A particular GUI renderer           |

Use model identity and request identity on Fit / Bind / Bake jobs.
Reject stale completions.

## FBX

`model.fbx` and the Blender shell-out are gone. The exporter wrote a
static mesh and never carried a skeleton, so a multi-clip GLB would have
been paired with a lifeless FBX. `--fbx`, `--no-fbx`, `--convert-fbx`,
and `--convert-only` are usage errors. A `model.fbx` already on disk
stays there. If FBX returns, it returns as a designed skeletal exporter
with tests.

Textures come out of the GLB (`core/src/textures.rs`). The extension
follows the actual bytes, not the declared MIME type. The rig path does
not read pixels: provider WebP (`EXT_texture_webp` with no core
`source`) loads without glTF document validation so a texture encoding
cannot refuse a bind.

## Known limits

- **Fingers** are not weighted. Hands will not curl with a grab or fist.
- **Root-motion** (`_RM`) clips are ignored.
- **Foreign animations** on an imported GLB we did not bake cannot be
  re-sourced; a declarative bake drops them.
- **`clip download` from `releases/latest`** needs a GitHub Release that
  attaches `clip-packs.zip`. Until then, debug builds read repo `packs/`,
  and tests use `ASSET_TAP_CLIP_PACKS_DIR`.
- **`--json` is not a clap global.** It must precede the subcommand
  (`asset-tap --json bind …`). Documented in the machine-interface spec.
- **Multi-mesh / multi-material** provider output has not been seen on a
  real asset. Preservation for that shape is covered synthetically
  (`rich_source` in `write.rs`).
- **Repeated bakes do not accumulate, but they are not byte-identical.**
  `serde_json` on `f32` is not a fixed point. Assert no-accumulation with
  slack, never byte equality.

## Core operations

- `fit_mesh`: landmarks, fit, weights, skinned rest; no animation.
- `fit_mesh_from_heads`: auto-fit for scale, apply arranged heads, then
  weight. Refuses off-mesh heads. `BindReport` says what the pose changed.
- `heads_off_mesh`: the same check without a write.
- `seed_bind_markers` / `default_bind_markers`: overlay only; no write.
- `apply_clip`: retarget onto fitted rest, without reweighting.
- `bind_mesh` / `--rig`: fit + apply, or fit-only with the explicit option.
- `clip_overlay_for_rest` / `SkinnedClip`: preview using the same
  preparation as Bake.
- `canon::armature`: embedded rest (`Rig` → `root` → 52 joints).
- `skeleton::canonicalize`: rename a loaded armature to VRM names.
- `pack::find_clip`: the single resolver. Preview and Bake both use it.
- `write::patch_animations_glb`: remap clip channels onto destination
  nodes **by name**.

## Invariants

- Markers are overlays on a frozen mesh. Never skin from a drag.
- Bind: heads define the skeleton, weights follow. Playback is last.
- The test for "does moving a joint matter" is an on-mesh nudge. A yank
  into space is not a test; under correct semantics that bone owns
  nothing.
- Sticks skip excluded bones (`is_bind_bone`) and join the next bind
  ancestor.
- Playback time is clip time, not mixer wall time.
- Never reintroduce a scheme-specific string test. Quaternius renamed
  its rig once already.
- Two writers emit the `bind` step (`clips` + `skeleton`);
  `both_bind_step_writers_agree_on_shape` guards the drift.

For CLI/MCP calls, [AGENTS.md](../../AGENTS.md) and the
[machine-interface contract](../CLI_MACHINE_INTERFACE.md) are the source
of truth. `bind --json`, `--json clip download`, and `clip list --json`
are implemented (`clip install` rejects `--json`).
