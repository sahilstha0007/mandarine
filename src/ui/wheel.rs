use crate::theming::{Rgba, Theme};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, gsk};
use std::cell::{Cell, OnceCell, RefCell};
use std::f64::consts::TAU;
use std::rc::Rc;

/// Transparent margin around the glass disc. The window surface is
/// `wheel.size + 2 * PAD`, so the drop shadow has somewhere to render — a
/// surface is clipped to its own geometry, which is why a wheel sized exactly
/// to the disc looked pasted on instead of floating.
pub const PAD: f64 = 28.0;

/// What the wheel draws: pure data, owned by the window and shared with the
/// widget so controllers can update it without a property system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scene {
    pub sectors: usize,
    pub hover: Option<usize>,
    pub waiting: bool,
    pub size: f64,
    pub inner_frac: f64,
    pub gap_deg: f64,
    pub theme: Theme,
    /// 0..=1 while a conversion is running; `Some(1.0)` once it finished.
    pub progress: Option<f64>,
    /// (active page, page count) for the little segmented dots.
    pub page: (usize, usize),
    /// A file drag is hovering the dial: draw the "armed" ring.
    pub armed: bool,
}

/// Window geometry for a wheel of `wheel` core pixels.
pub fn surface_size(wheel: i32) -> i32 {
    wheel + (PAD * 2.0).round() as i32
}

/// Sector angular ranges in degrees, measured clockwise from 12 o'clock.
pub fn sector_angles(sectors: usize, gap_deg: f64) -> Vec<(f64, f64)> {
    if sectors == 0 {
        return vec![];
    }
    let total_gap = gap_deg * sectors as f64;
    let span = (360.0 - total_gap) / sectors as f64;
    (0..sectors)
        .map(|i| (i as f64 * (span + gap_deg), i as f64 * (span + gap_deg) + span))
        .collect()
}

fn geometry(scene: &Scene) -> (f64, f64, f64) {
    let c = scene.size / 2.0;
    let outer = (scene.size / 2.0 - PAD).max(8.0);
    let inner = outer * scene.inner_frac.clamp(0.1, 0.8);
    (c, inner, outer)
}

/// Map a point in widget coordinates to a sector index, or None.
pub fn hit_test(scene: &Scene, x: f64, y: f64) -> Option<usize> {
    let (c, inner, outer) = geometry(scene);
    let dx = x - c;
    let dy = y - c;
    let r = dx.hypot(dy);
    if r < inner || r > outer || scene.sectors == 0 {
        return None;
    }
    let deg_from_top = {
        let a = dy.atan2(dx).to_degrees(); // 0 = right, cw positive (y down)
        (a + 90.0 + 360.0) % 360.0
    };
    let gap = scene.gap_deg;
    let total_gap = gap * scene.sectors as f64;
    let span = (360.0 - total_gap) / scene.sectors as f64;
    let step = span + gap;
    let idx = (deg_from_top / step).floor() as usize;
    if idx >= scene.sectors {
        return None;
    }
    if deg_from_top - idx as f64 * step <= span {
        Some(idx)
    } else {
        None // in the gap
    }
}

pub fn label_anchor(scene: &Scene, index: usize) -> (f64, f64) {
    let (c, inner, outer) = geometry(scene);
    let angles = sector_angles(scene.sectors, scene.gap_deg);
    let (a1, a2) = angles.get(index).copied().unwrap_or((0.0, 0.0));
    let mid = (a1 + a2) / 2.0;
    let r = (inner + outer) / 2.0;
    let rad = (mid - 90.0).to_radians();
    (c + r * rad.cos(), c + r * rad.sin())
}

fn ease_out_quart(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(4)
}

// --- path + paint helpers ------------------------------------------------

/// Cover-everything rect for `append_color` (the node is clipped by its path).
fn full_rect() -> graphene::Rect {
    graphene::Rect::new(0.0, 0.0, f32::MAX / 4.0, f32::MAX / 4.0)
}

fn rgba(color: Rgba) -> gdk::RGBA {
    gdk::RGBA::new(color.0 as f32, color.1 as f32, color.2 as f32, color.3 as f32)
}

fn white(alpha: f64) -> gdk::RGBA {
    gdk::RGBA::new(1.0, 1.0, 1.0, alpha as f32)
}

fn black(alpha: f64) -> gdk::RGBA {
    gdk::RGBA::new(0.0, 0.0, 0.0, alpha as f32)
}

