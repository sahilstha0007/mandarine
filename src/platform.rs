//! Platform adapters. The wheel core is portable GTK4; only these seams know
//! about Hyprland, X11 and Win32. Hyprland keeps its layer-shell path; other
//! sessions get a borderless always-on-top toplevel and OS-native clipboard /
//! hotkey / accent behaviour.

use gtk4::gdk;
use gtk4::gdk::prelude::{DisplayExt, MonitorExt};
use gtk4::gio::prelude::ListModelExt;
use gtk4::glib::object::Cast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    Hyprland,
    Wayland,
    X11,
    Windows,
    Other,
}

pub fn session() -> Session {
    if cfg!(target_os = "windows") {
        Session::Windows
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        Session::Hyprland
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Session::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        Session::X11
    } else {
        Session::Other
    }
}

/// Apply the platform's window chrome rules. The wheel paints its own rim, so
/// every session is borderless; X11/Windows additionally stay above other windows.
pub fn style_window(window: &gtk4::Window, size: i32) {
    use gtk4::prelude::*;
    // GTK4 exposes no "keep above" or programmatic position on toplevels; the
    // compositor/WM owns both. The layer-shell path handles Hyprland natively.
    window.set_decorated(false);
    window.set_default_size(size, size);
}

/// The layer-shell path places the wheel itself. On X11/Windows GTK4 cannot move
/// a toplevel, so the WM centres it; `--x/--y` and file-manager `--files` are the
/// deterministic entry points there.
pub fn place_toplevel(_window: &gtk4::Window, _left: i32, _top: i32) {}

/// Pointer position in the same coordinate space GDK uses, or `None` when the
/// platform does not expose one (generic Wayland).
pub fn cursor_pos() -> Option<(i32, i32)> {
    match session() {
        Session::Windows => windows::cursor_pos(),
        Session::Hyprland => hyprland_cursor(),
        Session::X11 => xdotool_cursor(),
        _ => None,
    }
}

