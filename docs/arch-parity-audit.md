# Mandarine — Arch parity audit

Captured 2026-10-22 against the public [Tangerine for Mac site](https://tangerineformac.com/)
and the current `/home/sahil/Projects/mandarine` source tree. This is a gap ledger, not a claim
that Mandarine already implements Tangerine's whole product.

## Evidence boundary

- The site was read through the agent-reach web route using the Jina Reader mirror.
- The public pages expose marketing copy, supported formats, named tools, and simulated wheel
  behavior. They do **not** expose the paid Mac `.app` binary.
- The existing static bundle teardown in `docs/tangerine-teardown.md` remains the source for the
  inferred `z(ext)` matrix and modifier behavior.
- Observations below separate public claims from what the Rust source actually implements.

## What the current site says

| Surface | Public-site claim | Evidence |
|---|---|---|
| Conversions | 275 options | `/`, `/press` |
| Advanced tools | 26 | `/` marketing counter |
| Named tools | 22 tools in five groups | `/tools` page |
| File actions | AirDrop, Float, Copy, Move, Copy Path, Terminal | `/`, `/press` |
| Processing | Local-only, no uploads | `/`, `/privacy`, `/press` |
| Wheel | Drag, then Shift/Command/Control; wheel remains open until drop | `/`, `/press` |

The site's 26-tool counter and `/tools` list disagree. The existing bundle arithmetic also has
the 267/276/275 discrepancy documented in `tangerine-teardown.md`; neither number should be used
as an implementation-complete claim without naming the counting rule.

## Current Mandarine coverage

### Already covered for Arch/Hyprland

- Linux-feasible conversion targets are capability-gated by installed ImageMagick, FFmpeg,
  Poppler, LibreOffice, archive tools, Pandoc, and Calibre helpers.
- Local processing and collision-safe sibling outputs are implemented.
- GTK4 drag/drop accepts both `GdkFileList` and `text/uri-list`.
- The wheel is an **OVERLAY layer surface** (`namespace = "mandarine"`), anchored top-left and
offset by margins — never a window, so no float/decorate/ontop rules and no `hyprctl` placement.
  *(Superseded 2026-10-24: the earlier note here claimed the wheel had to be a toplevel because a
  layer surface would not receive drags. Hyprland 0.56's `mouseMoveUnified` skips its
  keep-focus-while-pressed shortcut when `dndActive()`, so layer surfaces are valid drop targets;
  the borderless toplevel survives only as the fallback for compositors without
  `wlr-layer-shell`.)*
- The Arch interaction model uses Shift for conversions, Ctrl for actions, and Tab as the
  keyboard page switch because Meta is owned by Hyprland.
- The headless CLI and a file-manager `wheel --files` path provide an explicit Arch fallback when
  mid-drag behavior is unavailable or a compositor does not retarget the drag.

### Not equivalent yet

| Tangerine surface | Mandarine status | Why it matters |
|---|---|---|
| 22 parameterized editing tools | **5 tool actions** (2026-11-16) | Strip Meta, Rotate 90, Resize 50, Mute and Normalize were added as a third wheel page with fixed flags; they are *not* the 22 preview-driven editors. See [`competitor-landscape.md`](competitor-landscape.md). |
| AirDrop | Not ported | Apple transport; no direct Linux equivalent is claimed. |
| Float shelf | Not ported | Requires a separate shelf/window workflow. |
| Move | Not implemented | The source comment mentions a folder chooser, but `Action` has no `Move` variant or GUI path. |
| Single-file wheel | Deliberate divergence | Mandarine keeps multi-select for packing and PDF/archive workflows. |
| ~240 px wheel | Deliberate visual divergence | Mandarine's configured 440 px disc plus transparent shadow surface is larger. |
| Command editing page | Alt/Tab tools page (partial) | Command/Meta is a window-manager key on Arch; the tools page uses Alt or Tab and holds 5 fixed-parameter tools, not the 22 editors. |

Mandarine's five Arch actions — Copy, Copy Path, Terminal, Compress, Extract — are useful Linux
substitutes, but they must not be presented as the site's six actions.

## Concrete correctness/documentation gaps

These are smaller than the missing tools, but they are real mismatches in the current source tree:

1. `clipboard_fallback_secs = 0` is documented as disabling the fallback, while
   `src/ui/drop.rs::clipboard_fallback` still spawns a thread and performs an immediate read.
2. `custom_targets.steps` is documented as shell-free argv, but `src/convert.rs::run_custom`
   applies `split_whitespace()` to each configured string. A path containing spaces is split and
   the flat example in the docs is not a safe representation of a multi-command pipeline.
3. `README.md` and `docs/guide.md` had stale 67/66 test counts; the current tree contains 69
   inline `#[test]` attributes.
4. Wheel surface type has now flipped twice in these notes (layer-shell → toplevel → layer-shell).
   The current truth is the source: `gtk4_layer_shell` OVERLAY with namespace `mandarine`, plus a
   toplevel fallback when the protocol is missing. Anything older than 2026-10-24 is stale.
5. The installer reports only a subset of optional helpers. `mandarine list` is the authoritative
   capability check; the installer should eventually report Poppler split/merge/text helpers,
   archive extractors, Pandoc, and Calibre individually.

## Arch-first implementation order

### P0 — contracts before feature breadth

- Make `clipboard_fallback_secs = 0` a true opt-out and add a focused regression test.
- Replace the custom-target flat-string parser with an explicit argv schema that preserves spaces
  and still refuses shell evaluation.
- Add a `mandarine doctor`/installer dependency report that names each helper and the targets it
  unlocks.
- Keep the public count language honest: Linux-feasible subset, not 275 options.

### P1 — small, useful Arch tools

Implement these as real dialogs or explicit parameter prompts, not hidden fixed defaults:

- strip metadata;
- rotate/resize or crop one image;
- mute or change video speed;
- images to PDF with page order;
- merge/organize PDF pages.

These should share the existing local engine and collision-safe output layer, but they are not
Tangerine parity until the user can preview and choose parameters.

### P2 — Linux-native actions and packaging

- Add Move with a GTK folder chooser and a clear overwrite policy.
- Decide whether a Float substitute is worth a real shelf UX, rather than mapping it to a generic
  window.
- Add a `PKGBUILD`/AUR path and a user-level desktop or file-manager integration entry.
- Keep the Hyprland path first-class, but make the headless CLI usable on other Arch desktops.

### P3 — full tool surface

Only after P0/P1 are stable: build the remaining preview-based image, video, audio, and PDF
editors. Track each tool independently; do not collapse them into a single “tools page complete”
checkbox.

## Verification status

The repository contains prior notes claiming 69 tests and live Hyprland verification. A fresh
verification could not be rerun in this container because `cargo` is not available on its PATH.
No new code is claimed as tested by this audit.