fn lerp_color(a: Rgba, b: Rgba, t: f64) -> gdk::RGBA {
    let t = t.clamp(0.0, 1.0);
    gdk::RGBA::new(
        (a.0 + (b.0 - a.0) * t) as f32,
        (a.1 + (b.1 - a.1) * t) as f32,
        (a.2 + (b.2 - a.2) * t) as f32,
        (a.3 + (b.3 - a.3) * t) as f32,
    )
}

fn point_at(cx: f64, cy: f64, r: f64, deg: f64) -> (f32, f32) {
    let rad = (deg - 90.0).to_radians();
    ((cx + r * rad.cos()) as f32, (cy + r * rad.sin()) as f32)
}

fn disc_path(cx: f64, cy: f64, r: f64, steps: usize) -> gsk::Path {
    let pb = gsk::PathBuilder::new();
    let (x0, y0) = point_at(cx, cy, r, 0.0);
    pb.move_to(x0, y0);
    for i in 1..=steps {
        let deg = i as f64 / steps as f64 * 360.0;
        let (x, y) = point_at(cx, cy, r, deg);
        pb.line_to(x, y);
    }
    pb.close();
    pb.to_path()
}

/// The area between two concentric circles (fill with `FillRule::EvenOdd`).
/// Used to keep the drop shadow strictly outside the rim, where it belongs.
fn ring_path(cx: f64, cy: f64, r_in: f64, r_out: f64) -> gsk::Path {
    let pb = gsk::PathBuilder::new();
    for r in [r_out, r_in] {
        let (x0, y0) = point_at(cx, cy, r, 0.0);
        pb.move_to(x0, y0);
        for i in 1..=96 {
            let deg = i as f64 / 96.0 * 360.0;
            let (x, y) = point_at(cx, cy, r, deg);
            pb.line_to(x, y);
        }
        pb.close();
    }
    pb.to_path()
}

/// Filled ring segment between two radii, from `a1` to `a2` degrees (clockwise
/// from 12 o'clock).
fn band_path(cx: f64, cy: f64, ri: f64, ro: f64, a1: f64, a2: f64, steps: usize) -> gsk::Path {
    let pb = gsk::PathBuilder::new();
    let (x, y) = point_at(cx, cy, ro, a1);
    pb.move_to(x, y);
    for i in 1..=steps {
        let deg = a1 + (a2 - a1) * (i as f64 / steps as f64);
        let (x, y) = point_at(cx, cy, ro, deg);
        pb.line_to(x, y);
    }
    for i in 0..=steps {
        let deg = a2 - (a2 - a1) * (i as f64 / steps as f64);
        let (x, y) = point_at(cx, cy, ri, deg);
        pb.line_to(x, y);
    }
    pb.close();
    pb.to_path()
}

fn fill(snapshot: &gtk4::Snapshot, path: &gsk::Path, color: gdk::RGBA) {
    snapshot.push_fill(path, gsk::FillRule::Winding);
    snapshot.append_color(&color, &full_rect());
    snapshot.pop();
}

fn fill_disc(snapshot: &gtk4::Snapshot, cx: f64, cy: f64, r: f64, color: gdk::RGBA) {
    fill(snapshot, &disc_path(cx, cy, r, 96), color);
}

fn stroke(snapshot: &gtk4::Snapshot, path: &gsk::Path, color: gdk::RGBA, width: f32) {
    stroke_capped(snapshot, path, color, width, gsk::LineCap::Butt);
}

fn stroke_capped(
    snapshot: &gtk4::Snapshot,
    path: &gsk::Path,
    color: gdk::RGBA,
    width: f32,
    cap: gsk::LineCap,
) {
    snapshot.push_stroke(path, &gsk::Stroke::builder(width).line_cap(cap).build());
    snapshot.append_color(&color, &full_rect());
    snapshot.pop();
}

fn stroke_arc(
    snapshot: &gtk4::Snapshot,
    cx: f64,
    cy: f64,
    r: f64,
    a1: f64,
    a2: f64,
    color: gdk::RGBA,
    width: f32,
    cap: gsk::LineCap,
) {
    let steps = (((a2 - a1).abs() / 3.0).ceil() as usize).max(2);
    let pb = gsk::PathBuilder::new();
    let (x, y) = point_at(cx, cy, r, a1);
    pb.move_to(x, y);
    for i in 1..=steps {
        let deg = a1 + (a2 - a1) * (i as f64 / steps as f64);
        let (x, y) = point_at(cx, cy, r, deg);
        pb.line_to(x, y);
    }
    stroke_capped(snapshot, &pb.to_path(), color, width, cap);
}

