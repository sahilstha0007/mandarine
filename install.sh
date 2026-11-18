#!/usr/bin/env bash
# Mandarine installer for Arch / Hyprland.
#
#   ./install.sh            build and install to ~/.local/bin
#   PREFIX=/usr/local ./install.sh
#
# It only installs the binary. Missing engines are reported, never installed
# silently — pacman commands are printed so you can decide.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say()  { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;31m!!!\033[0m %s\n' "$*"; }

say "Checking build dependencies"
missing=()
command -v cargo >/dev/null || missing+=("rust")
command -v pkg-config >/dev/null || missing+=("pkgconf")
pkg-config --exists gtk4 || missing+=("gtk4")
# hyprctl is optional: X11 and generic Wayland sessions use the portable path.
if ((${#missing[@]})); then
  warn "Missing: ${missing[*]}"
  echo "    sudo pacman -S --needed rust pkgconf gtk4 wl-clipboard"
  exit 1
fi

say "Checking runtime helpers"
optional=()
command -v wl-copy >/dev/null || optional+=("wl-clipboard (clipboard actions on Wayland)")
command -v xclip >/dev/null || optional+=("xclip (clipboard actions on X11)")
command -v xdotool >/dev/null || optional+=("xdotool (pointer position on X11)")
command -v hyprctl >/dev/null || optional+=("hyprland (native layer-shell placement + hotkey)")
command -v ffmpeg >/dev/null || optional+=("ffmpeg (audio/video/subtitles)")
command -v magick >/dev/null || optional+=("imagemagick (images)")
command -v pdftoppm >/dev/null || optional+=("poppler (pdf)")
command -v soffice >/dev/null || optional+=("libreoffice-fresh (documents/spreadsheets)")
command -v zip >/dev/null || optional+=("zip (archives)")
if ((${#optional[@]})); then
  say "Optional engines not found (targets for them stay hidden):"
  printf '    - %s\n' "${optional[@]}"
fi

say "Building release binary"
cd "$SRC_DIR"
cargo build --release

say "Installing to $BIN_DIR/mandarine"
mkdir -p "$BIN_DIR"
install -m 0755 target/release/mandarine "$BIN_DIR/mandarine"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on PATH — add it to your shell rc." ;;
esac

say "Installing file-manager integration"
# "Open With Mandarine" appears in Nautilus, Dolphin, Thunar and Nemo (all read
# freedesktop .desktop files). The Nautilus script adds a right-click shortcut.
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/mandarine.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Mandarine
Comment=Convert files with a radial wheel
Exec=mandarine wheel --files %F
Icon=application-x-executable
Terminal=false
Categories=Utility;
MimeType=image/*;audio/*;video/*;application/pdf;text/plain;text/markdown;
DESKTOP
if command -v nautilus >/dev/null; then
  mkdir -p "$HOME/.local/share/nautilus/scripts"
  cat > "$HOME/.local/share/nautilus/scripts/Mandarine" <<'SCRIPT'
#!/bin/sh
exec mandarine wheel --files "$@"
SCRIPT
  chmod +x "$HOME/.local/share/nautilus/scripts/Mandarine"
fi
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

say "Installed. Probed capabilities:"
"$BIN_DIR/mandarine" list | grep -v 'magick writable formats' | sed -n '1,30p'

cat <<'EOF'

Next steps
  1. Bind the wheel. ML4W dotfiles read Lua binds, so add to
     ~/.config/ml4w/hypr/custom.lua (the stock ~/.config/hypr/custom.lua is immutable):

         hl.bind("SUPER + Y", hl.dsp.exec_cmd("mandarine"))

  2. Optional — drag instead of pressing a key. Wayland has no global drag
     event, so this keeps a 6 px drop target on one screen edge and opens the
     wheel when a drag touches it. Add to your Hyprland autostart:

     Lua config (ML4W, Hyprland 0.56) — same file as the bind above:

         hl.on("hyprland.start", function()
             hl.exec_cmd("mandarine listen --edge top")
         end)

     Classic hyprlang config:

         exec-once = mandarine listen --edge top

     (`--edge bottom|left|right`; pick an edge your bar or dock leaves free.)
     The subcommand only exists in builds newer than the one that shipped it —
     if `mandarine listen --help` says "unrecognized subcommand", reinstall.

  3. Recommended — give the glass something to refract, so Hyprland blurs the
     desktop behind the wheel instead of it reading as flat charcoal. The dial
     is a layer surface, so this is a layer rule on its namespace:

         hl.layer_rule({
             name = "mandarine-glass",
             match = { namespace = "mandarine" },
             blur = true,
             ignore_alpha = 0.25,
         })

     No float/decorate/ontop window rules are needed any more: the wheel is not
     a window. Old `windowrulev2 = float, class:^(io.sahil.mandarine)$` lines are
     harmless but do nothing.

  4. Reload Hyprland:  hyprctl reload
  5. Press SUPER+Y — or drag a file to the edge you chose — then pick a sector.
     Shift = convert, Ctrl = actions. Hover previews the result, arrows + Enter
     work without the mouse.

  Optional:  mandarine init-config     # write ~/.config/mandarine/config.toml

  Full manual: docs/guide.md
EOF
