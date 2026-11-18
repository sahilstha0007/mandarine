use crate::actions::{self, Action};
use crate::category;
use crate::config::Config;
use crate::convert;
use crate::engines;
use crate::notify;
use crate::theming::{self, Theme};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use super::drop;
use super::wheel::{self, Scene, WheelArea};

/// Shown while the wheel has nothing to work on. Naming the clipboard path
/// matters: a file that is only *selected* in Files is not readable from here,
/// so "copy it first" is the instruction people actually need.
const IDLE_HINT: &str = "drop, copy a file, or Esc";

/// Centre hint while a drag is still in flight over the dial.
const HOVER_HINT: &str = "release to load";

/// Width of the floating center stack (the well is ~185 px across).
const CENTER_W: f64 = 208.0;
const CENTER_H: f64 = 96.0;

/// Layer namespace. Hyprland matches `hl.layer_rule` on this, which is how the
/// glass gets a blurred desktop behind it — a surface with no namespace has no
/// backdrop to refract and reads as flat charcoal.
const LAYER_NAMESPACE: &str = "mandarine";

pub enum Msg {
    Files(Vec<PathBuf>),
    /// A drag is over the dial (true) or left it (false) — drives the armed ring.
    DragHover(bool),
    Clipboard(Vec<PathBuf>),
    /// Background worker result: paths resolved, categories detected, targets
    /// and actions computed off the GTK main thread.
    TargetsLoaded {
        paths: Vec<category::FileInfo>,
        targets: Vec<engines::Target>,
        actions: Vec<actions::Action>,
        tools: Vec<engines::Target>,
    },
    Progress(usize, usize, String),
    Finished(convert::BatchResult),
    ActionDone(Result<String, String>),
}

/// The wheel pages, mirroring Tangerine's modifier pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Shift — format conversions.
    #[default]
    Convert,
    /// Ctrl — file actions (copy, copy path, terminal, compress, extract).
    Actions,
    /// Tab — parameter-light editing tools (strip, rotate, resize, mute, normalize).
    Tools,
}

#[derive(Clone, Debug)]
pub enum Phase {
    Waiting,
    Ready,
    Running { current: usize, total: usize, name: String },
    Done { ok: usize, failed: usize },
    Failed(String),
}

pub struct State {
    pub phase: Phase,
    pub mode: Mode,
    pub files: Vec<category::FileInfo>,
    pub targets: Vec<engines::Target>,
    pub actions: Vec<Action>,
    pub tools: Vec<engines::Target>,
    pub hover: Option<usize>,
}

impl State {
    /// Number of sectors on the active page.
    pub fn page_len(&self) -> usize {
        match self.mode {
            Mode::Convert => self.targets.len(),
            Mode::Actions => self.actions.len(),
            Mode::Tools => self.tools.len(),
        }
    }

    /// Waiting *and* Ready accept a new drop: a second drag should replace the
    /// selection instead of being thrown away.
    pub fn accepts_drop(&self) -> bool {
        matches!(self.phase, Phase::Waiting | Phase::Ready)
    }
}

pub struct App {
    pub app: gtk4::Application,
    pub window: gtk4::Window,
    pub fixed: gtk4::Fixed,
    pub da: WheelArea,
    pub center: gtk4::Box,
    pub center_title: gtk4::Label,
    pub center_hint: gtk4::Label,
    /// The hint to show when nothing is hovered (hover shows a preview instead).
    pub base_hint: String,
    pub labels: Vec<gtk4::Label>,
    pub state: State,
    pub cfg: Config,
    pub theme: Theme,
    pub scene: Rc<RefCell<Scene>>,
    pub last_scene: Scene,
    /// A drag is currently over the dial.
    pub dragging: bool,
    /// The drag reached the dial itself, not just the standby strip.
    pub takeover: bool,
    /// The wheel is mapped and visible.
    pub visible: bool,
    /// The wheel is a layer-shell surface rather than a toplevel.
    pub layered: bool,
    /// Preview of the dropped file in the centre well.
    pub thumb: gtk4::Image,
    /// Exit after a job, or retract and wait for the next drag.
    pub lifecycle: Lifecycle,
    /// Bumped by every reveal/retract so a stale fade cannot keep running.
    pub anim: Cell<u32>,
}

pub type AppRef = Rc<RefCell<App>>;

/// What happens once the wheel is done with its work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lifecycle {
    /// Hotkey mode: the process exists for one interaction and exits after it.
    Exit,
    /// Resident mode: unmap the wheel and wait for the next drag.
    Retract,
}

/// How the wheel starts life. Resident mode builds it unmapped so the first
/// drag only has to map and park it.
pub enum Start {
    Shown {
        x: Option<i32>,
        y: Option<i32>,
        files: Vec<PathBuf>,
    },
    Hidden,
}

/// Entry for hotkey / file-manager mode: build and show the wheel at the cursor.
pub fn build(app: &gtk4::Application, x: Option<i32>, y: Option<i32>, files: Vec<PathBuf>) {
    build_with(app, Start::Shown { x, y, files }, Lifecycle::Exit);
}

/// Entry for resident mode: build the wheel, but leave it unmapped until a drag
/// reaches the standby strip.
pub fn build_hidden(app: &gtk4::Application) -> AppRef {
    build_with(app, Start::Hidden, Lifecycle::Retract)
}

