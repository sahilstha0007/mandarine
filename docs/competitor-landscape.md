# Mandarine — competitor landscape & reverse-engineered feature ledger

Captured 2026-11-16. Goal: find the richest feature sets among "drag a file, pick an
action" converters and fold the best Linux-feasible ones into Mandarine. This extends
[`tangerine-teardown.md`](tangerine-teardown.md) (the original target) and
[`arch-parity-audit.md`](arch-parity-audit.md) (the 2026-10 gap ledger).

## Evidence & method

- Web survey via agent-reach / Exa (`web_search_exa`, `web_fetch_exa`) across converter
  marketing sites, comparison pages and App Store metadata.
- Source-level reverse engineering: cloned `kareem2099/DotMorph` (Apache-2.0),
  `jonasnlt/twistel` (GPL-3.0), `synthentica/kumquat` (MIT) into `/tmp/opencode/` and read
  their format catalogs, tool registries, CLI and operation code directly.
- REA static analysis of the **shipped DotMorph 2.0.3 wheel**
  (`dotmorph-2.0.3-py3-none-any.whl`, SHA-256
  `43b56a801bced34bd9dabe2e83c5f8077ab6baae4250b2c1e8879032b175e2cf`):
  `open_binary` → `inspect_artifact` (Evidence `ev_c8fa35849dfaf6c2fd3ee0a1aec257fa60132960593cb664ca0c0360697a5ebb`,
  summary `ev_28bf29f519a6a9a03854cbcadce7510af7ab6baae4250b2c1e8879032b175e2cf`).
  Inventory: 59 nodes / 60 files / 0 integrity contradictions — confirms the GitHub source
  is what ships. REA's CDP web path was unusable here (no Chrome/Chromium on this host;
  `tangerineformac.com/assets/*` now 404), so bundle-level RE of the current Tangerine site
  was not possible.
