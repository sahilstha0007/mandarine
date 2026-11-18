//! Resident edge strip: how Mandarine learns that a drag started.
//!
//! Wayland has no "a drag began" event. A client only finds out by being the
//! surface a drag is offered to, so the trigger has to be a surface that is
//! already on screen: a hairline layer-shell strip hugging one screen edge.
//! It accepts input, paints a quiet accent line, and forwards three things —
//! drag entered, drag left, files dropped — to whoever owns the wheel.
//!
//! Hyprland hit-tests the OVERLAY and TOP layers while a drag is active
//! (`mouseMoveUnified` skips its "keep focus while a button is held" shortcut
//! when `dndActive()`), so a layer surface is a valid drop target there. An
//! older revision of this app assumed otherwise and used a toplevel; that is no
//! longer true as of 0.56.
//!
//! Standby is deliberately inert: one mapped 6 px surface, no tick callback, no
//! timers, nothing animating. Painting only happens on a drag enter/leave.

use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use super::drop::{self, Targets};

/// Thickness of the strip along the edge it hugs.
///
/// This is the only part of a resident Mandarine that takes pointer input, so
/// every pixel is a click the desktop below does not get. A drag aimed at a
/// screen edge reliably reaches the outermost few pixels, which is why it can
/// be this thin.
pub const THICKNESS: i32 = 6;

const NAMESPACE: &str = "mandarine-strip";

/// Which screen edge the standby strip lives on. Exposed as `--edge` because
/// which edge is free depends on the user's bars and docks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EdgeChoice {
    Top,
    Bottom,
    Left,
    Right,
}

/// What the strip observed. Delivered on the GTK main thread, in order.
pub enum StripMsg {
    /// A drag entered the strip; global cursor position at that moment.
    Enter { x: i32, y: i32 },
    /// The drag left the strip, or was cancelled.
    Leave,
    /// Files were dropped directly on the strip.
    Files(Vec<PathBuf>),
}

/// Build and show the standby strip. Returns `None` when the compositor has no
/// `wlr-layer-shell`, which is a hard requirement: without it there is no way
/// to keep a drop target on screen without opening a window.
pub fn build(app: &gtk4::Application, tx: Sender<StripMsg>, edge: EdgeChoice) -> Option<gtk4::Window> {
    if !gtk4_layer_shell::is_supported() {
        drop::log_dnd("strip: compositor does not implement wlr-layer-shell");
        return None;
    }

    let window = gtk4::Window::new();
    window.set_application(Some(app));
    window.set_decorated(false);
    window.set_resizable(false);
    window.set_focusable(false);

    // The visible hairline lives on a child: a Window's own CSS node does not
    // paint a background reliably across themes, and the child is also what the
    // drop controllers attach to.
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    line.add_css_class("mandarine-strip-line");
    line.set_can_focus(false);
    window.set_child(Some(&line));

    window.init_layer_shell();
    window.set_namespace(Some(NAMESPACE));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    // Reserve nothing: the strip floats over the desktop instead of shrinking
    // the usable area for every window.
    window.set_exclusive_zone(0);
    window.set_anchor(anchor_of(edge), true);
    // Both opposite anchors plus an explicit extent: Hyprland honours the
    // requested size even with two opposite anchors (gtk4-layer-shell fills in
    // GTK's 200 px fallback otherwise), so the surface is sized from the output
    // it is pinned to instead of relying on stretching.
    let monitor = monitor_under_cursor();
    if let Some(mon) = &monitor {
        window.set_monitor(Some(mon));
    }
    let (extent_w, extent_h) = monitor
        .as_ref()
        .map(|m| {
            let g = m.geometry();
            (g.width(), g.height())
        })
        .unwrap_or((1920, 1080));
    match edge {
        EdgeChoice::Top | EdgeChoice::Bottom => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_default_size(extent_w, THICKNESS);
        }
        EdgeChoice::Left | EdgeChoice::Right => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_default_size(THICKNESS, extent_h);
        }
    }
    apply_css(&window);

    let tx_enter = tx.clone();
    let tx_leave = tx.clone();
    let tx_files = tx;
    let line_enter = line.clone();
    let line_leave = line.clone();
    let line_drop = line.clone();
    drop::attach_targets(
        line.upcast_ref(),
        &Targets {
            enter: Rc::new(move |_, _| {
                line_enter.add_css_class("armed");
                let (x, y) = super::window::cursor_pos();
                let _ = tx_enter.send(StripMsg::Enter { x, y });
            }),
            leave: Rc::new(move || {
                line_leave.remove_css_class("armed");
                // A cancelled drag (Esc, or a source that withdrew) also lands
                // here: GTK sends leave, never drop.
                let _ = tx_leave.send(StripMsg::Leave);
            }),
            drop: Rc::new(move |paths| {
                line_drop.remove_css_class("armed");
                tx_files.send(StripMsg::Files(paths)).is_ok()
            }),
        },
    );

    window.present();
    Some(window)
}

/// The output the pointer is on, or the first one. The strip belongs to the
/// screen the user is working on, and pinning it also makes the extent below
/// unambiguous on a multi-monitor desktop.
fn monitor_under_cursor() -> Option<gtk4::gdk::Monitor> {
    let display = gtk4::gdk::Display::default()?;
    let (cx, cy) = super::window::cursor_pos();
    let monitors = display.monitors();
    let mut first = None;
    for i in 0..monitors.n_items() {
        let Some(mon) = monitors.item(i).and_then(|o| o.downcast::<gtk4::gdk::Monitor>().ok())
        else {
            continue;
        };
        let g = mon.geometry();
        if cx >= g.x() && cx < g.x() + g.width() && cy >= g.y() && cy < g.y() + g.height() {
            return Some(mon);
        }
        first.get_or_insert(mon);
    }
    first
}

fn anchor_of(edge: EdgeChoice) -> Edge {
    match edge {
        EdgeChoice::Top => Edge::Top,
        EdgeChoice::Bottom => Edge::Bottom,
        EdgeChoice::Left => Edge::Left,
        EdgeChoice::Right => Edge::Right,
    }
}

fn apply_css(window: &gtk4::Window) {
    let accent = crate::theming::load().accent;
    let rgb = format!(
        "{},{},{}",
        (accent.0 * 255.0).round() as u32,
        (accent.1 * 255.0).round() as u32,
        (accent.2 * 255.0).round() as u32
    );
    // Rest: a hairline you can find if you look for it. Armed: the accent, so a
    // drag over the edge visibly arms the app before the wheel even appears.
    let css = format!(
        ".mandarine-strip {{ background-color: transparent; }}\n\
         .mandarine-strip-line {{\n\
           background-color: rgba({rgb},0.14);\n\
           border-radius: 3px;\n\
         }}\n\
         .mandarine-strip-line.armed {{ background-color: rgba({rgb},0.85); }}\n"
    );
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css);
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::RootExt::display(window),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
