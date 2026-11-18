# Tangerine — client-side teardown

*(Site copy announces v1.4; the shipping App Store build is **2.0.1**. See
[Store-verified metadata](#store-verified-metadata-and-screenshot-evidence-2026-10-24).)*

Reverse-engineering notes for [tangerineformac.com](https://tangerineformac.com/), captured
2026-10-19. Purpose: derive the exact behaviour Mandarine mirrors on Arch/Hyprland.

**Method.** Static inspection of the public SvelteKit bundles and the rendered pages. No Mac
binary was obtained (App Store distribution is a paid `.app`), so everything below is either
*observed* (present in a fetched asset), *inferred* (derived from code), or *unknown*.

## Evidence

| Asset | SHA-256 |
|---|---|
| `https://tangerineformac.com/` (`tangerine.html`) | `ff0901fb723c59a916c1318cb6d931421fdee6b25cf66fa14c4672b3fda4ce62` |
| `/assets/home-BU_Sezi2.js` (`thome.js`) | `24a7b6d5d14087d1afb576161c013f8ce4559903ad3fc8ed5d6d3e098f86d3fd` |
| `/assets/tools-DHpyoIBf.js` (`ttools.js`) | `d2da237347cba2f74904b1288017528a76396150b63548a4003036c914f604e9` |
| `/tools` (`tools.html`) | `c0c5705d8e5b8b35dc07de38b8f41600591f8203250b82c393ecffd0ada864ba` |

Page metadata (observed):

- Site stack: SvelteKit (`modulepreload-polyfill`, hashed `/assets/*.js`), Cloudflare-fronted
  (`email-protection` obfuscation), `/sitemap.xml` lists `/`, `/tools`, `/press`.
- App facts from `/press`: **Tangerine 1.4**, by **Leap Studio**, Apple-silicon Mac,
  **macOS 13+**, **$29.99 one-time**, public launch **7 September 2026**, site
  `tangerineformac.com`.
- `/privacy`: "Every conversion, compression, and edit runs on your Mac… Tangerine never
  uploads your files, and it has no server to upload them to." No analytics, no tracking.
- `<meta name="description">`: "drag a file, then tap Shift to convert, Command for editing
  tools, or Control for file actions."
- The "93 MB" in the project note is **not** stated on the site, but the App Store listing
  reports 97,620,931 bytes = **93.1 MiB**, so the figure was right all along. Source: iTunes
  lookup, recorded below.

## Wheel behaviour (observed + inferred)

`home-BU_Sezi2.js`:

```js
const Ae={Shift:"shiftKey",Meta:"metaKey",Control:"ctrlKey",Alt:"altKey"};
function K(e){return e.size!==1?null:e.has("Shift")?"conversions":e.has("Meta")?"tools":e.has("Control")?"actions":null}
```

- The wheel opens **only for a single-file selection** (`e.size!==1` → nothing).
- While dragging, holding a modifier selects the page: **Shift → conversions**,
  **Command → tools**, **Control → actions**. Keys are remappable in Settings (`Ae` map).
- The wheel stays open until the file is dropped; releasing the key does not close it.
- Empty page copy: *"No editing tools for this format. Tap Shift to convert or Control for
  file actions."*
- Hover/target geometry (from minified constants, scale factors included):

```js
m = { inner: 48*.8*1.1 + 8*.8,   // ≈ 48.64
      outer: 166*.8*.9,          // ≈ 119.52
      backdrop: 166*.8*.9 + 8*.8,// ≈ 125.92
      gap: 5*.8,                 // 4.0
      corner: 28*.8*.9,          // ≈ 20.16
      labelInner: 50*.8 }        // 40.0
```

  Labels sit at `labelInner + (outer-labelInner)*0.56`. Sectors are rounded-corner wedges
  (quadratic corners, gap subtracted as an angular epsilon `asin(gap/2/r)`). Wheel is roughly
  **240 px across** on a 1× display.

- Conversion output naming (`Ge`): stem preserved, target extension swapped; collisions become
  `name 2.ext`, `name 3.ext`. Tool output naming (`tt`) prefixes a label: `photo Compressed.jpg`,
  `clip Trimmed.mp4`, `audio Bleeped.m4a`, `video Speed 2x.mp4`, folders for split results.

## Conversion matrix (observed, `z(ext)` in `home-BU_Sezi2.js`)

Sets from the bundle:

```js
qe = ["jpg","png","webp","heic","tiff","avif","bmp","pdf"]           // image targets
se = ["jpg","png","webp","heic","tiff","svg","avif","bmp"]           // image sources
q  = ["mp3","m4a","wav","flac","ogg","opus","aiff","wma"]            // audio
T  = ["mp4","mov","mkv","webm","avi","wmv"]                          // video containers
X  = ["srt","vtt"]                                                   // subtitles
re = ["docx","pages","md","rtf","rtfd","textClipping"]                // documents
H  = ["xlsx","xls","ods","csv","tsv"]                                // spreadsheets
Xe = ["docx","pages","pdf","txt","md","html","epub","png","jpg"]      // document targets
He = {"rar","ppt","xls","rtf","rtfd","textClipping"}                  // never an output
```

| Source | Offered targets (minus the source itself, minus `He`) |
|---|---|
| image (`se`) | `jpg png webp heic tiff avif bmp pdf` |
| audio (`q`) | `mp3 m4a wav flac ogg opus aiff wma` |
| video (`T`) | other containers + `gif` + `mp3` |
| `gif` | `mp4 mov mkv webm avi wmv` |
| `pdf` | `docx pages pptx xlsx jpg png epub txt md` |
| `ppt` / `pptx` | `pdf jpg png` |
| spreadsheet (`H`) | other spreadsheet formats + `pdf` |
| docs (`re`) | `docx pages pdf txt md html epub png jpg` |
| `epub` | `pdf docx pages txt md html` |
| `txt` | `docx pages pdf md html epub jpg png` |
| subtitle (`X`) | `srt vtt txt` |
| archive (`zip tar gz rar`) | `zip tar gz` |

### The 275-path claim, checked

Re-deriving the pair count from the bundle produces three different numbers, and none of them
is a rounding error:

| Figure | What it is |
|---|---|
| **267** | faithful reproduction of `z()`, including the bundle's own case bug: the document source set spells `textClipping` in camelCase while real extensions are lower-cased, so those 9 paths can never match |
| **276** | the same table with that one typo corrected |
| **275** | what `/press` advertises |

`matrix::count_paths()` reproduces **267** and its test asserts it. The advertised 275 therefore
looks like the app's own (case-insensitive) matching rather than the landing-page table — it is
marketing, and the table is the source of truth. The three figures are recorded here so nobody
re-derives them from scratch.

## File actions (Control)

Titles present in the bundle: `AIRDROP`, `FLOAT`, `COPY`, `COPY PATH`, `MOVE`, `TERMINAL`.
Press description: "AirDrop, Float, Copy, Move, Copy Path, and Terminal".

- **Float** — "Keep files in a floating shelf. Pin it to a screen edge and drag files into
  another app."
- **Copy** — "Copy files to the clipboard, ready to paste in Finder."
- **Terminal** — "Open a folder, or a file's containing folder, in Terminal or iTerm2."
- Actions also work with folders and mixed selections (press page).

## Editing tools (Command) — `/tools` page

22 tools in 5 groups (`tools-DHpyoIBf.js`), each opening its own window with a live preview:

| Group | Tools |
|---|---|
| Any file | Compress · Edit metadata |
| Images | Edit photos · Annotate photos · Add a background · Crop images · Redact photos · Create a collage |
| Video | Trim video · Crop video · Change video speed · Join videos · Save video frames · Split video · Redact video |
| Audio | Normalize volume · Create an audio visualizer · Trim audio · Convert audio channels · Bleep audio |
| PDF | Merge PDFs · Organize PDF pages |

Notable details: face/text detection for redaction runs locally; generated audio visualizer
exports MP4; video join uses the first clip as the output canvas; compressed outputs never touch
the original.

## What this means for Mandarine (Arch/Hyprland)

**Ported.** The conversion matrix is re-expressed in `src/matrix.rs` and gated on locally
probed engines, so the sectors Mandarine offers are the Linux-feasible subset of the table
above. File actions are implemented as a second wheel page (`src/actions.rs`): Copy (Wayland
clipboard), Copy Path, Terminal (kitty/wezterm/…), Compress, Extract. Pages follow Tangerine:
**Shift** = conversions, **Ctrl** = actions, with **Tab** as a keyboard alternative since Meta
belongs to the window manager on Hyprland. See [`guide.md`](guide.md) for the user manual.

**Deliberately not ported.**

- `pages`, `textClipping`, `rtfd` — Apple document formats with no Linux producer/consumer.
- `docx/pptx/xlsx` *from* PDF, `epub` — these are macOS Quick Look / iWork bridges; on Linux
  they need `pandoc` or Calibre. Mandarine advertises them only when `pandoc` or
  `ebook-convert` is on `PATH`.
- AirDrop, Float — no Linux equivalent (KDE Connect / clipboard shelf would be a rewrite).
- Editing tools with parameter dialogs (crop, trim, redact, collage, annotate) — these are
  22 interactive editors, not conversions; tracked in [`arch-parity-audit.md`](arch-parity-audit.md),
  not faked with fixed parameters.

**Parity limitations.** Tangerine wheels a single file only; Mandarine keeps multi-select for
the packing/merge cases, a deliberate divergence.

## Current public-site verification (2026-10-22)

The public pages were re-read through the web-reader route on 2026-10-22:

- `/` still advertises **275 conversion options**, **26 advanced file tools**, and **6 file actions**.
- `/tools` currently lists **22 named tools** in five groups: 2 any-file, 6 image, 7 video,
  5 audio, and 2 PDF tools. The site's **26 vs 22** tool count is therefore itself unresolved;
  Mandarine uses the named `/tools` list as the source of truth.
- `/press` repeats the 275-path marketing number and confirms Tangerine 1.4, Leap Studio,
  Apple silicon, macOS 13+, $29.99 one-time pricing, and the 7 September 2026 public launch.
- The pages continue to describe local-only processing, but they do not establish behavior of the
  paid Mac binary. The static bundle teardown remains the stronger evidence for wheel logic.

The Arch/Hyprland comparison is maintained separately in [`arch-parity-audit.md`](arch-parity-audit.md).

## Store-verified metadata and screenshot evidence (2026-10-24)

Captured with `curl` + the iTunes lookup API and the live App Store listing, because the earlier
pass only had the marketing site. Two of its open questions are now closed, one is newly open.

### No binary is obtainable — checked, not assumed

`/`, `/tools`, and `/press` contain no `.dmg`, `.pkg`, `.zip`, TestFlight or GitHub link. The only
install link on the site is
`https://apps.apple.com/us/app/tangerine-file-converter/id6813034065?mt=12`.

So there is nothing for Ghidra to open. This is a **distribution fact, not a missing tool**: the
paid MAS bundle exists only on a Mac that bought it. Native analysis (Ghidra, REA's
`native-macos` provider) stays blocked until an owned `.app` is copied off such a machine.

### App Store metadata

`GET https://itunes.apple.com/lookup?id=6813034065&country=us`

| Field | Value |
|---|---|
| `trackName` | Tangerine: File Converter |
| `version` | **2.0.1** |
| `bundleId` | `com.th.Tangerine` |
| `fileSizeBytes` | 97,620,931 = **93.1 MiB** |
| `minimumOsVersion` | 13.0 |
| `sellerName` | LEAP STUDIO |
| `releaseDate` | 2026-09-24 |
| `currentVersionReleaseDate` | 2026-10-17 |
| `kind` / `primaryGenreName` | `mac-software` / Utilities |
| `price` | `0.0` in the lookup response |

Corrections to the notes above:

- **v1.4 was never the app's version** — it is the site's press copy. The App Store build is 2.0.1,
  updated 2026-10-17, i.e. *after* this bundle capture on 2026-10-19. The site is the stale artifact.
- `price: 0.0` does not corroborate $29.99. The lookup API does not expose MAS pricing reliably, so
  the price remains a **site-only claim**; do not repeat it as store-verified.
- First release 2026-09-24 per the store vs 7 September 2026 per `/press`: unresolved (storefront
  or pre-order difference).
- The JS bundle is byte-identical to the 2026-10-19 capture
  (`home-BU_Sezi2.js` = `24a7b6d5…d3fd`), so all code-level findings above still hold for the demo.

### Screenshot evidence (5 × 800×500, from the live listing)

| Shot | SHA-256 | Shows |
|---|---|---|
| 1 | `11f7582271b0890b712e724fcb12aec645abe086c494e10d6e0e7c173816c042` | PNG conversion wheel |
| 2 | `a651705a646244b08851048179348c71558f2c2e74be0448d06c8170fc3df1e4` | tools wheel over Finder |
| 3 | `8796f858028c2ac10b13d8708c9bfc1044e4d2665748767e6943ca026a8abf07` | Edit Photo sheet |
| 4 | `e7853ac33697f0d58fa14f4028865c579f91056c2d70addaae588033d6eaa4eb` | Add BG sheet |
| 5 | `2e52d47951fee8a77be6bf4dc71b9893e9f4018525b9cdd353a804988106b2fe` | Trim / Convert channels / Bleep |

**The one finding that changes the matrix.** Shot 1 wheels a file named `tangerine tree.png` with
**eight** sectors: `JPG WEBP HEIC TIFF AVIF BMP PDF DOCX`. The demo's code cannot produce that —
`se.includes(e) ? t = qe`, and `qe = [jpg,png,webp,heic,tiff,avif,bmp,pdf]`, so `z("png")` returns
seven targets and `docx` appears nowhere in `qe`. The shipping app therefore offers an
**image → document** path the public demo does not implement. OCR-into-DOCX is the plausible
mechanism (macOS Vision), but that is an inference; the screenshot only proves the sector exists.

Consequence: the table above, and `src/matrix.rs::tests::image_matches_tangerine_table` which
asserts `["jpg","webp","heic","tiff","avif","bmp","pdf"]` for `png`, describe **the demo**, not app
2.0.1. If parity with the shipping app matters more than parity with the demo, image sources need a
`docx` target gated on an OCR engine.

Other observed details the bundle does not expose:

- The wheel's centre is a **file preview**: thumbnail, size pill (`2.2 MB`), and filename — not a
  text hint. Mandarine's centre currently reads `Drop to convert`.
- The **hovered sector fills solid with the accent colour**, and the pointer carries a small **`+`**
  badge while over it. Un-hovered sectors are pale glass with a 1 px rim.
- Conversion sectors are **uppercase text with no icons**; the tools page puts a glyph *above* each
  label (`COMPRESS`, `REDACT`, `CROP`, `ADD BG`, `ANNOTATE`, `METADATA`, `EDIT`).
- `Edit Photo` / `Add BG` / `Trim Audio` are rounded sheets with ✕ left, centred title, live preview,
  and sliders that show numeric values with per-row reset arrows.
- Shot 2's Finder footer reads `1 of 15 selected` — consistent with the single-file rule, **not**
  evidence of a multi-file wheel.

Still unknown: whether the image→DOCX sector is OCR-driven or a fixed template; the 26-vs-22 tool
count; and the wheel's true pixel diameter (marketing crops at unknown DPR).

### If an owned `.app` ever becomes available (Ghidra path)

```bash
# on the Mac that owns it
zip -r --symlinks /tmp/Tangerine.zip /Applications/Tangerine.app
# on Linux
unzip -q /tmp/Tangerine.zip -d /tmp/tangerine-app
file /tmp/tangerine-app/Tangerine.app/Contents/MacOS/Tangerine   # expect Mach-O 64-bit arm64
/home/sahil/Projects/Tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless \
  /tmp/ghidra-proj Tangerine \
  -import /tmp/tangerine-app/Tangerine.app/Contents/MacOS/Tangerine \
  -processor AARCH64:LE:64:v8.1 -analysisTimeoutPerFile 1800
```

Set expectations: a SwiftUI/AppKit app spends most of its code in `libswiftCore` and inlined
SwiftUI drawing, so Ghidra yields few readable functions. The cheap wins come first — `Info.plist`,
`nm -m` for ObjC method lists, the `Frameworks/` directory (Vision / VisionKit presence would settle
the OCR question above), localized `.lproj` strings, and `__TEXT,__const`.

### Tooling note

Bundle analysis was run with the REA CLI on the extracted asset tree
(`rea analyze-javascript-application /tmp/tangerine-web`), Evidence
`ev_2ac15521a8678407123f79beb1ef555f8d766975334c68221b2733a07921bf3b`. REA's `native-macos` provider
reports `unsupported_host` on Linux, so it cannot substitute for the Ghidra path above.
