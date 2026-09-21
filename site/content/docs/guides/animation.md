+++
title = "Animation (experimental)"
description = "Experimental humanoid rig and clip bake: GUI Animate panel, CLI bind / --rig, MCP clips."
date = 2026-09-13
weight = 6
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["guide"]
+++

{% <devlab.callout type="warning" title="Experimental"> %}

Humanoid rigging and clip bake are new. The skeleton, weights, and bake
contract are still being proven on real provider meshes. The panel, flags,
and clip names may change. Fingers are not weighted. Root-motion clips
are ignored.

{% </devlab.callout> %}

Asset Tap can rig a humanoid mesh to an embedded 52-joint skeleton and bake
animation clips into `model.glb`. The skeleton ships in the binary, so
rigging needs nothing downloaded. Packs only add clips.

One model carries N animations, the way Mixamo and Meshy work, rather than
one export per clip. Bake is declarative: the file ends up with exactly the
clips you named or ticked. Unticking (or omitting) a clip removes it.

## Why T-pose

Use the [humanoid](@/docs/guides/cli-usage.md#templates) template when you
want a figure the auto-fit can read. It asks for the pose first and the
character second, because the pose is what the rig is measured against.

The shipped skeleton's rest pose is a true T-pose, so a rigged mesh and
the clip library agree and clips apply at zero offset. Auto-fit also
reads T-pose proportions — a T-pose is about as wide as it is tall — to
place the skeleton. An A-posed mesh narrows that span enough to throw the
fit off entirely, not just soften it.

**Prompting for it is not a guarantee.** Lead with the pose, keep the arms
clear of the torso and the legs apart, and avoid coats, capes and long
hair that close those gaps. If a character comes out A-posed, regenerate
it rather than rigging it.

## Desktop app

Generate or open a character, switch to the 3D tab, and open **Animate**.
The panel is optional; the viewer stays an inspector until you do.

1. **Rig:** pose the shipped skeleton on a frozen mesh. Dragging a joint
   never deforms the model. Markers are colored by body part and labeled
   L or R; a legend sits beside the viewport, not over it. Rig always opens
   with joints you can drag.
2. **Auto-fit** is a button, not something that runs on open. It fits the
   skeleton to a readable humanoid and leaves the joints alone on a mesh it
   cannot read as a body. Review the result before you bind.
3. **Bind:** skins the mesh to the posed heads. Every joint must sit on
   the body; a joint off the mesh is refused by name. The confirmation says
   how many joints you moved from the auto-fit.
4. **Clips:** click a row to play it (preview never writes). Tick the set
   you want and press **Bake**. The panel shows what will be added and
   removed. An empty tick-set is a separate clear, not an overloaded Bake.

Animation packs add rows to the clip list. See [Clip packs](#clip-packs).
Switching clips does not rewrite `model.glb` until Bake.

## Command line

```bash
# Generate a character and rig it in one run
asset-tap --rig -y -t humanoid "a knight in plate armor"

# Rig an existing mesh and bake a walk
asset-tap bind --mesh model.glb --clip walk

# Several clips on one model
asset-tap bind --mesh model.glb --clip walk --clip Sword_Attack --clip Idle_Loop

# Skinned T-pose, no animation
asset-tap bind --mesh model.glb --fit-only
```

`--clip` repeats on both `--rig` and `bind`. Re-running `bind` on an
already-rigged mesh keeps its skeleton and weights, so adding a clip cannot
undo joints you arranged by hand. Pass `--refit` to discard the pose.

```bash
asset-tap clip install --from Universal-Animation-Library.zip
asset-tap clip install --from Universal-Animation-Library-2.zip
asset-tap clip list
asset-tap clip list --model output/2026-09-08_120000/model.glb
```

A missing clip or pack is a local error (exit 7), not a retryable failure.
`walk` is an alias for `Walk_Loop`; `clip list` shows the names the pack
actually contains.

Machine-readable: `asset-tap --json bind --mesh model.glb --clip walk`.
The result names the written model and the full baked clip set. See the
[CLI machine interface](https://github.com/nightandwknd/asset-tap/blob/main/docs/CLI_MACHINE_INTERFACE.md).

## Clip packs

Clips come from [Quaternius](https://quaternius.com)' CC0 libraries:

- [Universal Animation Library](https://quaternius.com/packs/universalanimationlibrary.html)
  (`ual1`)
- [Universal Animation Library 2](https://quaternius.com/packs/universalanimationlibrary2.html)
  (`ual2`)

The rest pose we ship is derived from the first library. Both packs can sit
side by side; catalogs merge.

**Out of the box:** **Download Universal Animation Libraries** in Help or Animate →
**Add pack...** (or `asset-tap clip download` / `--json clip download` / MCP
`clip_download`) pulls the Standard libraries from the latest Asset Tap
release. The archive is hash-verified and is **not** inside the app.
Already-installed packs are left alone, so a Source upgrade you installed
yourself is never overwritten. `clip download --force` refreshes only packs
stamped by a previous download.

Each Quaternius page also has a free **Standard** download (the site button
or Itch) and the remaining paid **Source** clips. Those zips install the
same way: drop the `.zip` / folder / `.glb` on the **Animation panel**,
**Add pack...**, File → **Install Animation Pack...**, or
`clip install --from`. The overlay says "Drop to install animation pack"
on that panel so a UAL archive is not imported as a library bundle.
Dropping a pack on the preview toasts to use the Animation panel. The pack id drops the tier suffix, so installing Source
over Standard replaces that library rather than duplicating its clips.

## MCP

`generate` takes `bind: true` and `clips: ["walk", "Sword_Attack"]`. The
same rules as the CLI: no pack required to rig, a missing clip is a local
error. Full tool list: [MCP server](@/docs/guides/mcp.md).

## What this does not do

- **Fingers** stay excluded. Hands will not curl with a grab or fist clip.
- **Root-motion** (`_RM`) variants are ignored so preview does not walk the
  model out of the viewport. In-place loops only.
- **Props** can be rigged by hand. Auto-fit will not invent a pose on a
  chest or crate; use the [prop](@/docs/guides/cli-usage.md#templates)
  template when you are not animating.
- **No weight painting, no invented bones, no full animation editor.**
  Bind computes weights from the posed heads.

A `bind` step is recorded on `bundle.json` when you Bind and again when you
bake. Re-binding a model that already carries baked clips keeps them: each
one is re-sourced from its pack onto the new rig. A clip whose pack is no
longer installed cannot be re-sourced; the panel says which ones were
dropped, and baking again restores them once the pack is back. See
[Bundle structure](@/docs/guides/bundle-structure.md#the-bind-step-experimental).