/// Vertical gradient clipped to the disc — the body of the glass.
fn fill_disc_gradient(
    snapshot: &gtk4::Snapshot,
    cx: f64,
    cy: f64,
    r: f64,
    top: gdk::RGBA,
    mid: gdk::RGBA,
    bottom: gdk::RGBA,
) {
    let path = disc_path(cx, cy, r, 160);
    snapshot.push_fill(&path, gsk::FillRule::Winding);
    let bounds = graphene::Rect::new((cx - r) as f32, (cy - r) as f32, (2.0 * r) as f32, (2.0 * r) as f32);
    let start = graphene::Point::new(cx as f32, (cy - r) as f32);
    let end = graphene::Point::new(cx as f32, (cy + r) as f32);
    let stops = [
        gsk::ColorStop::new(0.0, top),
        gsk::ColorStop::new(0.52, mid),
        gsk::ColorStop::new(1.0, bottom),
    ];
    snapshot.append_linear_gradient(&bounds, &start, &end, &stops);
    snapshot.pop();
}

/// Light entering at the top edge of the disc, dying out before the middle.
///
/// A vertical linear ramp clipped to the disc, rather than a radial blob: GTK's
/// radial gradients leave a visible circular seam where the last stop lands,
/// which reads as a lens smudge on an otherwise clean material.
fn gloss(snapshot: &gtk4::Snapshot, cx: f64, cy: f64, r: f64, color: gdk::RGBA, reach: f64) {
    let path = disc_path(cx, cy, r, 160);
    snapshot.push_fill(&path, gsk::FillRule::Winding);
    let bounds = graphene::Rect::new((cx - r) as f32, (cy - r) as f32, (2.0 * r) as f32, (2.0 * r) as f32);
    let start = graphene::Point::new(cx as f32, (cy - r) as f32);
    let end = graphene::Point::new(cx as f32, (cy + r * reach) as f32);
    let faded = gdk::RGBA::new(color.red(), color.green(), color.blue(), 0.0);
    let mid = gdk::RGBA::new(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha() * 0.34,
    );
    let stops = [
        gsk::ColorStop::new(0.0, color),
        gsk::ColorStop::new(0.42, mid),
        gsk::ColorStop::new(1.0, faded),
    ];
    snapshot.append_linear_gradient(&bounds, &start, &end, &stops);
    snapshot.pop();
}