fn build_with(app: &gtk4::Application, start: Start, lifecycle: Lifecycle) -> AppRef {
    let hidden = matches!(start, Start::Hidden);
    let cfg = crate::config::load();
    let theme = theming::load();

    // Pre-warm engine capabilities on a background thread. The first call to
    // `capabilities()` probes ffmpeg, ImageMagick, poppler, etc. via subprocess,
    // which would block the GTK main render thread if done lazily during load().
    // By the time a user drops files, the OnceLock is already populated.
    let _ = std::thread::spawn(|| {
        let _ = crate::engines::capabilities();
    });

    // The surface is larger than the disc: the extra ring is transparent and
    // exists only so the drop shadow can breathe.
    let size = wheel::surface_size(cfg.wheel.size);

    let scene = Rc::new(RefCell::new(Scene {
        sectors: 0,
        hover: None,
        waiting: true,
        size: size as f64,
        inner_frac: cfg.wheel.inner_radius,
        gap_deg: cfg.wheel.sector_gap_deg,
        theme,
        progress: None,
        page: (0, 2),
        armed: false,
    }));

    let (tx_poller, rx) = channel::<Msg>();
    let tx = tx_poller.clone();
    POLLER_TX.with(|cell| *cell.borrow_mut() = Some(tx.clone()));

    let da = WheelArea::new(scene.clone(), size);
    let fixed = gtk4::Fixed::new();
    fixed.put(&da, 0.0, 0.0);

    // Center stack: what the dial is holding. Tangerine previews the file here
    // (thumbnail plus facts) instead of printing an instruction, because the
    // only question the user has mid-drag is "did it pick up the right file?".
    let center = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    center.set_halign(gtk4::Align::Center);
    center.set_valign(gtk4::Align::Center);
    center.set_can_target(false);
    center.set_size_request(CENTER_W as i32, CENTER_H as i32);
    let thumb = gtk4::Image::new();
    thumb.add_css_class("thumb");
    thumb.set_pixel_size(52);
    thumb.set_can_target(false);
    thumb.set_visible(false);
    center.append(&thumb);
    let center_title = gtk4::Label::new(None);
    center_title.add_css_class("title");
    center_title.set_justify(gtk4::Justification::Center);
    center_title.set_single_line_mode(true);
    center_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    center_title.set_max_width_chars(24);
    center_title.set_can_target(false);
    let center_hint = gtk4::Label::new(None);
    center_hint.add_css_class("hint");
    center_hint.set_justify(gtk4::Justification::Center);
    center_hint.set_single_line_mode(true);
    center_hint.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    center_hint.set_max_width_chars(30);
    center_hint.set_can_target(false);
    center.append(&center_title);
    center.append(&center_hint);
    fixed.put(&center, (size as f64 - CENTER_W) / 2.0, (size as f64 - CENTER_H) / 2.0 - 3.0);

    let window = gtk4::Window::new();
    window.set_application(Some(app));
    window.set_resizable(false);
    window.add_css_class("mandarine-window");
    // The wheel + labels live in the Fixed; without this the surface maps at
    // GTK's 200x200 default with nothing painted in it.
    window.set_child(Some(&fixed));

    // A layer surface, not a window.
    //
    // The dial used to be a toplevel that Hyprland floated and moved by IPC.
    // That made it a real window: it entered the float/focus machinery, needed
    // window rules to stay borderless, and every reveal was three `hyprctl`
    // dispatches racing the drag. As an OVERLAY layer surface it sits above all
    // windows without being one, needs no rules to stay unmanaged, and is
    // placed by anchors plus margins. The same surface type already carries
    // pointer input on this compositor, so it is a valid drop target.
    let layered = gtk4_layer_shell::is_supported();
    if layered {
        window.init_layer_shell();
        window.set_namespace(Some(LAYER_NAMESPACE));
        window.set_layer(Layer::Overlay);
        // Arrows, Tab, Enter and Esc are the keyboard path through the wheel,
        // and it is summoned deliberately: it owns the keyboard while it is up.
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        // Zero = shrink no window to make room for a thing that is not resident.
        window.set_exclusive_zone(0);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_default_size(size, size);
    } else {
        window.set_decorated(false);
        window.set_default_size(size, size);
    }
    apply_css(&window, &theme);

    let state = State { phase: Phase::Waiting, mode: Mode::default(), files: vec![], targets: vec![], actions: vec![], tools: vec![], hover: None };
    let app_state: AppRef = Rc::new(RefCell::new(App {
        app: app.clone(),
        window: window.clone(),
        fixed,
        da: da.clone(),
        center,
        center_title,
        center_hint,
        base_hint: String::new(),
        labels: vec![],
        state,
        cfg: cfg.clone(),
        theme,
        scene,
        last_scene: Scene::default(),
        dragging: false,
        takeover: false,
        visible: !hidden,
        layered,
        thumb: thumb.clone(),
        lifecycle,
        anim: Cell::new(0),
    }));
    set_center(
        &mut app_state.borrow_mut(),
        "Drop to convert",
        IDLE_HINT,
    );

    refresh_preview(&mut app_state.borrow_mut());

    attach_motion(&da, app_state.clone());
    attach_scroll(&da, app_state.clone());
    attach_click(&da, app_state.clone());
    attach_keys(&window, app_state.clone());
    drop::attach(da.upcast_ref(), tx.clone());
    // A resident wheel only ever appears because of a drag; guessing from the
    // clipboard there would pop it up for no reason. `clipboard_fallback_secs = 0`
    // is the documented opt-out and drop.rs handles it too.
    if !hidden && cfg.clipboard_fallback_secs > 0 {
        drop::clipboard_fallback(Duration::from_secs(cfg.clipboard_fallback_secs), tx.clone());
    }

    let poll_state = app_state.clone();
    glib::timeout_add_local(Duration::from_millis(120), move || {
        loop {
            match rx.try_recv() {
                Ok(msg) => handle_msg(&poll_state, msg),
                Err(_) => break,
            }
        }
        sync_scene(&poll_state);
        glib::ControlFlow::Continue
    });

    if !layered && crate::platform::session() == crate::platform::Session::Hyprland {
        ensure_decorless();
    }

    let (start_files, cursor) = match start {
        Start::Shown { x, y, files } => {
            if layered {
                // Anchors and margins are enough: the surface is placed before
                // it is mapped, so there is no window to float afterwards.
                (files, Some(cursor_pos_or(x, y)))
            } else {
                (files, Some(place_window(&window, size, x, y)))
            }
        }
        Start::Hidden => {
            // Same geometry as a shown wheel, so revealing it is a map and not a
            // resize, and the compositor can place it the moment it maps.
            window.set_default_size(size, size);
            window.set_decorated(false);
            (Vec::new(), None)
        }
    };

    if let Some((cx, cy)) = cursor {
        if layered {
            place_layer(&window, size, cx, cy);
        }
        window.present();
        drop::log_dnd(&format!(
            "--- open at {cx},{cy} size={size} files={}",
            start_files.len()
        ));
        if !layered {
            present_at(&app_state, size, cx, cy, false);
        }
        if !start_files.is_empty() {
            handle_msg(&app_state, Msg::Files(start_files));
        }
    }

    app_state
}

/// Park a freshly mapped wheel at (cx, cy).
///
/// A plain toplevel, moved by Hyprland itself. Two things depend on the order
/// here: the window has to be mapped before it can be floated and moved, and an
/// in-flight file drag only follows the pointer into a surface that already
/// exists — hence the pointer nudge afterwards.
///
/// `urgent` is for the drag-to-open path, where the drag is definitely still in
/// flight and every extra millisecond is a millisecond the user spends dragging
/// at a dial that has not accepted it yet.
fn present_at(st: &AppRef, size: i32, cx: i32, cy: i32, urgent: bool) {
    let st = st.clone();
    glib::timeout_add_local_once(Duration::from_millis(24), move || {
        let (left, top) = wheel_origin(cx, cy, size, monitor_geometry(cx, cy));
        if crate::platform::session() == crate::platform::Session::Hyprland {
            park_window(size, left, top);
        } else {
            // X11 and Win32 toplevels can place themselves through GDK.
            crate::platform::place_toplevel(&st.borrow().window, left, top);
        }
    });
    let (first, second) = if urgent { (80, 220) } else { (240, 460) };
    if crate::platform::session() == crate::platform::Session::Hyprland {
        glib::timeout_add_local_once(Duration::from_millis(first), nudge_pointer);
        glib::timeout_add_local_once(Duration::from_millis(second), nudge_pointer);
    }
}

