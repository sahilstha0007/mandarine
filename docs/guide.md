# Mandarine — user guide

Everything Mandarine can do, from first install to writing your own wheel sectors.
Commands assume an Arch/Hyprland setup; the GUI's core integration is GTK4, `wl-clipboard` and
`wlr-layer-shell` (the dial is an OVERLAY layer surface, so the compositor places it and no
window rules are needed).

---

## 1. Install

```sh
sudo pacman -S --needed base-devel rust pkgconf gtk4 wl-clipboard

# engines — install what you'll actually use
sudo pacman -S --needed ffmpeg imagemagick poppler libreoffice-fresh zip 7zip unrar

cd ~/Projects/mandarine
./install.sh                 # builds release, installs to ~/.local/bin/mandarine
mandarine list               # shows exactly what this machine can do
```

Then bind the wheel and give it something to refract (ML4W keeps the stock `custom.lua`
immutable):

```lua
-- ~/.config/ml4w/hypr/custom.lua
hl.bind("SUPER + Y", hl.dsp.exec_cmd("mandarine"))
```

`hyprctl reload`, press **SUPER+Y**. That is the whole setup: the dial is a layer surface, so it
needs no float/decorate/ontop rules and never appears in your window list.

The dial is an **OVERLAY layer surface** (`namespace = "mandarine"`) anchored top-left and offset
by margins, so it floats above every window without being one. It takes the keyboard while it is
up — that is how arrows, Tab, Enter and Esc drive it. One optional layer rule gives the glass
something to refract:

```lua
hl.layer_rule({
    name = "mandarine-glass",
    match = { namespace = "mandarine" },
    blur = true,
    ignore_alpha = 0.25,
})
```

On a compositor without `wlr-layer-shell` the dial falls back to a borderless toplevel that
Mandarine parks under the cursor with `hyprctl`, so the old window rules still have a purpose
there.

> Optional engines are never installed for you. A missing engine is not an error — the wheel
> simply stops offering its sectors. `mandarine list` is the source of truth.

---

## 2. The wheel

Press **SUPER+Y** anywhere — including **while you are already dragging a file**: keep the button
down, tap the key, release over a sector. A glass dial fades in under the cursor (220 ms) and the
ring breathes at 12 o'clock while it waits.

Wayland detail worth knowing: a surface that appears *during* a drag is not handed that drag
until the pointer moves, so the wheel warps the pointer by one pixel and back on open. That hop
is what makes "drag, tap, release" work instead of silently doing nothing. A background app
cannot observe that a drag has started on Wayland, which is why the trigger is a key and not
Tangerine's hold-Shift (macOS tells an app when a drag begins; Wayland does not).

If you would rather not drag at all, the dial opens pre-loaded:

```sh
mandarine wheel --files ~/Pictures/a.png ~/Downloads/b.pdf
```

Drop that in `~/.local/share/nautilus/scripts/Mandarine` (or a Dolphin service menu) and the file
manager's right-click menu gets a "Mandarine" entry that works on the current selection.

1. **Drop file(s)** on the dial — from Nautilus, Dolphin, Thunar, a browser, anywhere; both GTK
   file lists and plain `text/uri-list` are accepted. Dragging a file over the dial arms it: the
   rim turns accent and the centre says `release to load`. Nothing to drop? Copy the file first
   (a *selected* file is not readable across Wayland surfaces) — the wheel picks it up after 4 s.
2. The dial rebuilds into sectors — one per valid target for that selection. A second drop
   replaces the selection instead of being ignored.
3. **Hover** a sector: it lifts, its rim lights, and the centre line previews the real result
   name in the accent colour (`→ photo.webp`, `→ pack.tar.gz`).
4. **Pick** by clicking, typing **1–9**, scrolling, arrowing and pressing **Enter** (only when a
   sector is lit), or **Tab**-cycling pages. **Esc** always cancels; pressing **SUPER+Y** again
   closes the dial instead of opening a second one.
5. Conversions run one after another; a hairline progress arc fills around the rim, the centre
   shows `Converting 2 of 3`, results land next to the originals, and swaync gets a summary.

Text is set in **SF Pro Display / SF Pro Text** when installed, falling back to **Inter** (which
is installed here, and is the closest open grotesque), then to the GTK default. The two dots
under the centre hint are the page indicator: the accent dot is the page you are on.

### Pages

| Page | Keys | Contents |
|---|---|---|
| Conversions | **Shift** (default) | formats for the dropped file's category |
| Actions | **Ctrl** | Copy · Copy Path · Terminal · Compress · Extract |
| Tools | **Alt** | Strip Meta · Rotate 90 · Resize 50 · Mute · Normalize |

**Tab** cycles the three pages without holding a modifier. **←/→** (or **↑/↓**) walk the sectors,
**Enter** takes the highlighted one, **ESC** cancels at any time. Sectors are numbered
left-to-right from 12 o'clock, so `1–9` is a fast path.

### What the wheel decides for you

- **Category** comes from the extension, falling back to MIME sniffing: image, audio, video,
  pdf, office, spreadsheet, text, subtitle, archive.
