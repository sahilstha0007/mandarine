use clap::Parser;
use mandarine::cli::{Cli, Command};

/// Force GTK4 onto the native Wayland backend before any GTK initialization.
/// Without this, GTK may silently fall back to XWayland, and file drops from
/// Wayland-native file managers (Nautilus, Dolphin, Thunar) are refused — the
/// drop target accepts the drag but never receives a `drop` event.
fn ensure_wayland_backend() {
    // Only force Wayland on Linux sessions that actually have a Wayland display;
    // X11 and Windows need their native GDK backend.
    if !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    if std::env::var("GDK_BACKEND").is_err() {
        // Safety: setting GDK_BACKEND before GTK initialization is safe;
        // it only affects backend selection and is done once at process start.
        unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    }
    if std::env::var("GDK_DISPLAY").is_err()
        && std::env::var("WAYLAND_DISPLAY").is_ok()
    {
        // Let GTK discover the Wayland display from the environment naturally.
    }
}

fn main() {
    ensure_wayland_backend();
    let cli = Cli::parse();
    let code = match cli.command {
        None => mandarine::ui::run(None, None, vec![]).map(|_| 0).unwrap_or_else(|e| fail(&e)),
        Some(Command::Wheel { x, y, files }) => {
            mandarine::ui::run(x, y, files).map(|_| 0).unwrap_or_else(|e| fail(&e))
        }
        Some(cmd) => mandarine::cli::run(cmd).unwrap_or_else(|e| fail(&e)),
    };
    std::process::exit(code);
}

fn fail(e: &anyhow::Error) -> i32 {
    eprintln!("mandarine: {e:#}");
    1
}