// --- resident mode: reveal / retract -------------------------------------

/// Show the already-built wheel at the cursor.
///
/// Idempotent: a drag that re-enters the strip while the wheel is up does
/// nothing. The state is reset first, so a dial left over from a finished
/// conversion never shows stale sectors.
pub fn reveal(st: &AppRef, cx: i32, cy: i32) {
    if st.borrow().visible {
        return;
    }
    {
        let mut a = st.borrow_mut();
        a.anim.set(a.anim.get() + 1); // cancel a retract that is fading out
        a.visible = true;
        a.takeover = false;
        // The drag that triggered this is still in flight, so the dial is armed
        // from the first frame: "will it take my file?" is the only question
        // the user has at this moment.
        a.dragging = true;
        a.state = State {
            phase: Phase::Waiting,
            mode: Mode::default(),
            files: vec![],
            targets: vec![],
            actions: vec![],
            tools: vec![],
            hover: None,
        };
        a.fixed.set_opacity(1.0);
        set_center(&mut a, "Drop to convert", HOVER_HINT);
    }
    relabel(st);
    let (size, da) = {
        let a = st.borrow();
        (a.scene.borrow().size as i32, a.da.clone())
    };
    da.restart_entrance();
    {
        let a = st.borrow();
        if a.layered {
            place_layer(&a.window, size, cx, cy);
        }
        a.window.present();
    }
    drop::log_dnd(&format!("reveal at {cx},{cy} size={size}"));
    if !st.borrow().layered {
        present_at(st, size, cx, cy, true);
    }
    sync_scene(st);
}

/// Fade the dial out, then unmap it and go back to standby.
///
/// Called when the drag leaves or is cancelled without a drop, and after a job
/// finishes in resident mode.
pub fn retract(st: &AppRef) {
    if !st.borrow().visible {
        return;
    }
    let generation = {
        let a = st.borrow();
        a.anim.set(a.anim.get() + 1);
        a.anim.get()
    };
    fade_step(st, generation, 0);
}

/// 12 × 16 ms ≈ 190 ms: long enough to read as a retract, short enough not to
/// sit in front of the next drag.
const FADE_STEPS: u32 = 12;

fn fade_step(st: &AppRef, generation: u32, step: u32) {
    let st2 = st.clone();
    glib::timeout_add_local_once(Duration::from_millis(16), move || {
        let done = {
            let a = st2.borrow();
            // A reveal during the fade bumps `anim`; abandon this animation
            // rather than fight it.
            if a.anim.get() != generation || !a.visible {
                return;
            }
            let t = (step + 1) as f64 / FADE_STEPS as f64;
            a.fixed.set_opacity((1.0 - t).clamp(0.0, 1.0));
            step + 1 >= FADE_STEPS
        };
        if done {
            hide(&st2);
        } else {
            fade_step(&st2, generation, step + 1);
        }
    });
}

fn hide(st: &AppRef) {
    {
        let mut a = st.borrow_mut();
        a.window.set_visible(false);
        a.visible = false;
        a.dragging = false;
        a.takeover = false;
        a.fixed.set_opacity(1.0);
        a.state = State {
            phase: Phase::Waiting,
            mode: Mode::default(),
            files: vec![],
            targets: vec![],
            actions: vec![],
            tools: vec![],
            hover: None,
        };
        for label in std::mem::take(&mut a.labels) {
            a.fixed.remove(&label);
        }
        set_center(&mut a, "Drop to convert", IDLE_HINT);
    }
    sync_scene(st);
    drop::log_dnd("wheel retracted");
}

/// The drag left the standby strip.
///
/// Usually the pointer is about to be re-routed into the wheel (`present_at`
/// nudges it, which only takes effect on the next motion), so give the hand-off
/// a window before deciding the user simply dragged away.
pub fn strip_left(st: &AppRef) {
    let (visible, takeover) = {
        let a = st.borrow();
        (a.visible, a.takeover)
    };
    if !visible || takeover {
        return;
    }
    schedule_retract(st, 450);
}

fn schedule_retract(st: &AppRef, ms: u64) {
    let st2 = st.clone();
    let generation = {
        let a = st.borrow();
        a.anim.set(a.anim.get() + 1);
        a.anim.get()
    };
    glib::timeout_add_local_once(Duration::from_millis(ms), move || {
        if st2.borrow().anim.get() == generation {
            retract_if_idle(&st2);
        }
    });
}

/// Retract only if the drag is not on the dial and nothing was loaded.
///
/// The strip's leave and the dial's enter arrive in that order, so the grace
/// timer armed by the former must not fire once the latter has taken over.
fn retract_if_idle(st: &AppRef) {
    let idle = {
        let a = st.borrow();
        a.visible && !a.takeover && a.state.files.is_empty()
    };
    if idle {
        retract(st);
    }
}

/// Files that arrived on the strip's own surface (the user dropped without
/// waiting for the wheel to be routed the drag).
pub(crate) fn deliver_files(st: &AppRef, paths: Vec<PathBuf>) {
    handle_msg(st, Msg::Files(paths));
}

/// Hyprland frames every toplevel with its border, and a border around a dial
/// that is otherwise a disc of glass is the one thing that makes it look like a
/// window. There is no per-window dispatch for that, so register a rule once per
/// compositor session — not once per launch, which would pile rules up in the
/// running config.
fn ensure_decorless() {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_else(|_| "default".into());
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let marker = dir.join(format!("mandarine-rule-{sig}"));
    if marker.exists() {
        return;
    }
    let _ = std::process::Command::new("hyprctl")
        .args([
            "eval",
            "hl.window_rule({ match = { class = \"^(io.sahil.mandarine)$\" }, decorate = false })",
        ])
        .output();
    let _ = std::fs::write(marker, b"");
}

/// Float, size and move the wheel through Hyprland.
///
/// Only used on the fallback path, when the compositor has no
/// `wlr-layer-shell`: a toplevel has no way to place itself on Wayland, so the
/// compositor has to do it by IPC. The layer-shell path in [`place_layer`]
/// needs none of this.
fn park_window(size: i32, left: i32, top: i32) {
    // Hyprland 0.56 dispatches are Lua: each call is one dispatcher expression.
    for call in [
        "hl.dsp.window.float({})".to_string(),
        format!("hl.dsp.window.resize({{ x = {size}, y = {size} }})"),
        format!("hl.dsp.window.move({{ x = {left}, y = {top} }})"),
    ] {
        let _ = std::process::Command::new("hyprctl")
            .args(["dispatch", &call])
            .output();
    }
}