- Claims below are `observed` (in fetched source/site) or `inferred` (deduced from a
  competitor's own documentation). No paid Mac binary was obtained.

## The landscape

| Tool | Platform / licence | What it actually offers | Standout idea worth stealing |
|---|---|---|---|
| **Tangerine** | macOS 13+, paid, MAS | 275 conversion paths, 22 tools, 6 actions; Shift/Command/Control wheel | The gesture itself (already ported) |
| **DotMorph** | Linux, Apache-2.0 | **440 conversions**; two-ring GPU wheel + edge dock; Nautilus/Thunar/Dolphin/Nemo menus; global hotkey; watch folders; daemon; presets; EN/AR; dark/light/glass | `web/email/thumb/print/share` presets; `--preset`, `--quality`, `--dpi`, `--resize`, `--strip`, `--dry-run`, `--jobs`, `--overwrite`; Tools = compress / 50% / rotate / strip / mute / merge PDF |
| **twistel** | macOS 26, GPL-3 | 314 conversions + **~30 tools**: OCR, read QR, remove background, annotate, collage, social-media sizes, PDF sign/redact/stamp/compare/password/archive/extract-images, video trim/crop/speed/join/snapshots/split/redact/burn-subtitles/remove-audio, audio transcribe/normalize/channels/bleep, audio→video | Operational breadth; `QuickRun` for headless tools |
| **Kumquat** | macOS 13+, MIT | More formats than Swivel (JSON, XLSX, EPUB built in), watch folders, Intel support; corpus tests every candidate format with FFmpeg/qpdf/veraPDF; `Kumquat --benchmark` for wheel-open budgets | **Format-corpus verification** and a wheel-open performance budget |
| **Swivel** | macOS 13+, MIT | Tangerine's gesture, CLI `swivel`, Shortcuts "Convert File"/Compress/PDF, Finder Quick Action, welcome practice wheel | First-run practice wheel; OS-level automation hooks |
| **Morph** | Linux + Windows, GPL-3 | `Ctrl+Shift+Space` orb at pointer, right-click "Convert with Morph", actions ring (resize/crop/shrink/extract soundtrack/mute/normalize/PDF merge/split/rotate), `.converted.` output naming, GNOME shortcut registration | Shipped Linux AppImage/deb/rpm; desktop integration as a first-class feature |
| **Whirl** | macOS 26 | Converts *during* the drag, no modifier | Zero-gesture conversion |
| **TeraConvert** | macOS, commercial | **800+ conversions**, edit text inside PDFs, remove image backgrounds, batch folders, "spin the wheel" | In-place PDF text editing |
| **Dropmute** | macOS | Drop whole folders, group by type, "best for" tags per format, remembers last format per input type, favorites, recent history, estimated output size | Recommendation + memory UX around the wheel |
| **Permute** | macOS, paid | Drag-drop queue, drop zones, format presets (WebM/AV1/MP3/ProRes), bulk edit, stitch files, PDF pages→images | Preset library for media |
| **Dropzone** | macOS | Drag targets for move/share/upload, resize & compress actions | Shelf/target model (Tangerine "Float") |

## Reverse-engineered feature matrix (Linux-feasible subset)

| Capability | Tangerine | DotMorph | twistel | Morph | Mandarine before | Mandarine now |
|---|---|---|---|---|---|---|
| Format conversions | 275 | 440 | 314 | ~50 | Tangerine subset, engine-gated | unchanged |
| Strip metadata | tool | tool | tool | — | ✘ | **✔** |
| Rotate 90° | — | tool | tool | tool | ✘ | **✔** |
| Resize / 50% | tool | tool | tool | tool | ✘ | **✔ (50%)** |
| Mute video | — | tool | tool | tool | ✘ | **✔** |
| Normalize audio | tool | — | tool | tool | ✘ | **✔** |
| Merge / split PDF | tool | merge | tool | tool | ✔ (CLI + wheel) | unchanged |
| Compress (file size) | tool | tool | tool | tool | only archive packing | ✘ still |
| Watch folders | — | ✔ | — | — | ✘ | ✘ |
| Presets (web/email/…) | — | ✔ | — | ✔ (media) | ✘ | ✘ |
| Dry-run / quality / dpi | — | ✔ | — | ✘ | ✘ | ✘ |
| File-manager menus | Finder Quick Action | 4 file managers | Finder services | right-click | one Nautilus script example | unchanged |
| OCR / QR / background removal | image→DOCX (OCR) | — | ✔ | — | ✘ | ✘ |

## Implemented this session: the editing-tools page

The biggest single gap was the **tools** surface. Added a parameter-light slice that reuses
the existing argv-builder + capability-gating architecture (no new dependencies):

- `src/engines/tools.rs` — `EditingTool { Strip, Rotate90, Resize50, Mute, Normalize }`,
  format-category gating, and exact argv builders. Images go through `magick -strip /
  -rotate 90 / -resize 50%`; audio/video through `ffmpeg` stream-copy (`-map_metadata -1`
  for strip, `-map -0:a` for mute) or `loudnorm` for normalize. Outputs are collision-safe
  suffixed siblings (`photo-clean.png`, `clip-muted.mp4`).
- A third wheel page: **Alt** or **Tab** cycles convert → actions → tools, keeping tool
  sectors off the already-full conversion page (keys 1–9 stay valid on every page).
- `TargetKind::Tool` flows through the shared `convert::dispatch`, so both the wheel and
  `mandarine convert FILE --to strip|rotate90|resize50|mute|normalize` use the same engine.
- `mandarine list` now prints the available tools.
- 4 new unit tests (argv, gating, id round-trip); full suite: **77 passing**.
- Live checks: PNG strip (296 B → 83 B), MP4 mute (video-only stream) and MP4 metadata
  strip verified with ImageMagick/ffprobe.

`shortcut:` tools use fixed, battle-tested parameters; the upgrade path is per-tool dialogs
with live previews, as in Tangerine/twistel.

## Next wins, ranked (not done)

1. **Presets + convert flags** (`--preset web|email|thumb|print|share`, `--quality`,
   `--dpi`, `--resize`, `--dry-run`) — DotMorph's single best CLI idea; needs an options
   struct plumbed into the image argv builder. Medium.
2. **Watch folders** — DotMorph/Kumquat; a small resident watcher reusing `convert::run`.
   Medium, no new crate if polling is acceptable; `inotifywait` if installed.
3. **File-manager integration** — install Nautilus/Thunar/Dolphin/Nemo menus from
   `install.sh`, not just a README example. Small.
4. **Compress (file-size)** for images/video — distinct from archive packing. Small.
5. **Corpus verification** — a `docs/corpus.sh` that round-trips tricky real files and
   validates outputs with outside tools, borrowed from Kumquat. Medium, high trust value.
6. **OCR image→DOCX** (the one confirmed app-2.0.1-only Tangerine sector) — needs tesseract
   gating. Medium.
7. Remaining twistel tools (trim, join, speed, burn subtitles, QR, background removal) —
   each needs a real parameter dialog; do not fake with fixed flags.