pub mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WheelAreaImp {
        pub scene: OnceCell<Rc<RefCell<Scene>>>,
        /// Monotonic start time (µs) for the materialise-in transition.
        pub started: Cell<i64>,
        /// Per-sector hover strength, smoothed every frame (0..1).
        pub hover_strength: RefCell<Vec<f64>>,
        /// Honors `gtk-enable-animations` — the freedesktop on-ramp to
        /// "prefers-reduced-motion". Off means: no entrance, no easing, no
        /// breathing loop, just instant state changes.
        pub animate: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WheelAreaImp {
        const NAME: &'static str = "MandarineWheelArea";
        type Type = super::WheelArea;
        type ParentType = gtk4::DrawingArea;
    }

    impl ObjectImpl for WheelAreaImp {}

    impl WidgetImpl for WheelAreaImp {
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let Some(scene_cell) = self.scene.get() else { return };
            let scene = *scene_cell.borrow();
            let theme = scene.theme;
            let (c, inner, outer) = geometry(&scene);
            let angles = sector_angles(scene.sectors, scene.gap_deg);
            let now = glib::monotonic_time();
            let started = self.started.get();
            let appear = if started == 0 || !self.animate.get() {
                1.0
            } else {
                ease_out_quart((now - started) as f64 / 220_000.0)
            };
            let strengths = self.hover_strength.borrow();
            let strength = |i: usize| strengths.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);

            snapshot.push_opacity(appear);

            // 1. Floating shadow — stacked translucent discs fake a gaussian
            //    without an offscreen blur pass. Clipped to the ring outside
            //    the rim: shadow leaking under the glass muddies the material
            //    and steals the contrast the labels need.
            const LAYERS: usize = 22;
            snapshot.push_fill(
                &ring_path(c, c, outer - 1.0, outer + PAD),
                gsk::FillRule::EvenOdd,
            );
            for i in 0..LAYERS {
                let t = i as f64 / (LAYERS - 1) as f64;
                let r = outer + PAD * (1.0 - t);
                let dy = 5.0 * (1.0 - t);
                fill_disc(snapshot, c, c + dy, r, rgba(theme.shadow));
            }
            snapshot.pop();

            // 2. Glass body: top light, bottom dense — that is what reads as
            //    thickness once Hyprland blurs the desktop behind it.
            fill_disc_gradient(
                snapshot,
                c,
                c,
                outer,
                rgba(theme.glass_top),
                lerp_color(theme.glass_top, theme.glass_bottom, 0.55),
                rgba(theme.glass_bottom),
            );

            // 3. Specular gloss along the top edge, then the rim lights.
            gloss(snapshot, c, c, outer, white(0.16), 0.62);

            // Bottom inner shade gives the disc a rolled edge.
            stroke_arc(snapshot, c, c, outer - 2.0, 128.0, 232.0, black(0.16), 7.0, gsk::LineCap::Round);
            stroke_arc(snapshot, c, c, outer - 1.0, 110.0, 250.0, black(0.07), 14.0, gsk::LineCap::Round);
            // Bright top rim.
            stroke_arc(snapshot, c, c, outer - 1.3, 300.0, 420.0, white(0.55), 1.6, gsk::LineCap::Round);
            stroke_arc(snapshot, c, c, outer - 4.5, 292.0, 428.0, white(0.08), 4.0, gsk::LineCap::Round);
            // Hairline around the whole disc.
            stroke(snapshot, &disc_path(c, c, outer - 0.5, 160), rgba(theme.hairline), 1.0);

            // 4. Sectors: a quiet wash at rest, accent-tinted and lifted on hover.
            for (i, (a1, a2)) in angles.iter().enumerate() {
                let s = strength(i);
                let lift = 2.6 * s;
                let ri = inner - 0.8 * s;
                let ro = outer - 1.6 + lift;
                let path = band_path(c, c, ri, ro, a1 + 0.35, a2 - 0.35, 36);
                // Hover commits: Tangerine fills the sector with the accent and
                // puts white type on it, rather than tinting the glass. A 19 %
                // wash reads as "slightly warmer", which is not an answer to
                // "which one will I get".
                let v = crate::theming::vivid(theme.accent);
                let hot = (v.0, v.1, v.2, 0.95);
                fill(snapshot, &path, lerp_color(theme.wedge, hot, s));

                // Resting separators stay implicit: the gap shows the body.
                if s > 0.01 {
                    let a = theme.accent;
                    let accent = |alpha: f64| {
                        gdk::RGBA::new(a.0 as f32, a.1 as f32, a.2 as f32, alpha as f32)
                    };
                    stroke(snapshot, &path, accent(0.45 * s), 1.2);
                    // The outer rim lights up with the sector: the one cue
                    // that reads at a glance without moving a single label.
                    stroke_arc(
                        snapshot,
                        c,
                        c,
                        outer - 1.2,
                        a1 + 1.2,
                        a2 - 1.2,
                        accent(0.70 * s),
                        1.8,
                        gsk::LineCap::Round,
                    );
                    // A halo that spills past the rim: the dial "lifts" off the
                    // desktop toward the pointer without any blur pass.
                    stroke_arc(
                        snapshot,
                        c,
                        c,
                        outer + 2.0,
                        a1 + 1.2,
                        a2 - 1.2,
                        accent(0.20 * s),
                        3.5,
                        gsk::LineCap::Round,
                    );
                }
            }

            // 5. Center well — inset, with a top shadow so it reads as recessed.
            let well = disc_path(c, c, inner, 128);
            snapshot.push_fill(&well, gsk::FillRule::Winding);
            {
                let bounds = graphene::Rect::new(
                    (c - inner) as f32,
                    (c - inner) as f32,
                    (2.0 * inner) as f32,
                    (2.0 * inner) as f32,
                );
                let center = graphene::Point::new(c as f32, (c - inner * 0.3) as f32);
                let stops = [
                    gsk::ColorStop::new(0.0, rgba(theme.center)),
                    gsk::ColorStop::new(1.0, white(0.015)),
                ];
                snapshot.append_radial_gradient(
                    &bounds,
                    &center,
                    inner as f32,
                    inner as f32,
                    0.0,
                    inner as f32,
                    &stops,
                );
            }
            snapshot.pop();
            stroke_arc(snapshot, c, c, inner - 2.5, 290.0, 430.0, black(0.14), 4.0, gsk::LineCap::Round);
            stroke_arc(snapshot, c, c, inner - 1.0, 140.0, 220.0, white(0.055), 1.5, gsk::LineCap::Round);
            stroke(snapshot, &well, white(0.13), 1.0);

            // 6. Progress: hairline track + accent arc with round caps.
            if let Some(p) = scene.progress {
                let r = outer - 7.0;
                stroke(snapshot, &disc_path(c, c, r, 160), white(0.10), 3.5);
                let p = p.clamp(0.0, 1.0);
                if p > 0.001 {
                    // A finished ring is a statement, not a shout: it settles
                    // back to a quiet accent hairline once the work is done.
                    let done = p >= 0.999;
                    let a = theme.accent;
                    let color = gdk::RGBA::new(
                        a.0 as f32,
                        a.1 as f32,
                        a.2 as f32,
                        if done { 0.55 } else { 1.0 },
                    );
                    stroke_arc(
                        snapshot,
                        c,
                        c,
                        r,
                        0.0,
                        360.0 * p,
                        color,
                        if done { 2.5 } else { 3.5 },
                        gsk::LineCap::Round,
                    );
                }
            }

            // 7a. Armed: a drag is over the dial, so the rim commits to the
            //     accent. This is the only moment the wheel shouts — and it is
            //     the answer to "is it going to take my file?".
            if scene.armed {
                let a = theme.accent;
                let accent = |alpha: f64| {
                    gdk::RGBA::new(a.0 as f32, a.1 as f32, a.2 as f32, alpha as f32)
                };
                stroke(snapshot, &disc_path(c, c, outer - 0.5, 160), accent(0.90), 2.0);
                stroke(snapshot, &disc_path(c, c, outer - 5.0, 160), accent(0.20), 5.0);
                stroke(snapshot, &disc_path(c, c, inner + 1.0, 128), accent(0.35), 1.5);
            }

            // 7b. Waiting state: a slow accent breath at 12 o'clock.
            if scene.waiting && !scene.armed && self.animate.get() {
                let phase = (now as f64 / 1_600_000.0 * TAU).sin() * 0.5 + 0.5;
                let accent = gdk::RGBA::new(
                    theme.accent.0 as f32,
                    theme.accent.1 as f32,
                    theme.accent.2 as f32,
                    (0.22 + 0.45 * phase) as f32,
                );
                stroke_arc(snapshot, c, c, outer - 2.5, -3.0, 3.0, accent, 2.0, gsk::LineCap::Round);
                stroke_arc(
                    snapshot,
                    c,
                    c,
                    inner + 3.0,
                    -70.0,
                    70.0,
                    white(0.04 + 0.05 * phase),
                    1.0,
                    gsk::LineCap::Round,
                );
                stroke_arc(
                    snapshot,
                    c,
                    c,
                    inner + 3.0,
                    110.0,
                    250.0,
                    white(0.04 + 0.05 * phase),
                    1.0,
                    gsk::LineCap::Round,
                );
            }

            // 8. Segmented page dots, like a two-up segmented control.
            if scene.page.1 > 1 && !scene.waiting {
                let total = scene.page.1.min(4);
                let y = c + inner * 0.66;
                for i in 0..total {
                    let x = c + (i as f64 - (total as f64 - 1.0) / 2.0) * 11.0;
                    let color = if i == scene.page.0 {
                        rgba(theme.accent)
                    } else {
                        white(0.20)
                    };
                    fill_disc(snapshot, x, y, 2.1, color);
                }
            }

            snapshot.pop();
        }
    }

    impl DrawingAreaImpl for WheelAreaImp {}
}