- **Mixed drop** (e.g. a PNG and an MP3) offers packing targets only: ZIP and TAR.GZ.
- **Capability gating** — every sector was checked against your installed engines at startup.
  No ImageMagick, no image sectors; no HEIC delegate, no HEIC sector.
- **Naming** — `photo.heic` → `photo.webp`; if that exists, `photo (2).webp`. Your source file
  is never modified or overwritten, and neither is any unrelated file with the same name.

---

## 3. File actions (Ctrl page)

| Action | What it does |
|---|---|
| **Copy** | puts the selection on the clipboard as `text/uri-list`, ready to paste in a file manager |
| **Copy Path** | puts newline-separated absolute paths on the clipboard |
| **Terminal** | opens the file's folder (or the folder itself) in your terminal |
| **Compress** | ZIP (or TAR.GZ) of the selection, including folders, beside the first item |
| **Extract** | unpacks each archive into a sibling `name_extracted/` folder |

Terminal is auto-detected from `kitty`, `wezterm`, `foot`, `ghostty`, `alacritty`, `konsole`,
`gnome-terminal` — override it with `terminal = "wezterm"` in the config.

## 3b. Editing tools (Alt page)

| Tool | Applies to | What it does |
|---|---|---|
| **Strip Meta** | image, audio, video | `magick -strip` / `ffmpeg -map_metadata -1 -c copy` |
| **Rotate 90** | image | `magick -rotate 90` |
| **Resize 50** | image | `magick -resize 50%` |
| **Mute** | video | drops all audio streams with stream copy |
| **Normalize** | audio | ffmpeg `loudnorm` |

Each tool writes a new sibling file (`photo-clean.png`, `clip-muted.mp4`); the original is
never touched. Tools are gated on `magick`/`ffmpeg` and use fixed, sensible parameters —
no dialogs yet. The same tools work headlessly: `mandarine convert photo.png --to strip`.

---

## 4. What converts to what

