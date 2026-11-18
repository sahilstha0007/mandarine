use gtk4::prelude::*;
use gtk4::{gdk, gio};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use super::Msg;

/// Bitmask of drag actions a surface advertises. `COPY` for copy-on-drop
/// (the common case), `MOVE` so file managers that prefer move semantics
/// (e.g. dragging within the same filesystem) are not rejected outright.
fn dnd_actions() -> gdk::DragAction {
    gdk::DragAction::COPY | gdk::DragAction::MOVE
}

/// Set `MANDARINE_DND_LOG=/path/to/file` to record every drag event the wheel
/// receives. Drag-and-drop is the one part of this app that cannot be unit
/// tested and that differs per compositor, so the log is the only honest
/// evidence when a drop "does nothing".
///
/// The log now records full payload traces so a dropped `application/x-xdg-protocol-arg`
/// or `text/uri-list` body can be inspected when a drop lands but resolves to
/// nothing usable.
pub(crate) fn log_dnd(line: &str) {
    // On by default, because "the drop does nothing" is unfalsifiable otherwise
    // and drag-and-drop is compositor-specific. `MANDARINE_DND_LOG` overrides
    // the path; `MANDARINE_DND_LOG=off` disables it.
    let path = match std::env::var_os("MANDARINE_DND_LOG") {
        Some(p) if p == "off" => return,
        Some(p) => p,
        None => {
            let base = std::env::var_os("XDG_STATE_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".local/state")
                });
            let dir = base.join("mandarine");
            let _ = std::fs::create_dir_all(&dir);
            dir.join("dnd.log").into()
        }
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

fn log_paths(what: &str, paths: &[PathBuf]) {
    log_dnd(&format!(
        "{what} n={} paths={:?}",
        paths.len(),
        paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
    ));
}

/// What a drop surface does with a drag: one set of handlers, installed for
/// every accepted payload shape. The wheel turns them into `Msg`s for its own
/// loop; the standby strip turns them into reveal/retract messages.
///
/// `Rc` rather than `Box` because a GTK controller closure must be `'static`
/// and each payload shape needs its own copy.
#[derive(Clone)]
pub(crate) struct Targets {
    pub enter: Rc<dyn Fn(f64, f64)>,
    pub leave: Rc<dyn Fn()>,
    /// Returns whether the surface consumed the drop.
    pub drop: Rc<dyn Fn(Vec<PathBuf>) -> bool>,
}

/// Attach drop targets to a widget.
///
/// Two formats, because they are not interchangeable: GTK4 apps (Nautilus 43+)
/// hand over a `GdkFileList`, while GTK3 apps, Qt file managers (Dolphin),
/// Thunar and every browser only publish text — `text/uri-list`, or
/// `application/x-xdg-protocol-arg` for xdg-utils launches. Both text shapes
/// arrive as a `String`, so one target covers them; accepting only the file
/// list meant a drag from most of those silently did nothing.
pub(crate) fn attach_targets(w: &gtk4::Widget, t: &Targets) {
    // --- GdkFileList target (GTK4 native) ---
    let target = gtk4::DropTarget::new(gdk::FileList::static_type(), dnd_actions());
    let enter = t.enter.clone();
    target.connect_enter(move |_, x, y| {
        log_dnd(&format!("enter filelist at {x:.0},{y:.0} actions=COPY|MOVE"));
        enter(x, y);
        dnd_actions()
    });
    let leave = t.leave.clone();
    target.connect_leave(move |_| {
        log_dnd("leave filelist");
        leave();
    });
    let drop = t.drop.clone();
    target.connect_drop(move |_, value, _, _| match value.get::<gdk::FileList>() {
        Ok(list) => {
            let paths: Vec<PathBuf> = list
                .files()
                .iter()
                .filter_map(|f| f.path())
                .collect();
            log_paths("drop filelist", &paths);
            if paths.is_empty() {
                false
            } else {
                drop(paths)
            }
        }
        Err(e) => {
            log_dnd(&format!("drop filelist ERR {e:?}"));
            false
        }
    });
    w.add_controller(target);

    // --- text target: text/uri-list and application/x-xdg-protocol-arg ---
    let target = gtk4::DropTarget::new(String::static_type(), dnd_actions());
    let enter = t.enter.clone();
    target.connect_enter(move |_, x, y| {
        log_dnd(&format!("enter text at {x:.0},{y:.0} actions=COPY|MOVE"));
        enter(x, y);
        dnd_actions()
    });
    let leave = t.leave.clone();
    target.connect_leave(move |_| {
        log_dnd("leave text");
        leave();
    });
    let drop = t.drop.clone();
    target.connect_drop(move |_, value, _, _| {
        let Ok(text) = value.get::<String>() else {
            log_dnd("drop text ERR not-a-string");
            return false;
        };
        let paths = parse_uri_list(text.as_bytes());
        log_dnd(&format!(
            "drop text n={} raw={:?} paths={:?}",
            paths.len(),
            text,
            paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
        ));
        if paths.is_empty() {
            false
        } else {
            drop(paths)
        }
    });
    w.add_controller(target);
}

/// The wheel's drop surface: hover drives the armed ring, a drop loads files.
pub fn attach(w: &gtk4::Widget, tx: Sender<Msg>) {
    let hover = tx.clone();
    let left = tx.clone();
    attach_targets(
        w,
        &Targets {
            enter: Rc::new(move |_, _| {
                let _ = hover.send(Msg::DragHover(true));
            }),
            leave: Rc::new(move || {
                let _ = left.send(Msg::DragHover(false));
            }),
            drop: Rc::new(move |paths| tx.send(Msg::Files(paths)).is_ok()),
        },
    );
}

/// After `delay` without a drop, offer files currently on the clipboard
/// (Nautilus copies file URIs on copy; wl-clipboard hands them over).
///
/// The clipboard read is performed on a background thread so it never blocks
/// the GTK main render thread. If `clipboard_fallback_secs = 0` in config, the
/// caller skips spawning this entirely (see `build()` in `window.rs`).
pub fn clipboard_fallback(delay: Duration, tx: Sender<Msg>) {
    if delay.is_zero() {
        // `clipboard_fallback_secs = 0` is a documented opt-out: do not read
        // the clipboard at all.
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let _ = tx.send(Msg::Clipboard(clipboard_files()));
    });
}

