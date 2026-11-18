# Mandarine — platform support

The wheel core is portable GTK4. Only `src/platform.rs` (and the layer-shell strip on Linux)
knows about the OS. Two native targets are supported as of 2026-11-16: **Linux** and **Windows**.
macOS is not a target (no GTK4 app bundle or macOS native adapter is shipped).

## Session detection

`platform::session()` picks the adapter at runtime:

| Detected by | Session | Hotkey | Clipboard | Placement |
|---|---|---|---|---|
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland | compositor bind (`SUPER+Y`) | `wl-copy` | layer-shell OVERLAY, placed by margins |
| `WAYLAND_DISPLAY` | generic Wayland | compositor bind | `wl-copy` | layer-shell when available, else WM-centred toplevel |
| `DISPLAY` | X11 | DE custom shortcut | `xclip` | WM-centred toplevel; `xdotool` supplies pointer position |
| `target_os = windows` | Windows | **Win+Y** registered in-app (`RegisterHotKey`) | PowerShell `Set-Clipboard` | WM-centred toplevel |

GTK4 has no public API to position a toplevel or set "always on top"; compositor/WM owns both.
That is why the layer-shell path is the only cursor-accurate one. On X11/Windows the
deterministic entry points are `mandarine wheel --x N --y N` and the file-manager integration,
which preloads files and needs no placement.

## Linux

- **Arch/Hyprland** keeps the first-class path: layer-shell wheel namespace `mandarine`,
  edge strip (`mandarine listen`), `hyprctl` pointer nudge, matugen/pywal accent.
- **X11 / GNOME / KDE / XFCE**: `install.sh` no longer requires `hyprctl`. It installs a
  freedesktop `.desktop` file (shows under *Open With* in Nautilus/Dolphin/Thunar/Nemo) and,
  when Nautilus is present, a right-click script. Install `xclip` for clipboard actions and
  `xdotool` if you want the wheel to open at the pointer. Bind a DE shortcut to `mandarine`.
- Build: `./install.sh` (or `cargo build --release`).

## Windows

- Build with MSYS2's GTK4 toolchain: `pacman -S mingw-w64-x86_64-gtk4` then
  `cargo build --release` **from inside the MINGW64 shell** (pkg-config must see GTK4).
- The exe needs GTK4's DLLs at runtime. `install.ps1` detects MSYS2's `mingw64\bin`, adds it
  to the user PATH, and runs `mandarine.exe --version` to verify the binary actually starts
  before registering the hotkey and shell verbs. If MSYS2 is not found it prints the pacman
  command instead of failing silently.
- Install with `powershell -ExecutionPolicy Bypass -File windows\install.ps1`. It copies the
  binary to `%LOCALAPPDATA%\Mandarine`, adds it to the user PATH, registers a logon `Run` entry
  for `mandarine daemon` (native Win+Y hotkey), and adds an Explorer context verb plus a Send To
  shortcut (multi-file).
- The accent colour comes from DWM's `DwmGetColorizationColor`, so the wheel follows the
  Windows theme without matugen.
- Clipboard file copies are written as a comma-separated path list via PowerShell; this is a
  deliberate simplification of the Wayland `text/uri-list` path (no CF_HDROP yet).
- Exact pointer placement on Windows is not implemented — GTK4 cannot move a toplevel. Use the
  Explorer/Send To integration or `--x/--y`.

## Portable split (what changed)

- `Cargo.toml`: `gtk4-layer-shell` is now a Linux-only target dependency.
- `src/platform.rs`: session detection, style/placement, cursor, monitor geometry, clipboard,
  hotkey registration, and OS accent — all behind one seam.
- `src/ui/window.rs`, `src/cli.rs`: route through the platform seam; Hyprland keeps its
  layer-shell/`hyprctl` behaviour.
- `src/main.rs`: `GDK_BACKEND=wayland` is only forced on Linux sessions with `WAYLAND_DISPLAY`.
- `src/ui/strip.rs` and `listen`: Linux-only — a layer shell has no Windows equivalent.

## What is deliberately degraded on Windows

- **No drag-to-edge strip** — that is a Wayland layer-shell feature. Explorer context menu and
  Send To are the native equivalents.
- **Copy action** writes a comma-separated path list through `Set-Clipboard`, not a real
  CF_HDROP file clipboard as on Wayland. Pasting into Explorer gives text, not file objects.
- **Wheel opens WM-centred** — GTK4 cannot position a toplevel. Use `--x/--y` if you need exact
  placement (e.g. from a launcher script).
- **Engines are separate installs** — ffmpeg, ImageMagick, poppler, LibreOffice, etc. must be on
  PATH (MSYS2 packages or native Windows builds); missing ones simply hide their targets.

## Verification

- Linux: `cargo test` — 78 unit tests pass; release/debug builds succeed.
- Windows: the Win32 adapter uses `RegisterHotKey`, `GetMessageW`, `GetCursorPos` and
  `DwmGetColorizationColor` through direct FFI. **It has not been compiled or run on Windows in
  this environment** — no Windows toolchain or target is installed here. Treat the Windows path
  as unverified until built on MSYS2.