The matrix is transcribed from Tangerine v1.4 and then filtered by your engines
([full table](tangerine-teardown.md#conversion-matrix-observed-zext-in-home-bu_sezi2js)).
The Linux-feasible subset:

| Drop this | Get these sectors |
|---|---|
| `jpg png webp heic tiff svg avif bmp` | `jpg png webp heic tiff avif bmp pdf` |
| `mp3 m4a wav flac ogg opus aiff wma` | any of the same list |
| `mp4 mov mkv webm avi wmv` | the other containers + `gif` + `mp3` |
| `gif` | `mp4 mov mkv webm avi wmv` |
| `pdf` | `jpg png txt md html`, plus **merge** (2+ PDFs) and **split** (1 PDF) |
| `docx doc odt rtf ppt pptx` | `pdf txt html docx odt png jpg` |
| `xlsx xls ods csv tsv` | `pdf csv tsv xlsx ods` |
| `srt vtt` | `srt vtt txt` |
| `zip tar gz rar 7z …` | `zip tar gz` (repack), **extract** |
| `md html epub` | `pdf docx txt md html epub` — only when `pandoc` or Calibre is installed |

Notes:

- `xls`, `ppt`, `rtf`, `rar` are never offered as *outputs* — that is Tangerine's own rule.
- PDF→`docx`/`epub` needs `pandoc` (or `ebook-convert`); it's hidden otherwise.
- Document/spreadsheet conversions go through LibreOffice into a private temp dir and are copied
  to a collision-free name, because `soffice` names output after the input stem and refuses to
  overwrite. Your existing files are never moved or renamed.

---

## 5. Headless CLI

The wheel and the CLI share one conversion engine, so anything the wheel does is scriptable.

```sh
mandarine convert FILE... --to TARGET [--out-dir DIR]
mandarine action  NAME FILE...
mandarine list
mandarine init-config
mandarine wheel [--x N --y N]        # the overlay (default with no arguments)
```

Examples:

```sh
mandarine convert vacation.heic --to webp
mandarine convert *.png --to jpg --out-dir converted/
mandarine convert invoice.pdf --to txt
mandarine convert a.pdf b.pdf --to merge          # -> a.pdf (unique name on collision)
mandarine convert report.pdf --to split           # -> report_pages/report-1.pdf, -2, ...
mandarine convert shots.zip --to tar.gz           # archive repack
mandarine convert clip.srt --to vtt

mandarine action copy-path photo.png              # wl-copy
mandarine action terminal .                       # open terminal here
mandarine action compress folder                 # folder.zip
mandarine action extract photos.zip              # photos_extracted/
```

Exit status is `0` when every job succeeded, `1` if any failed (failures are also printed).
Progress goes to stderr, results to stdout, so `mandarine convert … --to jpg | wc -l` counts
outputs.

`--to` accepts aliases: `jpeg`→jpg, `tif`→tiff, `aif`→aiff, `htm`→html, `tar.gz`/`tgz`→targz.

---

## 6. Configuration

`mandarine init-config` writes a fully-commented default file to
`~/.config/mandarine/config.toml`. Partial files are fine — anything you leave out keeps its
default, and unknown keys are rejected loudly (`deny_unknown_fields`).

```toml
notify = true                 # swaync summaries
audio_bitrate = "192k"        # mp3/m4a/wma bitrate
video_crf = 23                # x264/x265 quality (lower = better)
pdf_dpi = 300                 # PDF -> raster resolution
clipboard_fallback_secs = 4   # 0 disables the clipboard fallback
terminal = "auto"             # or "kitty", "wezterm", ...

[wheel]
size = 440                    # window/sector diameter in px
inner_radius = 0.42           # dead zone in the centre, fraction of outer radius
sector_gap_deg = 3.0          # gap between sectors
```

> [!warning] Known contract gap
> `clipboard_fallback_secs = 0` is intended to disable the clipboard fallback, but the current
> implementation still schedules an immediate clipboard read. Keep the default at `4` until the
> opt-out is fixed and covered by a regression test.

### Custom sectors

Any command line can become a wheel sector. `{in}` and `{out}` are intended to be substituted with
absolute paths and executed without a shell. **Current gap:** the implementation still tokenizes
custom steps with `split_whitespace()`, so paths containing spaces are not argv-safe yet. Treat
custom targets as a roadmap feature until that parser/schema is corrected.

```toml
[[custom_targets]]
id = "png90"
label = "PNG q90"
ext = "png"
categories = ["image"]        # image, audio, video, pdf, office, spreadsheet, text, subtitle, archive
steps = ["magick", "{in}", "-strip", "-quality", "90", "{out}"]
```

The output file is named `{out}` only if the command writes it there; Mandarine pre-computes a
collision-free path from `ext` and reports it.

---

## 7. Theming

The material is deliberately **neutral charcoal glass** — Apple's rule is that glass only means
something when there is a backdrop under it, so the base never takes the wallpaper's hue. The
one wallpaper-derived value is the accent: matugen's `primary` from
`~/.config/ml4w/colors/colors.json` (the ML4W setup this machine already runs) lights the hover
fill, the rim glow, the progress arc and the active page dot. Without matugen it falls back to
Apple blue `#0a84ff`.

Two decisions keep it from looking like a tinted rectangle over somebody's wallpaper:

- **The shadow lives outside the rim.** The twenty-two stacked shadow discs are clipped to the
  ring beyond the disc, so no shadow leaks under the glass and muddies it.
- **Light is painted, not borrowed.** A top-down linear gloss plus a 1.6 px catch-light on the
  upper rim give the material its thickness; the body darkens toward the base. There are no
  radial-gradient blobs — GTK leaves a visible seam where a radial stop ends, which read as a
  smudge.

Backdrop blur is the compositor's job, not GTK's: the `mandarine-glass` layer rule above asks
Hyprland to blur the desktop behind the layer. Change the wallpaper, reopen the wheel, and the
accent follows.

Geometry lives in `~/.config/mandarine/config.toml`: `wheel.size` (disc diameter, default 440),
`inner_radius` (centre well, 0.42 of the radius) and `sector_gap_deg` (3°).

---

## 8. Troubleshooting

| Symptom | Fix |
|---|---|
| Nothing happens on SUPER+Y | Bind lives in `~/.config/ml4w/hypr/custom.lua` (stock `custom.lua` is `chattr +i`); `hyprctl configerrors` |
| Dial lands on the wrong monitor | Placement follows `hyprctl cursorpos`; on a multi-monitor layout with a negative offset, pass `--x/--y` explicitly |
| No backdrop blur behind the dial | Optional: add the `blur = true` window rule above and enable `decoration:blur:enabled`. The material reads correctly without it |
| A drop still does nothing | Check `~/.local/state/mandarine/dnd.log`: `enter` without `drop` is a compositor problem, an empty file is a drag that never reached the dial |
| No sectors for a file type | That engine is missing — run `mandarine list` |
| Wheel appears on the wrong monitor | Placement reads `hyprctl cursorpos -j`; check the compositor keeps the cursor on the focused output |
| `wl-copy not found` on a Copy action | `sudo pacman -S wl-clipboard` |
| HEIC/AVIF sector missing | ImageMagick lacks the delegate: `magick -list format \| grep -i heic` |
| `pandoc not found` | Expected on this machine; install `pandoc` or Calibre for md/html/epub targets |
| soffice conversion is slow the first time | LibreOffice warms its profile on first use in the GUI; later runs are fast |
| GTK prints `Theme parser error: gtk-dark.css` | Pre-existing theme issue, unrelated to Mandarine; the wheel still draws |

Uninstall: `rm ~/.local/bin/mandarine ~/.config/mandarine -r`.

---

## 9. Development

```sh
cargo test        # 78 unit tests — argv builders, matrix parity, registry gating, naming, actions, tools, wheel geometry, platform parsing
cargo clippy --all-targets
cargo run -- list # probe this machine without building release
```

Argv builders are pure functions, so most behaviour is verified without executing a real
conversion; `docs/tangerine-teardown.md` records where each rule came from.