fn hyprland_cursor() -> Option<(i32, i32)> {
    let out = std::process::Command::new("hyprctl").args(["cursorpos", "-j"]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let x = v.get("x")?.as_i64()? as i32;
    let y = v.get("y")?.as_i64()? as i32;
    (x != 0 || y != 0).then_some((x, y))
}

/// `xdotool getmouselocation --shell` prints `X=123`, `Y=456`. No link-time X11
/// dependency, so Wayland-only builds stay clean.
pub fn parse_xdotool(text: &str) -> Option<(i32, i32)> {
    let field = |key: &str| -> Option<i32> {
        text.lines().find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
    };
    Some((field("X=")?, field("Y=")?))
}

fn xdotool_cursor() -> Option<(i32, i32)> {
    let out = std::process::Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()?;
    parse_xdotool(&String::from_utf8_lossy(&out.stdout))
}

/// Output geometry for the active session. Hyprland reports logical coordinates;
/// elsewhere GDK's monitor list is authoritative.
pub fn monitor_geometry(cx: i32, cy: i32) -> (i32, i32, i32, i32) {
    if session() == Session::Hyprland {
        if let Some(r) = hyprland_monitor(cx, cy) {
            return r;
        }
    }
    if let Some(display) = gdk::Display::default() {
        let monitors = display.monitors();
        let monitor = (0..monitors.n_items())
            .filter_map(|i| monitors.item(i).and_then(|o| o.downcast::<gdk::Monitor>().ok()))
            .find(|m| {
                let g = m.geometry();
                cx >= g.x() && cx < g.x() + g.width() && cy >= g.y() && cy < g.y() + g.height()
            })
            .or_else(|| monitors.item(0).and_then(|o| o.downcast::<gdk::Monitor>().ok()));
        if let Some(m) = monitor {
            let g = m.geometry();
            return (g.x(), g.y(), g.width(), g.height());
        }
    }
    (0, 0, 1280, 720)
}

fn hyprland_monitor(cx: i32, cy: i32) -> Option<(i32, i32, i32, i32)> {
    let out = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let list = v.as_array()?;
    let rect = |m: &serde_json::Value| -> Option<(i32, i32, i32, i32)> {
        Some((
            m.get("x")?.as_i64()? as i32,
            m.get("y")?.as_i64()? as i32,
            m.get("width")?.as_i64()? as i32,
            m.get("height")?.as_i64()? as i32,
        ))
    };
    list.iter()
        .filter_map(rect)
        .find(|(x, y, w, h)| cx >= *x && cx < x + w && cy >= *y && cy < y + h)
        .or_else(|| list.first().and_then(rect))
}

/// Copy text/uri-list or plain text to the OS clipboard. Called from a worker
/// thread, so every branch is a subprocess — no GTK main-context calls here.
pub fn clipboard_copy(body: &str, mime: &str) -> anyhow::Result<()> {
    match session() {
        Session::Windows => windows::clipboard(body, mime),
        Session::X11 => {
            let mut child = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", mime])
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            use std::io::Write;
            if let Some(s) = child.stdin.as_mut() {
                s.write_all(body.as_bytes())?;
            }
            drop(child.stdin.take());
            anyhow::ensure!(child.wait()?.success(), "xclip failed");
            Ok(())
        }
        _ => crate::actions::wl_copy(body, mime),
    }
}

/// Register the global hotkey where the OS lets an app do that. Linux desktops
/// own global shortcuts themselves; their bind simply launches `mandarine`.
pub fn register_hotkey(on_pressed: impl Fn() + Send + Sync + 'static) {
    if session() == Session::Windows {
        windows::register_hotkey(on_pressed);
    }
}

/// OS accent colour as `#rrggbb`, when the platform exposes one. Linux keeps
/// its matugen / pywal readers in `theming.rs`.
#[cfg(target_os = "windows")]
pub fn accent_hex() -> Option<String> {
    windows::accent_hex()
}

#[cfg(not(target_os = "windows"))]
pub fn accent_hex() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
mod windows {
    pub fn cursor_pos() -> Option<(i32, i32)> { None }
    pub fn clipboard(_: &str, _: &str) -> anyhow::Result<()> { anyhow::bail!("Windows-only") }
    pub fn register_hotkey(_: impl Fn() + Send + Sync + 'static) {}
    pub fn accent_hex() -> Option<String> { None }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    #[repr(C)]
    struct Point { x: i32, y: i32 }
    #[repr(C)]
    struct Msg {
        hwnd: isize, message: u32, wparam: usize, lparam: isize,
        time: u32, pt: Point, private: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(p: *mut Point) -> i32;
        fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: isize, filter_min: u32, filter_max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
    }
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmGetColorizationColor(color: *mut u32, opaque: *mut i32) -> i32;
    }

    const MOD_WIN: u32 = 0x0008;
    const VK_Y: u32 = 0x0059;
    const WM_HOTKEY: u32 = 0x0312;

    pub fn cursor_pos() -> Option<(i32, i32)> {
        let mut p = Point { x: 0, y: 0 };
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
    }

    pub fn clipboard(body: &str, mime: &str) -> anyhow::Result<()> {
        // PowerShell is on every supported Windows; stdin avoids quoting the payload.
        let paths = if mime == "text/uri-list" {
            body.lines()
                .filter_map(|l| l.trim().strip_prefix("file://"))
                .map(|u| percent_decode(u))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            body.to_string()
        };
        let mut child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "$input | Out-String | Set-Clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        use std::io::Write;
        if let Some(s) = child.stdin.as_mut() {
            s.write_all(paths.as_bytes())?;
        }
        drop(child.stdin.take());
        anyhow::ensure!(child.wait()?.success(), "Set-Clipboard failed");
        Ok(())
    }

    fn percent_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(a * 16 + b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    pub fn register_hotkey(on_pressed: impl Fn() + Send + Sync + 'static) {
        let handler: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(on_pressed);
        std::thread::spawn(move || {
            if unsafe { RegisterHotKey(0, 1, MOD_WIN, VK_Y) } == 0 {
                return;
            }
            let mut msg = Msg {
                hwnd: 0, message: 0, wparam: 0, lparam: 0,
                time: 0, pt: Point { x: 0, y: 0 }, private: 0,
            };
            while unsafe { GetMessageW(&mut msg, 0, 0, 0) } > 0 {
                if msg.message == WM_HOTKEY {
                    let h = handler.clone();
                    let _ = glib::idle_add(move || {
                        h();
                        glib::ControlFlow::Break
                    });
                }
                unsafe { TranslateMessage(&msg); DispatchMessageW(&msg); }
            }
        });
    }

    pub fn accent_hex() -> Option<String> {
        let mut argb = 0u32;
        let mut opaque = 0i32;
        if unsafe { DwmGetColorizationColor(&mut argb, &mut opaque) } == 0 {
            return None;
        }
        let r = (argb >> 16) & 0xff;
        let g = (argb >> 8) & 0xff;
        let b = argb & 0xff;
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xdotool_shell_output() {
        let text = "X=123\nY=456\nSCREEN=0\nWINDOW=42\n";
        assert_eq!(parse_xdotool(text), Some((123, 456)));
        assert_eq!(parse_xdotool("X=nope\nY=2\n"), None);
    }
}