/// Warp the pointer by one pixel and back, so an in-flight drag is re-routed
/// to this window: the compositor only re-targets a drag on motion. Hyprland-only
/// (it is the compositor this app targets); a failure to warp is harmless.
///
/// The pixel is taken from the pointer's position *now*, never from where the
/// dial opened: the user is already moving toward a sector by the time this
/// runs, and warping back to the opening point would yank the cursor under them.
fn nudge_pointer() {
    fn warp(x: i32, y: i32) {
        let _ = std::process::Command::new("hyprctl")
            .args([
                "eval",
                &format!("hl.dispatch(hl.dsp.cursor.move({{x = {x}, y = {y}}}))"),
            ])
            .output();
    }
    let (x, y) = cursor_pos();
    // Never trust a failed probe: (0,0) is the top-left corner, and hopping
    // there would yank the pointer off the dial.
    if x == 0 && y == 0 {
        return;
    }
    warp(x + 1, y);
    glib::timeout_add_local_once(Duration::from_millis(60), move || warp(x, y));
}

fn handle_msg(st: &AppRef, msg: Msg) {
    match msg {
        Msg::Files(paths) => {
            drop::log_dnd(&format!("msg files n={}", paths.len()));
            if st.borrow().state.accepts_drop() {
                load(st, paths);
            }
        }
        Msg::DragHover(over) => {
            let (changed, leave_without_files) = {
                let mut a = st.borrow_mut();
                if !a.state.accepts_drop() {
                    (false, false)
                } else {
                    let waiting = matches!(a.state.phase, Phase::Waiting);
                    a.dragging = over;
                    // The drag reached the dial itself, not just the strip: the
                    // hand-off worked, so the strip's grace timer must not
                    // retract this. Leaving the dial clears it again.
                    a.takeover = over;
                    let title = if waiting {
                        "Drop to convert".to_string()
                    } else {
                        file_summary(&a.state.files)
                    };
                    let hint = if over {
                        HOVER_HINT
                    } else if waiting {
                        IDLE_HINT
                    } else {
                        mode_hint(a.state.mode)
                    };
                    set_center(&mut a, &title, hint);
                    let retract = !over
                        && a.lifecycle == Lifecycle::Retract
                        && a.state.files.is_empty();
                    (true, retract)
                }
            };
            if changed {
                sync_scene(st);
            }
            if leave_without_files {
                // Dragged off the dial, or cancelled with Esc, without dropping:
                // return to standby instead of lingering over the desktop.
                schedule_retract(st, 120);
            }
        }
        Msg::Clipboard(paths) => {
            if !matches!(st.borrow().state.phase, Phase::Waiting) {
                return;
            }
            if paths.is_empty() {
                let mut a = st.borrow_mut();
                set_center(&mut a, "Drop to convert", IDLE_HINT);
            } else {
                load(st, paths);
            }
        }
        Msg::TargetsLoaded {
            paths: infos,
            targets,
            actions,
            tools,
        } => {
            let mut a = st.borrow_mut();
            if !matches!(a.state.phase, Phase::Waiting | Phase::Ready) {
                return;
            }
            if infos.is_empty() && targets.is_empty() && actions.is_empty() && tools.is_empty() {
                a.state.phase = Phase::Failed("Nothing available for these files".into());
                set_center(&mut a, "Nothing to do here", "No targets for these files");
                schedule_quit(st, 2200);
                return;
            }
            a.dragging = false;
            a.state.files = infos;
            a.state.targets = targets;
            a.state.actions = actions;
            a.state.tools = tools;
            a.state.mode = if !a.state.targets.is_empty() {
                Mode::Convert
            } else if !a.state.actions.is_empty() {
                Mode::Actions
            } else {
                Mode::Tools
            };
            a.state.phase = Phase::Ready;
            let hint = mode_hint(a.state.mode);
            let title = file_summary(&a.state.files);
            set_center(&mut a, &title, hint);
            drop(a);
            relabel(st);
            sync_scene(st);
        }
        Msg::Progress(i, n, name) => {
            let mut a = st.borrow_mut();
            a.state.phase = Phase::Running { current: i, total: n, name: name.clone() };
            let title = format!("Converting {} of {}", (i + 1).min(n), n);
            set_center(&mut a, &title, &truncate(&file_name(&name), 28));
        }
        Msg::Finished(result) => {
            let ok = result.ok_count();
            let failed = result.failed_count();
            {
                let mut a = st.borrow_mut();
                a.state.phase = Phase::Done { ok, failed };
                a.state.hover = None;
                let title = if failed > 0 { "Finished with errors" } else { "Done" };
                let hint = if failed > 0 {
                    format!("{ok} converted · {failed} failed")
                } else {
                    format!("{ok} file{} converted", if ok == 1 { "" } else { "s" })
                };
                set_center(&mut a, title, &hint);
            }
            notify::summarize_with_actions(st.borrow().cfg.notify, &result);
            schedule_quit(st, 1500);
        }
        Msg::ActionDone(result) => {
            let (ok, failed) = if result.is_ok() { (1, 0) } else { (0, 1) };
            let (title, hint) = match &result {
                Ok(note) => (note.clone(), String::new()),
                Err(e) => ("Action failed".to_string(), truncate(e, 34)),
            };
            {
                let mut a = st.borrow_mut();
                a.state.phase = Phase::Done { ok, failed };
                a.state.hover = None;
                set_center(&mut a, &title, &hint);
            }
            let notify_on = st.borrow().cfg.notify;
            notify::send(notify_on, "Mandarine", if hint.is_empty() { &title } else { &hint });
            schedule_quit(st, 1200);
        }
    }
}

fn load(st: &AppRef, paths: Vec<PathBuf>) {
    let mut paths = paths;
    for p in paths.iter_mut() {
        // File-manager integrations pass `file://` URIs as often as paths.
        if let Some(s) = p.to_str() {
            if s.starts_with("file://") {
                if let Some(real) = gtk4::gio::File::for_uri(s).path() {
                    *p = real;
                    continue;
                }
            }
        }
        if !p.is_absolute() {
            *p = std::env::current_dir().unwrap_or_default().join(&*p);
        }
    }
    paths.retain(|p| p.exists());
    if paths.is_empty() {
        let mut a = st.borrow_mut();
        set_center(&mut a, "No valid files", "Try dropping a file from Files");
        return;
    }

    // Offload capability probing, category detection, target resolution and
    // action gating to a background thread. The GTK main thread must stay free
    // for rendering and drag animations. Results come back via Msg::TargetsLoaded.
    let tx = POLLER_TX.with(|c| c.borrow().clone());
    let Some(tx) = tx else { return };
    let st2 = st.clone();
    let cfg = st.borrow().cfg.clone();

    std::thread::spawn(move || {
        let caps = crate::engines::capabilities().clone();
        let infos = crate::category::detect(&paths);
        let targets = crate::engines::targets_for(&infos, &caps, &cfg);
        let acts = crate::actions::available(&infos, &caps);
        let tools = crate::engines::tools_for(&infos, &caps);
        let _ = tx.send(Msg::TargetsLoaded {
            paths: infos,
            targets,
            actions: acts,
            tools,
        });
        let _ = st2; // keeps Rc alive until the thread ends
    });
}

