pub mod drop;
#[cfg(target_os = "linux")]
pub mod strip;
pub mod wheel;
pub mod window;

use anyhow::Result;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::Cell;
use std::path::PathBuf;

pub use window::Msg;

thread_local! {
    /// The wheel is a single-instance app: pressing SUPER+Y again toggles it
    /// shut instead of stacking a second dial on top of the first one.
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Launch the wheel overlay GUI. `x`/`y` override cursor placement (debug),
/// `files` pre-loads the wheel (file-manager integration, no drag needed).
pub fn run(x: Option<i32>, y: Option<i32>, files: Vec<std::path::PathBuf>) -> Result<()> {
    let app = gtk4::Application::new(Some("io.sahil.mandarine"), gtk4::gio::ApplicationFlags::empty());
    app.connect_activate(move |app| {
        if OPEN.get() {
            app.quit();
            return;
        }
        OPEN.set(true);
        window::build(app, x, y, files.clone());
    });
    // Pass GTK an empty argv: the process arguments were already consumed by
    // clap, and GTK would otherwise reject `--x`/`--y` as its own unknown flags.
    app.run_with_args::<&str>(&[]);
    Ok(())
}

/// Resident mode: keep a standby strip mapped on a screen edge, and open the
/// wheel under the cursor when a file drag reaches it.
///
/// This is the only way to react to a drag without a hotkey: Wayland has no
/// global drag event, so the app has to already own a surface the drag can be
/// offered to. One process holds both the strip and an unmapped wheel; a reveal
/// is a map plus a `hyprctl` move, not a new process.
///
/// `NON_UNIQUE` on purpose. With the default single-instance flag this process
/// would become the primary instance of `io.sahil.mandarine` and swallow the
/// activation that `mandarine wheel --files …` (the file-manager integration)
/// relies on. Both processes carry the same application id, so the existing
/// Hyprland window rules keep matching the revealed wheel; the pidfile below is
/// what stops two listeners.
#[cfg(target_os = "linux")]
pub fn listen(edge: strip::EdgeChoice) -> Result<()> {
    if already_listening() {
        anyhow::bail!("mandarine listen is already running");
    }
    write_pidfile();

    let app = gtk4::Application::new(
        Some("io.sahil.mandarine"),
        gio::ApplicationFlags::NON_UNIQUE,
    );
    let started = Cell::new(false);
    let failed = std::rc::Rc::new(Cell::new(false));

    app.connect_activate({
        let failed = failed.clone();
        move |app| {
            if started.replace(true) {
                return;
            }
            // The wheel is built once and only mapped on demand: rebuilding it
            // per drag would re-register CSS, controllers and the message loop.
            let host = window::build_hidden(app);
            let (tx, rx) = std::sync::mpsc::channel::<strip::StripMsg>();
            if strip::build(app, tx, edge).is_none() {
                failed.set(true);
                app.quit();
                return;
            }
            // One poller for the strip channel. 60 ms is imperceptible during a
            // drag and costs nothing while idle; nothing else runs in standby.
            glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        strip::StripMsg::Enter { x, y } => window::reveal(&host, x, y),
                        strip::StripMsg::Leave => window::strip_left(&host),
                        strip::StripMsg::Files(paths) => {
                            // Dropped straight on the strip, without the reveal
                            // having been routed the drag yet.
                            let (x, y) = window::cursor_pos();
                            window::reveal(&host, x, y);
                            window::deliver_files(&host, paths);
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }
    });

    app.run_with_args::<&str>(&[]);
    remove_pidfile();
    if failed.get() {
        anyhow::bail!(
            "this compositor does not implement wlr-layer-shell; use `mandarine wheel` instead"
        );
    }
    Ok(())
}

fn pidfile() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("mandarine-listen.pid")
}

fn write_pidfile() {
    let _ = std::fs::write(pidfile(), std::process::id().to_string());
}

fn remove_pidfile() {
    let _ = std::fs::remove_file(pidfile());
}

/// A listener is identified by its pidfile *and* a live `/proc` entry whose
/// command line says `listen`: a recycled pid must not block a new listener.
fn already_listening() -> bool {
    let Ok(pid) = std::fs::read_to_string(pidfile()) else {
        return false;
    };
    let pid = pid.trim();
    if pid.is_empty() {
        return false;
    }
    let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    cmdline.contains("listen")
}