glib::wrapper! {
    pub struct WheelArea(ObjectSubclass<imp::WheelAreaImp>)
        @extends gtk4::DrawingArea, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl WheelArea {
    /// Restart the materialise-in fade.
    ///
    /// The widget outlives each appearance in resident mode, so its start time
    /// has to be reset or the second reveal pops in without the fade.
    pub fn restart_entrance(&self) {
        self.imp().started.set(glib::monotonic_time());
        self.queue_draw();
    }

    pub fn new(scene: Rc<RefCell<Scene>>, size: i32) -> Self {
        let da: Self = glib::Object::new();
        da.imp().scene.set(scene).expect("scene set once");
        da.imp().started.set(glib::monotonic_time());
        da.imp().animate.set(
            gtk4::Settings::default()
                .map(|s| s.is_gtk_enable_animations())
                .unwrap_or(true),
        );
        da.set_content_width(size);
        da.set_content_height(size);
        da.set_hexpand(false);
        da.set_vexpand(false);

        // One tick source drives materialise-in, hover easing and the waiting
        // breath. It only paints when something actually moved.
        da.add_tick_callback(move |w, _clock| {
            let imp = w.imp();
            let scene = imp.scene.get().map(|s| *s.borrow()).unwrap_or_default();
            let mut dirty = false;

            {
                let mut strengths = imp.hover_strength.borrow_mut();
                if strengths.len() != scene.sectors {
                    strengths.resize(scene.sectors, 0.0);
                    dirty = true;
                }
                let animate = imp.animate.get();
                for (i, s) in strengths.iter_mut().enumerate() {
                    let target = if scene.hover == Some(i) { 1.0 } else { 0.0 };
                    if !animate {
                        if *s != target {
                            *s = target;
                            dirty = true;
                        }
                        continue;
                    }
                    let next = *s + (target - *s) * 0.24;
                    if (next - *s).abs() > 0.002 {
                        *s = next;
                        dirty = true;
                    } else if *s != target {
                        *s = target;
                        dirty = true;
                    }
                }
            }

            let elapsed = glib::monotonic_time() - imp.started.get();
            if elapsed < 260_000 && imp.animate.get() {
                dirty = true;
            }
            if scene.waiting && imp.animate.get() {
                dirty = true;
            }
            if dirty {
                w.queue_draw();
            }
            glib::ControlFlow::Continue
        });
        da
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(sectors: usize, gap: f64) -> Scene {
        Scene {
            sectors,
            hover: None,
            waiting: false,
            size: surface_size(440) as f64,
            inner_frac: 0.42,
            gap_deg: gap,
            theme: Theme::default(),
            progress: None,
            page: (0, 2),
            armed: false,
        }
    }

    #[test]
    fn surface_adds_room_for_the_shadow() {
        assert_eq!(surface_size(440), 496);
        let s = scene(0, 0.0);
        let (c, inner, outer) = geometry(&s);
        assert!((c - 248.0).abs() < 0.01);
        assert!((outer - 220.0).abs() < 0.01);
        assert!((inner - 92.4).abs() < 0.01);
    }

    #[test]
    fn sector_angles_tile_the_circle() {
        assert_eq!(sector_angles(4, 0.0), vec![(0.0, 90.0), (90.0, 180.0), (180.0, 270.0), (270.0, 360.0)]);
        let gapped = sector_angles(4, 4.0);
        assert_eq!(gapped.len(), 4);
        // 4° gap after each sector: the last one ends 4° short of 12 o'clock.
        assert!((gapped[3].1 - 356.0).abs() < 1e-9);
        assert!((gapped[0].0 - 0.0).abs() < 1e-9);
        assert_eq!(sector_angles(0, 3.0), Vec::<(f64, f64)>::new());
    }

    #[test]
    fn hit_test_finds_the_sector_under_the_pointer() {
        let s = scene(4, 0.0);
        let (c, _, _) = geometry(&s);
        // Straight up = first sector (12 o'clock start).
        assert_eq!(hit_test(&s, c, c - 150.0), Some(0));
        // Straight right = second sector.
        assert_eq!(hit_test(&s, c + 150.0, c), Some(1));
        // Straight down = third.
        assert_eq!(hit_test(&s, c, c + 150.0), Some(2));
        // Center well and outside the rim are not targets.
        assert_eq!(hit_test(&s, c, c), None);
        assert_eq!(hit_test(&s, c + 260.0, c), None);
    }

    #[test]
    fn hit_test_skips_the_gap_between_sectors() {
        let s = scene(4, 10.0);
        let (c, _, _) = geometry(&s);
        // 85° falls inside the 80°–90° gap between sector 0 and 1.
        let rad = 85f64.to_radians();
        assert_eq!(hit_test(&s, c + 150.0 * rad.sin(), c - 150.0 * rad.cos()), None);
    }

    #[test]
    fn label_anchor_sits_between_the_radii() {
        let s = scene(4, 0.0);
        let (c, inner, outer) = geometry(&s);
        let (x, y) = label_anchor(&s, 0);
        let r = (x - c).hypot(y - c);
        assert!((r - (inner + outer) / 2.0).abs() < 0.5);
        // Sector 0 of four spans 0°–90°, so its label sits up and to the right.
        assert!(x > c && y < c);
    }
}