/// Rebuild the sector labels for the active page.
fn relabel(st: &AppRef) {
    let mut a = st.borrow_mut();
    let old = std::mem::take(&mut a.labels);
    for l in old {
        a.fixed.remove(&l);
    }
    if !matches!(a.state.phase, Phase::Ready) {
        return;
    }
    let size = a.scene.borrow().size;
    let scene = Scene {
        sectors: a.state.page_len(),
        hover: None,
        waiting: false,
        size,
        inner_frac: a.cfg.wheel.inner_radius,
        gap_deg: a.cfg.wheel.sector_gap_deg,
        theme: a.theme,
        progress: None,
        page: (a.state.mode as usize, 3),
        armed: a.dragging,
    };
    let labels: Vec<String> = match a.state.mode {
        Mode::Convert => a.state.targets.iter().map(|t| t.label.clone()).collect(),
        Mode::Actions => a.state.actions.iter().map(|x| x.label().to_string()).collect(),
        Mode::Tools => a.state.tools.iter().map(|t| t.label.clone()).collect(),
    };
    for (i, text) in labels.iter().enumerate() {
        let l = gtk4::Label::new(Some(text));
        l.add_css_class("sector");
        l.set_can_target(false);
        l.set_justify(gtk4::Justification::Center);
        l.set_single_line_mode(true);
        l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        l.set_size_request(112, 26);
        let (lx, ly) = wheel::label_anchor(&scene, i);
        a.fixed.put(&l, lx - 56.0, ly - 13.0);
        a.labels.push(l);
    }
}

fn set_mode(st: &AppRef, mode: Mode) {
    let changed = {
        let mut a = st.borrow_mut();
        if !matches!(a.state.phase, Phase::Ready) || a.state.mode == mode {
            false
        } else {
            a.state.mode = mode;
            a.state.hover = None;
            let hint = mode_hint(mode);
            let title = file_summary(&a.state.files);
            set_center(&mut a, &title, hint);
            true
        }
    };
    if changed {
        relabel(st);
        sync_scene(st);
    }
}

fn activate(st: &AppRef, idx: usize) {
    let (mode, files, target, action, cfg) = {
        let mut a = st.borrow_mut();
        if !matches!(a.state.phase, Phase::Ready) || idx >= a.state.page_len() {
            return;
        }
        a.state.hover = None;
        for l in &a.labels {
            l.remove_css_class("hot");
        }
        let files = a.state.files.clone();
        let cfg = a.cfg.clone();
        let mode = a.state.mode;
        let target = match mode {
            Mode::Tools => a.state.tools.get(idx).cloned(),
            _ => a.state.targets.get(idx).cloned(),
        };
        let action = a.state.actions.get(idx).copied();
        let name = match mode {
            Mode::Actions => action.map(|x| x.label().to_string()).unwrap_or_default(),
            _ => target.as_ref().map(|t| t.label.clone()).unwrap_or_default(),
        };
        a.state.phase = Phase::Running { current: 0, total: files.len(), name: name.clone() };
        let hint = format!("{} item{}", files.len(), if files.len() == 1 { "" } else { "s" });
        set_center(&mut a, &name, &hint);
        (mode, files, target, action, cfg)
    };
    match (mode, target, action) {
        (Mode::Convert | Mode::Tools, Some(target), _) => {
            notify::job_started(st.borrow().cfg.notify, files.len());
            start_conversion(st, files, target, cfg);
        }
        (Mode::Actions, _, Some(action)) => run_action(files, action, cfg),
        _ => {}
    }
}

fn run_action(files: Vec<category::FileInfo>, action: Action, cfg: Config) {
    let tx = POLLER_TX.with(|c| c.borrow().clone());
    let Some(tx) = tx else { return };
    std::thread::spawn(move || {
        let caps = engines::capabilities().clone();
        let result = actions::run(action, &files, &cfg, &caps, &crate::platform::clipboard_copy)
            .map_err(|e| format!("{e:#}"));
        let _ = tx.send(Msg::ActionDone(result));
    });
}

fn start_conversion(st: &AppRef, files: Vec<category::FileInfo>, target: engines::Target, cfg: Config) {
    // The thread feeds progress/results into the poller channel registered at
    // build time (see POLLER_TX below).
    let tx = POLLER_TX.with(|c| c.borrow().clone());
    let Some(tx) = tx else { return };
    let st2 = st.clone();
    std::thread::spawn(move || {
        let caps = engines::capabilities().clone();
        let tx_progress = tx.clone();
        let progress = move |i: usize, n: usize, p: &std::path::Path| {
            let _ = tx_progress.send(Msg::Progress(i, n, p.display().to_string()));
        };
        let result = match convert::dispatch(&files, &target.id, Some(target.kind), &cfg, &caps, &progress) {
            Ok(r) => r,
            Err(e) => convert::BatchResult {
                outcomes: files
                    .iter()
                    .map(|f| convert::JobOutcome {
                        input: f.path.clone(),
                        outputs: vec![],
                        error: Some(format!("{e:#}")),
                    })
                    .collect(),
            },
        };
        let _ = tx.send(Msg::Finished(result));
        let _ = st2; // keeps Rc alive until the thread ends
    });
}

fn sync_scene(st: &AppRef) {
    let mut a = st.borrow_mut();
    let page = (a.state.mode as usize, 2);
    let (sectors, hover, waiting, progress) = match &a.state.phase {
        Phase::Waiting => (0, None, true, None),
        Phase::Ready => (a.state.page_len(), a.state.hover, false, None),
        Phase::Running { current, total, .. } => (
            a.state.page_len(),
            None,
            false,
            Some((*current as f64 / (*total).max(1) as f64).clamp(0.0, 1.0)),
        ),
        Phase::Done { .. } => (a.state.page_len(), None, false, Some(1.0)),
        Phase::Failed(_) => (0, None, false, None),
    };
    let size = a.scene.borrow().size;
    let next = Scene {
        sectors,
        hover,
        waiting,
        size,
        inner_frac: a.cfg.wheel.inner_radius,
        gap_deg: a.cfg.wheel.sector_gap_deg,
        theme: a.theme,
        progress,
        page,
        armed: a.dragging && matches!(a.state.phase, Phase::Waiting | Phase::Ready),
    };
    if next != a.last_scene {
        a.last_scene = next;
        *a.scene.borrow_mut() = next;
        a.da.queue_draw();
    }
}