/// Read file paths off the clipboard as either a URI list (Nautilus, Dolphin)
/// or plain text holding URIs/paths (`wl-copy file:///tmp/a.png`, `pwd | wl-copy`).
fn clipboard_files() -> Vec<PathBuf> {
    for kind in ["text/uri-list", "text/plain"] {
        let Ok(out) = std::process::Command::new("wl-paste")
            .args(["--type", kind])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let paths = parse_uri_list(&out.stdout);
        if !paths.is_empty() {
            return paths;
        }
    }
    Vec::new()
}

pub(crate) fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            if line.starts_with("file://") {
                gio::File::for_uri(line).path()
            } else {
                let p = PathBuf::from(line);
                p.is_absolute().then_some(p)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uri_list() {
        let list = b"file:///home/sahil/a%20b.png\r\nfile:///tmp/x.pdf\n\ncruft\n";
        let paths = parse_uri_list(list);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/sahil/a b.png"));
        assert_eq!(paths[1], PathBuf::from("/tmp/x.pdf"));
    }

    #[test]
    fn plain_paths_and_uris_mix() {
        let text = b"file:///tmp/a.png\n/tmp/b.pdf\nnot-a-path\n";
        assert_eq!(
            parse_uri_list(text),
            vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.pdf")]
        );
    }

    #[test]
    fn dnd_actions_include_copy_and_move() {
        let actions = dnd_actions();
        assert!(actions.contains(gdk::DragAction::COPY));
        assert!(actions.contains(gdk::DragAction::MOVE));
    }
}