fn update_hover_at(st: &AppRef, x: f64, y: f64) {
    let idx = {
        let a = st.borrow();
        if !matches!(a.state.phase, Phase::Ready) {
            None
        } else {
            wheel::hit_test(&*a.scene.borrow(), x, y)
        }
    };
    set_hover(st, idx);
    st.borrow()
        .da
        .set_cursor_from_name(if idx.is_some() { Some("pointer") } else { None });
}

fn attach_motion(da: &WheelArea, st: AppRef) {
    let m = gtk4::EventControllerMotion::new();
    let st_move = st.clone();
    m.connect_motion(move |_, x, y| update_hover_at(&st_move, x, y));
    // The wheel usually opens *under* the cursor, so the first sector is often
    // resolved by an enter with no motion behind it.
    let st_enter = st.clone();
    m.connect_enter(move |_, x, y| update_hover_at(&st_enter, x, y));
    let st_leave = st;
    m.connect_leave(move |_| {
        set_hover(&st_leave, None);
        st_leave.borrow().da.set_cursor_from_name(None);
    });
    da.add_controller(m);
}

/// Wheel/touchpad scrolling walks the sectors, which is how people expect a
/// dial to behave without hunting for the keys.
fn attach_scroll(da: &WheelArea, st: AppRef) {
    let s = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
    s.connect_scroll(move |_, _, dy| {
        if matches!(st.borrow().state.phase, Phase::Ready) && dy != 0.0 {
            cycle_hover(&st, if dy > 0.0 { 1 } else { -1 });
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    da.add_controller(s);
}

fn attach_click(da: &WheelArea, st: AppRef) {
    let g = gtk4::GestureClick::new();
    let st_click = st.clone();
    g.connect_pressed(move |_, _, x, y| {
        let idx = {
            let a = st_click.borrow();
            if !matches!(a.state.phase, Phase::Ready) {
                None
            } else {
                wheel::hit_test(&*a.scene.borrow(), x, y)
            }
        };
        if let Some(i) = idx {
            activate(&st_click, i);
        }
    });
    da.add_controller(g);
}

fn attach_keys(window: &gtk4::Window, st: AppRef) {
    let k = gtk4::EventControllerKey::new();
    k.connect_key_pressed(move |_, key, _, state| {
        use gdk::Key;
        // Tangerine's pages: Shift = conversions, Control = actions. (Meta is the
        // window manager's key on Hyprland, so it cannot be a page switch here.)
        if state.contains(gdk::ModifierType::CONTROL_MASK) {
            set_mode(&st, Mode::Actions);
        } else if state.contains(gdk::ModifierType::ALT_MASK) {
            set_mode(&st, Mode::Tools);
        } else if state.contains(gdk::ModifierType::SHIFT_MASK) {
            set_mode(&st, Mode::Convert);
        }
        match key {
            Key::Escape => {
                quit(&st);
                glib::Propagation::Stop
            }
            Key::Tab => {
                let next = match st.borrow().state.mode {
                    Mode::Convert => Mode::Actions,
                    Mode::Actions => Mode::Tools,
                    Mode::Tools => Mode::Convert,
                };
                set_mode(&st, next);
                glib::Propagation::Stop
            }
            Key::Right | Key::Down => {
                cycle_hover(&st, 1);
                glib::Propagation::Stop
            }
            Key::Left | Key::Up => {
                cycle_hover(&st, -1);
                glib::Propagation::Stop
            }
            Key::Return | Key::KP_Enter => {
                // Only a sector that is actually lit: Enter over the centre
                // well used to fire the first sector, which is never what the
                // pointer was pointing at.
                let idx = {
                    let a = st.borrow();
                    if matches!(a.state.phase, Phase::Ready) {
                        a.state.hover
                    } else {
                        None
                    }
                };
                if let Some(i) = idx {
                    activate(&st, i);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            _ => {
                let name = key.name().unwrap_or_default();
                let digit = name.chars().next().and_then(|c| c.to_digit(10)).filter(|n| (1..=9).contains(n));
                if let Some(n) = digit {
                    activate(&st, (n - 1) as usize);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        }
    });
    window.add_controller(k);
}

fn quit(st: &AppRef) {
    let (app, lifecycle) = {
        let a = st.borrow();
        (a.app.clone(), a.lifecycle)
    };
    match lifecycle {
        Lifecycle::Exit => app.quit(),
        Lifecycle::Retract => retract(st),
    }
}

fn schedule_quit(st: &AppRef, ms: u64) {
    let st2 = st.clone();
    let (app, lifecycle) = {
        let a = st.borrow();
        (a.app.clone(), a.lifecycle)
    };
    glib::timeout_add_local_once(Duration::from_millis(ms), move || match lifecycle {
        Lifecycle::Exit => app.quit(),
        Lifecycle::Retract => retract(&st2),
    });
}

fn file_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Two-line center readout. `hint` is remembered so a hover preview can be
/// undone when the pointer leaves a sector.
fn set_center(a: &mut App, title: &str, hint: &str) {
    a.center_title.set_text(title);
    a.center_hint.set_text(hint);
    a.base_hint.clear();
    a.base_hint.push_str(hint);
    refresh_preview(a);
}

/// Load the dropped file into the centre well.
///
/// Images decode to a texture so the dial shows the actual picture; everything
/// else gets the same file-type icon the desktop would use. A multi-file
/// selection has no single thumbnail, so the well stays text-only there.
fn refresh_preview(a: &mut App) {
    let single = if a.state.files.len() == 1 {
        a.state.files.first()
    } else {
        None
    };
    let Some(info) = single else {
        a.thumb.set_visible(false);
        return;
    };
    match gdk::Texture::from_filename(&info.path) {
        Ok(texture) => {
            a.thumb.set_paintable(Some(&texture));
        }
        Err(_) => {
            let (ctype, _) = gtk4::gio::content_type_guess(Some(&info.path), None);
            a.thumb.set_paintable(gdk::Paintable::NONE);
            a.thumb.set_from_gicon(&gtk4::gio::content_type_get_icon(&ctype));
        }
    }
    a.thumb.set_visible(true);
}

/// `2.2 MB · PNG` — the facts Tangerine shows next to the preview.
fn facts(files: &[category::FileInfo]) -> String {
    let total: u64 = files
        .iter()
        .filter_map(|f| std::fs::metadata(&f.path).ok().map(|m| m.len()))
        .sum();
    let ext = |f: &category::FileInfo| {
        f.path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default()
    };
    match files {
        [] => String::new(),
        [one] => format!("{} · {}", human_size(total), ext(one)),
        many => format!("{} files · {}", many.len(), human_size(total)),
    }
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

fn mode_hint(mode: Mode) -> &'static str {
    match mode {
        Mode::Convert => "Shift convert · Tab actions",
        Mode::Actions => "Ctrl actions · Tab tools",
        Mode::Tools => "Alt tools · Tab convert",
    }
}

fn file_summary(files: &[category::FileInfo]) -> String {
    match files {
        [one] => one
            .path
            .file_stem()
            .map(|s| truncate(&s.to_string_lossy(), 24))
            .unwrap_or_else(|| "1 file".to_string()),
        many => format!("{} files", many.len()),
    }
}

/// What the center should read in the Ready phase, including the hover preview
/// (`→ photo.webp`) that makes the wheel feel predictive.
fn ready_text(a: &App) -> (String, String) {
    let title = file_summary(&a.state.files);
    let mut hint = if a.state.files.is_empty() {
        a.base_hint.clone()
    } else {
        facts(&a.state.files)
    };
    if matches!(a.state.mode, Mode::Convert | Mode::Tools) {
        let page = if a.state.mode == Mode::Tools { &a.state.tools } else { &a.state.targets };
        if let Some(t) = a.state.hover.and_then(|i| page.get(i)) {
            if t.kind == engines::TargetKind::Format {
                if let Some(f) = a.state.files.first() {
                    let stem = f
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    hint = format!("→ {stem}.{}", output_ext(&a.cfg, t));
                }
            } else {
                hint = format!("→ {}", t.label);
            }
        }
    }
    (title, hint)
}

/// The extension the file will actually carry. `targz` is a target id, not an
/// extension, and a custom target's id may be anything (`png90`), so the
/// preview used to promise `photo.targz`.
fn output_ext(cfg: &Config, target: &engines::Target) -> String {
    if let Some(c) = cfg.custom_targets.iter().find(|c| c.id == target.id) {
        return c.ext.clone();
    }
    match target.kind {
        engines::TargetKind::TarGz => "tar.gz".into(),
        _ => target.id.clone(),
    }
}

fn update_center(st: &AppRef) {
    let a = st.borrow();
    if !matches!(a.state.phase, Phase::Ready) {
        return;
    }
    let (title, hint) = ready_text(&a);
    a.center_title.set_text(&title);
    a.center_hint.set_text(&hint);
    // A preview is a promise about what will happen; tint it like one.
    if matches!(a.state.mode, Mode::Convert | Mode::Tools) && a.state.hover.is_some() {
        a.center_hint.add_css_class("preview");
    } else {
        a.center_hint.remove_css_class("preview");
    }
}

/// Single entry point for changing the highlight: scene, label tint and center
/// preview all move together.
fn set_hover(st: &AppRef, idx: Option<usize>) {
    let changed = {
        let mut a = st.borrow_mut();
        if a.state.hover == idx {
            false
        } else {
            a.state.hover = idx;
            true
        }
    };
    if !changed {
        return;
    }
    {
        let a = st.borrow();
        for (i, l) in a.labels.iter().enumerate() {
            if Some(i) == idx {
                l.add_css_class("hot");
            } else {
                l.remove_css_class("hot");
            }
        }
    }
    sync_scene(st);
    update_center(st);
}

fn cycle_hover(st: &AppRef, delta: isize) {
    let (len, cur) = {
        let a = st.borrow();
        if !matches!(a.state.phase, Phase::Ready) {
            return;
        }
        (a.state.page_len(), a.state.hover)
    };
    if len == 0 {
        return;
    }
    let next = match cur {
        None if delta >= 0 => 0,
        None => len - 1,
        Some(i) => (i as isize + delta).rem_euclid(len as isize) as usize,
    };
    set_hover(st, Some(next));
}

// The conversion threads need the poller channel; a thread-local set at build
// time keeps the plumbing out of every function signature.
thread_local! {
    static POLLER_TX: RefCell<Option<Sender<Msg>>> = const { RefCell::new(None) };
}

fn apply_css(window: &gtk4::Window, theme: &Theme) {
    // The surface itself is transparent: everything you see is painted by the
    // wheel snapshot, and Hyprland blurs the desktop through it.
    let sector = (theme.text.0, theme.text.1, theme.text.2, 0.90);
    let hint = (theme.text_dim.0, theme.text_dim.1, theme.text_dim.2, 0.78);
    let css = format!(
        ".mandarine-window {{ background-color: transparent; }}\n\
         /* SF Pro if it is installed, then Inter — which is the closest
            open UI grotesque to it and ships on most Linux boxes. The stack
            degrades to the GTK default without changing layout. */
         .mandarine-window .title {{\n\
           font-family: \"SF Pro Display\", \"Inter Display\", \"Inter\";\n\
           font-size: 13px; font-weight: 600; letter-spacing: -0.2px;\n\
           color: rgba({t}); text-shadow: 0 1px 2px rgba(0,0,0,0.45);\n\
         }}\n\
         .mandarine-window .hint {{\n\
           font-family: \"SF Pro Text\", \"Inter\", sans-serif;\n\
           font-size: 11px; font-weight: 500; letter-spacing: -0.1px;\n\
           color: rgba({d}); text-shadow: 0 1px 2px rgba(0,0,0,0.35);\n\
         }}\n\
         .mandarine-window .sector {{\n\
           font-family: \"SF Pro Text\", \"Inter\", sans-serif;\n\
           font-size: 12px; font-weight: 600; letter-spacing: -0.1px;\n\
           color: rgba({s}); text-shadow: 0 1px 3px rgba(0,0,0,0.50);\n\
         }}\n\
         .mandarine-window .sector.hot {{ color: #ffffff; font-weight: 700; }}\n\
         .mandarine-window .hint.preview {{ color: rgba({a}); }}\n",
        t = css_rgba(theme.text),
        d = css_rgba(hint),
        s = css_rgba(sector),
        a = css_rgba((theme.accent.0, theme.accent.1, theme.accent.2, 0.95)),
    );
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css);
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::RootExt::display(window),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // --- ML4W Pywal / Hyprland palette sync ---
    // Load ~/.cache/wal/colors-gtk.css (GTK-specific) or fall back to
    // ~/.cache/wal/colors.css (general Pywal output) as an additional
    // CssProvider so Pywal colors override our neutral tokens where the user
    // has chosen to theme their desktop.
    if let Some(pywal_css) = theming::load_pywal_css() {
        let pywal_provider = gtk4::CssProvider::new();
        pywal_provider.load_from_string(&pywal_css);
        gtk4::style_context_add_provider_for_display(
            &gtk4::prelude::RootExt::display(window),
            &pywal_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn css_rgba(c: theming::Rgba) -> String {
    format!(
        "{},{},{},{}",
        (c.0 * 255.0).round() as u32,
        (c.1 * 255.0).round() as u32,
        (c.2 * 255.0).round() as u32,
        c.3
    )
}

/// Anchor the dial's layer surface over (cx, cy).
///
/// A layer surface has no absolute position: it is anchored to an output corner
/// and offset by margins. Anchoring top-left and offsetting by the distance
/// from that output's origin is the equivalent of "put it at these coordinates",
/// and unlike a toplevel it needs no compositor IPC to do it.
fn place_layer(window: &gtk4::Window, size: i32, cx: i32, cy: i32) {
    if let Some(mon) = monitor_under_cursor() {
        window.set_monitor(Some(&mon));
    }
    window.set_default_size(size, size);
    let (mx, my, mw, mh) = monitor_geometry(cx, cy);
    let (left, top) = wheel_origin(cx, cy, size, (mx, my, mw, mh));
    window.set_margin(Edge::Left, left - mx);
    window.set_margin(Edge::Top, top - my);
    // Only the anchored edges are offset; the others must be zero or the
    // compositor treats the surface as stretched between them.
    window.set_margin(Edge::Right, 0);
    window.set_margin(Edge::Bottom, 0);
}

/// Where to open: the coordinates asked for, else the pointer.
fn cursor_pos_or(x: Option<i32>, y: Option<i32>) -> (i32, i32) {
    match (x, y) {
        (Some(x), Some(y)) => (x, y),
        _ => cursor_pos(),
    }
}

/// The output the pointer is on, or the first one — the layer surface has to be
/// pinned to an output for its margins to mean anything.
fn monitor_under_cursor() -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let (cx, cy) = cursor_pos();
    let monitors = display.monitors();
    let mut first = None;
    for i in 0..monitors.n_items() {
        let Some(mon) = monitors.item(i).and_then(|o| o.downcast::<gdk::Monitor>().ok()) else {
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

/// Size the surface and work out where its centre goes. The actual parking is
/// `park_window`, once the compositor has mapped the window.
fn place_window(window: &gtk4::Window, size: i32, x: Option<i32>, y: Option<i32>) -> (i32, i32) {
    // Without this GTK maps a 200x200 surface and the 496 px wheel is drawn
    // outside it.
    crate::platform::style_window(window, size);
    match (x, y) {
        (Some(x), Some(y)) => (x, y),
        _ => cursor_pos(),
    }
}

pub(crate) fn cursor_pos() -> (i32, i32) {
    if let Some(pos) = crate::platform::cursor_pos() {
        return pos;
    }
    if let Some(display) = gdk::Display::default() {
        let monitors = display.monitors();
        if monitors.n_items() > 0 {
            if let Some(mon) = monitors.item(0).and_then(|o| o.downcast::<gdk::Monitor>().ok()) {
                let g = mon.geometry();
                return (g.x() + g.width() / 2, g.y() + g.height() / 2);
            }
        }
    }
    (0, 0)
}

/// Top-left corner for a wheel centred on the cursor, kept inside `monitor`
/// (`x`, `y`, `w`, `h`).
///
/// Dragging into a screen edge is the normal way to arm the strip, so an
/// unclamped wheel would hang half off-screen exactly when it is needed most.
pub fn wheel_origin(cx: i32, cy: i32, size: i32, monitor: (i32, i32, i32, i32)) -> (i32, i32) {
    let (mx, my, mw, mh) = monitor;
    let axis = |v: i32, start: i32, extent: i32| {
        let end = start + extent - size;
        // A wheel wider than the output: pin it to the origin rather than let
        // `clamp` panic on an inverted range.
        if end < start {
            start
        } else {
            v.clamp(start, end)
        }
    };
    (axis(cx - size / 2, mx, mw), axis(cy - size / 2, my, mh))
}

/// The output containing (cx, cy), falling back to the first one.
///
/// `hyprctl` reports logical coordinates, the same space `cursor_pos` and
/// `park_window` work in. GDK is only the fallback for compositors without it.
pub(crate) fn monitor_geometry(cx: i32, cy: i32) -> (i32, i32, i32, i32) {
    crate::platform::monitor_geometry(cx, cy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_origin_centres_on_the_cursor() {
        assert_eq!(wheel_origin(960, 540, 496, (0, 0, 1920, 1080)), (712, 292));
    }

    #[test]
    fn wheel_origin_keeps_the_dial_on_screen() {
        let mon = (0, 0, 1920, 1080);
        // Dragged into the top edge: the strip's normal trigger.
        assert_eq!(wheel_origin(100, 2, 496, mon), (0, 0));
        // Dragged into the bottom-right corner.
        assert_eq!(wheel_origin(1919, 1079, 496, mon), (1424, 584));
    }

    #[test]
    fn wheel_origin_handles_a_monitor_smaller_than_the_wheel() {
        assert_eq!(wheel_origin(50, 50, 496, (0, 0, 300, 300)), (0, 0));
    }

    fn target(id: &str, kind: engines::TargetKind) -> engines::Target {
        engines::Target {
            id: id.into(),
            label: id.to_uppercase(),
            kind,
        }
    }

    #[test]
    fn preview_promises_the_real_extension() {
        let cfg = Config::default();
        assert_eq!(
            output_ext(&cfg, &target("targz", engines::TargetKind::TarGz)),
            "tar.gz"
        );
        assert_eq!(
            output_ext(&cfg, &target("webp", engines::TargetKind::Format)),
            "webp"
        );
        let mut cfg = Config::default();
        cfg.custom_targets.push(crate::config::CustomTarget {
            id: "png90".into(),
            label: "PNG q90".into(),
            ext: "png".into(),
            categories: vec!["image".into()],
            steps: vec![],
        });
        assert_eq!(
            output_ext(&cfg, &target("png90", engines::TargetKind::Custom)),
            "png",
            "a custom target's id is a name, not an extension"
        );
    }

    #[test]
    fn drops_are_accepted_until_work_starts() {
        let mut state = State {
            phase: Phase::Waiting,
            mode: Mode::Convert,
            files: vec![],
            targets: vec![],
            actions: vec![],
            tools: vec![],
            hover: None,
        };
        assert!(state.accepts_drop());
        state.phase = Phase::Ready;
        assert!(state.accepts_drop(), "a second drag replaces the selection");
        state.phase = Phase::Running { current: 0, total: 1, name: "x".into() };
        assert!(!state.accepts_drop());
        state.phase = Phase::Done { ok: 1, failed: 0 };
        assert!(!state.accepts_drop());
    }
}
