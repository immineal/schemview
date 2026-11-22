//! schemview - a small, fast viewer for Minecraft schematics.
//! Usage: schemview [FILE]        open a viewer window
//!        schemview --stats FILE  parse + mesh without a window, print statistics

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod blocks;
mod formats;
mod legacy;
mod mesh;
mod nbt;
mod render;

use eframe::egui::{self, Color32, Pos2, Sense};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

const BG: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x30);

struct Loaded {
    mesh: mesh::Mesh,
    tex: assets::TexData,
    summary: String,
    schem: Arc<formats::Schem>,
    bakery: Arc<assets::Bakery>,
}

fn load_path(path: &Path) -> Result<Loaded, String> {
    let t0 = Instant::now();
    let data = std::fs::read(path).map_err(|e| format!("cannot read file: {e}"))?;
    let s = formats::load(&data)?;
    let t1 = Instant::now();
    if std::env::var_os("SCHEMVIEW_PALETTE").is_some() {
        for b in &s.palette {
            eprintln!("PALETTE {} {:?}", b.name, b.props);
        }
    }
    let mut bk = assets::bake_palette(&s.palette);
    let tb = Instant::now();
    let m = mesh::build(&s, &bk);
    let t2 = Instant::now();
    let tris = (m.opaque.idx.len() + m.trans.idx.len()) / 3;
    if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
        eprintln!("translucent quads: {}", m.trans.idx.len() / 6);
    }
    let mut summary = format!(
        "{}  ·  {} × {} × {}  ·  {} blocks  ·  {} palette entries  ·  {} triangles",
        s.format,
        s.size[0],
        s.size[1],
        s.size[2],
        m.blocks,
        s.palette.len().saturating_sub(1),
        tris
    );
    if let Some(t) = s.title.as_deref().filter(|t| !t.is_empty()) {
        summary.push_str(&format!("\n\"{t}\""));
        if let Some(a) = s.author.as_deref().filter(|a| !a.is_empty()) {
            summary.push_str(&format!(" by {a}"));
        }
    }
    summary.push_str(&format!(
        "\nparsed in {:.0} ms, models in {:.0} ms, meshed in {:.0} ms  ·  {}",
        (t1 - t0).as_secs_f64() * 1e3,
        (tb - t1).as_secs_f64() * 1e3,
        (t2 - tb).as_secs_f64() * 1e3,
        bk.source.as_deref().unwrap_or("no Minecraft install found: flat colours")
    ));
    let tex = std::mem::replace(&mut bk.tex, assets::TexData { size: 16, layers: Vec::new() });
    Ok(Loaded { mesh: m, tex, summary, schem: Arc::new(s), bakery: Arc::new(bk) })
}

fn load_guarded(path: &Path) -> Result<Loaded, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load_path(path))) {
        Ok(r) => r,
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown error".into());
            Err(format!("internal error while loading: {msg}"))
        }
    }
}

// ---------------------------------------------------------------- math

type M4 = [f32; 16];
fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0f32; 16];
    for c in 0..4 {
        for rr in 0..4 {
            r[c * 4 + rr] = (0..4).map(|k| a[k * 4 + rr] * b[c * 4 + k]).sum();
        }
    }
    r
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

struct Cam {
    yaw: f32,
    pitch: f32,
    /// Half of the visible height, in blocks.
    scale: f32,
    target: [f32; 3],
}

impl Cam {
    fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let back = [cp * sy, sp, cp * cy]; // from target towards the eye
        let f = [-back[0], -back[1], -back[2]];
        let r = norm(cross(f, [0.0, 1.0, 0.0]));
        let u = cross(r, f);
        (r, u, back)
    }
    fn mvp(&self, aspect: f32, depth: f32) -> M4 {
        let (s, u, back) = self.basis();
        let f = [-back[0], -back[1], -back[2]];
        let eye = [self.target[0] + back[0], self.target[1] + back[1], self.target[2] + back[2]];
        let view: M4 = [
            s[0], u[0], -f[0], 0.0, s[1], u[1], -f[1], 0.0, s[2], u[2], -f[2], 0.0, -dot(s, eye), -dot(u, eye), dot(f, eye), 1.0,
        ];
        let (w, h) = (self.scale * aspect, self.scale);
        let (n, fa) = (-depth, depth);
        let proj: M4 = [
            1.0 / w, 0.0, 0.0, 0.0, 0.0, 1.0 / h, 0.0, 0.0, 0.0, 0.0, -2.0 / (fa - n), 0.0, 0.0, 0.0, -(fa + n) / (fa - n), 1.0,
        ];
        mul(&proj, &view)
    }
}

// ---------------------------------------------------------------- layer range

/// Builds the faces that close the layer cut on a worker thread (latest request wins).
struct CapWorker {
    tx: mpsc::Sender<(usize, usize)>,
    rx: mpsc::Receiver<mesh::Mesh>,
}

impl CapWorker {
    fn new(s: Arc<formats::Schem>, bk: Arc<assets::Bakery>, ctx: egui::Context) -> Self {
        let (tx, wrx) = mpsc::channel::<(usize, usize)>();
        let (wtx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new().name("caps".into()).spawn(move || {
            while let Ok(mut r) = wrx.recv() {
                while let Ok(newer) = wrx.try_recv() {
                    r = newer;
                }
                let t = Instant::now();
                let m = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mesh::build_caps(&s, &bk, r.0, r.1)));
                if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
                    eprintln!("caps {r:?}: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
                }
                let Ok(m) = m else { continue };
                if wtx.send(m).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        CapWorker { tx, rx }
    }
}

#[derive(Clone, Copy)]
enum Grab {
    /// A handle; pixel offset from the press position to the handle's layer line.
    Lo(f32),
    Hi(f32),
    /// Moving the whole window; value under the pointer and the range when the drag started.
    Both(f32, u32, u32),
}

/// The slicer-style layer bar: show only block layers lo..=hi (absolute schematic y).
struct Layers {
    min: u32,
    max: u32,
    lo: u32,
    hi: u32,
    grab: Option<Grab>,
    scroll: f32,
    requested: Option<(u32, u32)>,
    worker: CapWorker,
    /// Centres of the top / bottom handle on screen (for the self test).
    hi_handle: Pos2,
    lo_handle: Pos2,
}

impl Layers {
    fn full(&self) -> bool {
        self.lo == self.min && self.hi == self.max
    }
    fn set(&mut self, lo: i64, hi: i64) {
        let (mn, mx) = (self.min as i64, self.max as i64);
        let hi = hi.clamp(mn, mx);
        let lo = lo.clamp(mn, hi);
        self.lo = lo as u32;
        self.hi = hi as u32;
    }
    /// Shift both handles, keeping the window size.
    fn shift(&mut self, lo0: u32, hi0: u32, d: i64) {
        let d = d.clamp(self.min as i64 - lo0 as i64, self.max as i64 - hi0 as i64);
        self.lo = (lo0 as i64 + d) as u32;
        self.hi = (hi0 as i64 + d) as u32;
    }
}

/// Backdrop for UI text and controls, so they stay readable over the scene.
const PANEL: Color32 = Color32::from_rgba_premultiplied(18, 19, 22, 200);

/// Paint text on a dark rounded backdrop; `pos` is the text's top-left corner.
fn boxed(painter: &egui::Painter, pos: Pos2, g: Arc<egui::Galley>) {
    painter.rect_filled(egui::Rect::from_min_size(pos, g.size()).expand2(egui::vec2(6.0, 3.0)), 4.0, PANEL);
    painter.galley(pos, g, Color32::WHITE);
}

const BAR_ACCENT: Color32 = Color32::from_rgb(0x76, 0xB2, 0x60);

// ---------------------------------------------------------------- app

struct App {
    path: Option<PathBuf>,
    rx: Option<mpsc::Receiver<Result<Loaded, String>>>,
    loading_since: Option<Instant>,
    error: Option<String>,
    summary: String,
    renderer: Option<Arc<Mutex<render::Renderer>>>,
    has_mesh: bool,
    bounds: ([f32; 3], [f32; 3]),
    cam: Cam,
    needs_fit: bool,
    show_help: bool,
    /// File name / statistics panel in the top-left corner (`I` toggles it).
    show_info: bool,
    layers: Option<Layers>,
    st_press: Pos2,
    /// Synthetic-input self test (SCHEMVIEW_SELFTEST=1): frame counter.
    selftest: Option<u32>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut error = None;
        let renderer = match cc.gl.as_ref() {
            Some(gl) => match render::Renderer::new(gl, {
                let ctx = cc.egui_ctx.clone();
                Arc::new(move || ctx.request_repaint())
            }) {
                Ok(r) => Some(Arc::new(Mutex::new(r))),
                Err(e) => {
                    error = Some(format!("OpenGL setup failed: {e}"));
                    None
                }
            },
            None => {
                error = Some("no OpenGL context available".into());
                None
            }
        };
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = App {
            path: None,
            rx: None,
            loading_since: None,
            error,
            summary: String::new(),
            renderer,
            has_mesh: false,
            bounds: ([0.0; 3], [1.0; 3]),
            cam: Cam { yaw: 0.0, pitch: 0.0, scale: 10.0, target: [0.0; 3] },
            needs_fit: true,
            show_help: false,
            show_info: std::env::var("SCHEMVIEW_INFO").map(|v| v != "0").unwrap_or(true),
            layers: None,
            st_press: Pos2::ZERO,
            selftest: std::env::var_os("SCHEMVIEW_SELFTEST").map(|_| 0),
        };
        app.reset_angles();
        if let Some(p) = path {
            app.open(&cc.egui_ctx, p);
        }
        app
    }

    fn reset_angles(&mut self) {
        self.cam.yaw = 35f32.to_radians();
        self.cam.pitch = 30f32.to_radians();
        // SCHEMVIEW_VIEW=yaw,pitch in degrees (for screenshots / tests).
        if let Ok(v) = std::env::var("SCHEMVIEW_VIEW") {
            let p: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            if p.len() == 2 {
                self.cam.yaw = p[0].to_radians();
                self.cam.pitch = p[1].clamp(-89.5, 89.5).to_radians();
            }
        }
    }

    fn open(&mut self, ctx: &egui::Context, p: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        let p2 = p.clone();
        std::thread::Builder::new()
            .name("loader".into())
            .spawn(move || {
                let r = load_guarded(&p2);
                let _ = tx.send(r);
                ctx2.request_repaint();
            })
            .ok();
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} - Schematic Viewer")));
        self.path = Some(p);
        self.rx = Some(rx);
        self.loading_since = Some(Instant::now());
        self.error = None;
    }

    fn fit(&mut self, aspect: f32) {
        self.reset_angles();
        let (mn, mx) = self.bounds;
        let c = [(mn[0] + mx[0]) / 2.0, (mn[1] + mx[1]) / 2.0, (mn[2] + mx[2]) / 2.0];
        self.cam.target = c;
        let (r, u, _) = self.cam.basis();
        let (mut er, mut eu) = (0f32, 0f32);
        for i in 0..8 {
            let p = [
                if i & 1 == 0 { mn[0] } else { mx[0] },
                if i & 2 == 0 { mn[1] } else { mx[1] },
                if i & 4 == 0 { mn[2] } else { mx[2] },
            ];
            let d = sub(p, c);
            er = er.max(dot(d, r).abs());
            eu = eu.max(dot(d, u).abs());
        }
        // Leave room for the text overlay at the top.
        self.cam.scale = (eu.max(er / aspect).max(1.0)) * 1.12;
        // SCHEMVIEW_ZOOM=f opens zoomed in by f (for screenshots / tests).
        if let Some(z) = std::env::var("SCHEMVIEW_ZOOM").ok().and_then(|v| v.parse::<f32>().ok()).filter(|z| *z > 0.0) {
            self.cam.scale /= z;
        }
    }

    /// The layer range bar on the right edge: two handles (top / bottom layer shown).
    fn layer_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(l) = &mut self.layers else { return };
        if !self.has_mesh || l.max <= l.min || rect.height() < 160.0 {
            return;
        }
        let ctx = ui.ctx().clone();
        let cx = rect.right() - 30.0;
        let (lmin, n) = (l.min, (l.max - l.min) as f32);
        // Full height for tall builds; short builds get a shorter bar so one layer is a small drag.
        let bot = rect.bottom() - 46.0;
        let top = bot - (n * 28.0).clamp(140.0, (bot - rect.top() - 62.0).max(40.0));
        let ypx = move |v: u32| bot - v.saturating_sub(lmin) as f32 / n * (bot - top);
        let vat = move |py: f32| lmin as f32 + (bot - py) / (bot - top) * n;
        const HW: f32 = 11.0; // handle half width
        const HH: f32 = 12.0; // handle height
        // The top handle sits just above its layer's position, the bottom one just below, so they never overlap.
        let hi_rect = |y: f32| egui::Rect::from_min_max(Pos2::new(cx - HW, y - HH - 1.0), Pos2::new(cx + HW, y - 1.0));
        let lo_rect = |y: f32| egui::Rect::from_min_max(Pos2::new(cx - HW, y + 1.0), Pos2::new(cx + HW, y + HH + 1.0));

        let area = egui::Rect::from_min_max(Pos2::new(cx - 26.0, top - HH - 4.0), Pos2::new(rect.right() - 4.0, bot + HH + 4.0));
        let resp = ui.interact(area, ui.id().with("layer_bar"), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }

        // Which part does a pointer position grab? Between the handles = move both, unless that can't move.
        let pick = |l: &Layers, p: Pos2| -> Grab {
            let (yh, yl) = (ypx(l.hi), ypx(l.lo));
            if hi_rect(yh).expand(4.0).contains(p) {
                Grab::Hi(yh - p.y)
            } else if lo_rect(yl).expand(4.0).contains(p) {
                Grab::Lo(yl - p.y)
            } else if p.y > yh && p.y < yl && !l.full() {
                Grab::Both(vat(p.y), l.lo, l.hi)
            } else if (p.y - yh).abs() < (p.y - yl).abs() {
                Grab::Hi(0.0)
            } else {
                Grab::Lo(0.0)
            }
        };
        // Decide what is grabbed from where the button went down, not where the pointer is once
        // egui recognises the drag (it may already have left the handle by then).
        if resp.drag_started() || resp.clicked() {
            let press = ctx.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            if let Some(p) = press.filter(|p| area.contains(*p)) {
                let mut g = pick(l, p);
                // On the empty track: the nearest handle jumps to the pointer.
                match &mut g {
                    Grab::Hi(off) if *off == 0.0 => {
                        l.set(l.lo as i64, vat(p.y).round() as i64);
                        *off = ypx(l.hi) - p.y;
                    }
                    Grab::Lo(off) if *off == 0.0 => {
                        l.set(vat(p.y).round() as i64, l.hi as i64);
                        *off = ypx(l.lo) - p.y;
                    }
                    _ => {}
                }
                l.grab = Some(g);
            }
        }
        if resp.dragged() {
            if let (Some(g), Some(p)) = (l.grab, resp.interact_pointer_pos()) {
                match g {
                    Grab::Hi(off) => l.set(l.lo as i64, vat(p.y + off).round() as i64),
                    Grab::Lo(off) => l.set(vat(p.y + off).round() as i64, l.hi as i64),
                    Grab::Both(v0, lo0, hi0) => l.shift(lo0, hi0, (vat(p.y) - v0).round() as i64),
                }
            }
        }
        if resp.drag_stopped() {
            l.grab = None;
        }
        if resp.double_clicked() {
            l.lo = l.min;
            l.hi = l.max;
        }
        // Scroll over the bar: step the handle nearest to the pointer (or both, between them).
        if resp.hovered() {
            l.scroll += ctx.input(|i| i.smooth_scroll_delta.y);
            let step = (l.scroll / 24.0).trunc();
            if step != 0.0 {
                l.scroll -= step * 24.0;
                let d = step as i64;
                if let Some(p) = resp.hover_pos() {
                    match pick(l, p) {
                        Grab::Hi(_) => l.set(l.lo as i64, l.hi as i64 + d),
                        Grab::Lo(_) => l.set(l.lo as i64 + d, l.hi as i64),
                        Grab::Both(..) => l.shift(l.lo, l.hi, d),
                    }
                }
            }
        } else {
            l.scroll = 0.0;
        }
        // Keys: Page Up / Down = top layer, with Shift = bottom layer.
        let keys: Vec<(i64, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key: egui::Key::PageUp, pressed: true, modifiers, .. } => Some((1, modifiers.shift)),
                    egui::Event::Key { key: egui::Key::PageDown, pressed: true, modifiers, .. } => Some((-1, modifiers.shift)),
                    _ => None,
                })
                .collect()
        });
        for &(d, shift) in &keys {
            if shift {
                l.set(l.lo as i64 + d, l.hi as i64);
            } else {
                l.set(l.lo as i64, l.hi as i64 + d);
            }
        }
        let d = keys.len();

        // Rebuild the faces closing the cut when the range changed.
        let want = (l.lo, l.hi);
        if l.requested != Some(want) {
            l.requested = Some(want);
            let _ = l.worker.tx.send((want.0 as usize, want.1 as usize));
        }

        // ---- draw
        let painter = ui.painter();
        let (yh, yl) = (ypx(l.hi), ypx(l.lo));
        painter.rect_filled(
            egui::Rect::from_min_max(Pos2::new(cx - HW - 5.0, top - HH - 6.0), Pos2::new(cx + HW + 5.0, bot + HH + 6.0)),
            6.0,
            PANEL,
        );
        l.hi_handle = hi_rect(yh).center();
        l.lo_handle = lo_rect(yl).center();
        painter.rect_filled(
            egui::Rect::from_min_max(Pos2::new(cx - 3.0, top), Pos2::new(cx + 3.0, bot)),
            3.0,
            Color32::from_gray(70),
        );
        painter.rect_filled(egui::Rect::from_min_max(Pos2::new(cx - 3.0, yh), Pos2::new(cx + 3.0, yl.max(yh + 1.0))), 3.0, BAR_ACCENT);
        // One tick per layer when there is room for it.
        let per = (bot - top) / n;
        if per >= 5.0 {
            for v in l.min..=l.max {
                let y = ypx(v);
                painter.line_segment([Pos2::new(cx + 5.0, y), Pos2::new(cx + 8.0, y)], egui::Stroke::new(1.0, Color32::from_gray(110)));
            }
        }
        let active = |g: Grab| match (l.grab, g) {
            (Some(Grab::Hi(_)), Grab::Hi(_)) | (Some(Grab::Lo(_)), Grab::Lo(_)) => true,
            (Some(Grab::Both(..)), _) => resp.dragged(),
            _ => false,
        };
        let font = egui::FontId::proportional(12.0);
        for (g, r, v, label_y, align) in [
            (Grab::Hi(0.0), hi_rect(yh), l.hi, yh - HH / 2.0 - 1.0, egui::Align2::RIGHT_CENTER),
            (Grab::Lo(0.0), lo_rect(yl), l.lo, yl + HH / 2.0 + 1.0, egui::Align2::RIGHT_CENTER),
        ] {
            let on = active(g) || resp.hover_pos().map(|p| r.expand(3.0).contains(p)).unwrap_or(false);
            let fill = if on { Color32::from_gray(250) } else { Color32::from_gray(215) };
            painter.rect_filled(r, 3.0, fill);
            painter.rect_stroke(r, 3.0, egui::Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
            // Grip lines.
            for k in [-2.0f32, 2.0] {
                let y = r.center().y + k;
                painter.line_segment([Pos2::new(cx - 5.0, y), Pos2::new(cx + 5.0, y)], egui::Stroke::new(1.0, Color32::from_gray(120)));
            }
            let text = format!("{}", v - l.min + 1);
            let gal = painter.layout_no_wrap(text, font.clone(), Color32::from_gray(235));
            let tr = align.anchor_size(Pos2::new(cx - HW - 6.0, label_y), gal.size()).expand2(egui::vec2(4.0, 1.0));
            painter.rect_filled(tr, 3.0, PANEL);
            painter.galley(tr.min + egui::vec2(4.0, 1.0), gal, Color32::WHITE);
        }
        let total = format!("{} layers", l.max - l.min + 1);
        let caption = if l.full() { total } else { format!("{} of {}", l.hi - l.lo + 1, total) };
        let g = painter.layout_no_wrap(caption, font, Color32::from_gray(170));
        boxed(painter, Pos2::new(rect.right() - 10.0 - g.size().x, bot + HH + 9.0), g);
        if resp.dragged() || d != 0 {
            ctx.request_repaint();
        }
    }

    fn radius(&self) -> f32 {
        let d = sub(self.bounds.1, self.bounds.0);
        (dot(d, d).sqrt() / 2.0).max(1.0)
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Finished loading?
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.rx = None;
                    self.loading_since = None;
                    match res {
                        Ok(l) => {
                            let mut ok = true;
                            if let (Some(r), Some(gl)) = (&self.renderer, frame.gl()) {
                                if let Err(e) = r.lock().unwrap().set_mesh(gl, &l.mesh, &l.tex) {
                                    self.error = Some(e);
                                    ok = false;
                                }
                            }
                            if ok {
                                self.bounds = (l.mesh.min, l.mesh.max);
                                let min = (l.mesh.min[1].max(0.0) as u32).min(mesh::Y_MASK as u32);
                                let max = ((l.mesh.max[1] as u32).saturating_sub(1)).clamp(min, mesh::Y_MASK as u32);
                                let mut ly = Layers {
                                    min,
                                    max,
                                    lo: min,
                                    hi: max,
                                    grab: None,
                                    scroll: 0.0,
                                    requested: Some((min, max)),
                                    worker: CapWorker::new(l.schem.clone(), l.bakery.clone(), ctx.clone()),
                                    hi_handle: Pos2::ZERO,
                                    lo_handle: Pos2::ZERO,
                                };
                                // SCHEMVIEW_LAYERS=lo,hi (1-based, counted from the lowest used layer) for testing.
                                if let Some(v) = std::env::var("SCHEMVIEW_LAYERS").ok() {
                                    let p: Vec<i64> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                                    if p.len() == 2 {
                                        ly.set(min as i64 + p[0] - 1, min as i64 + p[1] - 1);
                                    }
                                }
                                self.layers = Some(ly);
                                self.summary = l.summary;
                                self.has_mesh = true;
                                self.needs_fit = true;
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.loading_since = None;
                    self.error = Some("loader stopped unexpectedly".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        // Drag & drop another file onto the window.
        let dropped: Option<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| !p.as_os_str().is_empty()));
        if let Some(p) = dropped {
            self.open(&ctx, p);
        }

        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        // Reserve the bottom-most paint slot for the 3D view, so every piece of UI is drawn above it.
        let scene_slot = ui.painter().add(egui::Shape::Noop);
        self.layer_bar(ui, rect);
        let aspect = (rect.width() / rect.height().max(1.0)).max(0.01);
        if self.needs_fit && self.has_mesh {
            self.fit(aspect);
            self.needs_fit = false;
        }

        // ---- input
        let px_to_world = 2.0 * self.cam.scale / rect.height().max(1.0);
        if resp.dragged_by(egui::PointerButton::Primary) && !ctx.input(|i| i.modifiers.shift) {
            let d = resp.drag_delta();
            self.cam.yaw -= d.x * 0.008;
            self.cam.pitch = (self.cam.pitch + d.y * 0.008).clamp(-89.5f32.to_radians(), 89.5f32.to_radians());
        }
        if resp.dragged_by(egui::PointerButton::Secondary)
            || resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary) && ctx.input(|i| i.modifiers.shift))
        {
            let d = resp.drag_delta();
            let (r, u, _) = self.cam.basis();
            for a in 0..3 {
                self.cam.target[a] += (-r[a] * d.x + u[a] * d.y) * px_to_world;
            }
        }
        if resp.double_clicked() {
            self.needs_fit = true;
        }
        let (scroll, zoom, keys) = ctx.input(|i| {
            let k = |key| i.key_down(key);
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                (
                    k(egui::Key::ArrowLeft),
                    k(egui::Key::ArrowRight),
                    k(egui::Key::ArrowUp),
                    k(egui::Key::ArrowDown),
                    k(egui::Key::Plus) || k(egui::Key::Equals),
                    k(egui::Key::Minus),
                    i.key_pressed(egui::Key::R) || i.key_pressed(egui::Key::Home) || i.key_pressed(egui::Key::F),
                    i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::F1),
                    i.key_pressed(egui::Key::I),
                    i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Q),
                    i.stable_dt.min(0.1),
                ),
            )
        });
        let (kl, kr, ku, kd, kin, kout, kreset, khelp, kinfo, kquit, dt) = keys;
        if kinfo {
            self.show_info = !self.show_info;
        }
        let rot = 1.8 * dt;
        if kl {
            self.cam.yaw += rot;
        }
        if kr {
            self.cam.yaw -= rot;
        }
        if ku {
            self.cam.pitch = (self.cam.pitch + rot).min(89.5f32.to_radians());
        }
        if kd {
            self.cam.pitch = (self.cam.pitch - rot).max(-89.5f32.to_radians());
        }
        let mut factor = 1.0f32;
        if resp.hovered() || resp.dragged() {
            factor *= (-scroll * 0.0018).exp() / zoom.max(0.01);
        }
        if kin {
            factor *= (-2.0 * dt).exp();
        }
        if kout {
            factor *= (2.0 * dt).exp();
        }
        if (factor - 1.0).abs() > 1e-6 {
            let max_scale = self.radius() * 20.0;
            let new_scale = (self.cam.scale * factor).clamp(0.25, max_scale.max(1.0));
            // Zoom towards the cursor.
            if let Some(pos) = resp.hover_pos() {
                let c = rect.center();
                let half_h = rect.height().max(1.0) / 2.0;
                let (cx, cy) = ((pos.x - c.x) / half_h, -(pos.y - c.y) / half_h);
                let (r, u, _) = self.cam.basis();
                let ds = self.cam.scale - new_scale;
                for a in 0..3 {
                    self.cam.target[a] += (r[a] * cx + u[a] * cy) * ds;
                }
            }
            self.cam.scale = new_scale;
        }
        if kreset {
            self.needs_fit = true;
        }
        if khelp {
            self.show_help = !self.show_help;
        }
        if kquit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if kl || kr || ku || kd || kin || kout || kreset {
            ctx.request_repaint();
        }

        // ---- 3D
        if let (Some(r), true) = (&self.renderer, self.has_mesh) {
            let depth = self.radius() * 4.0 + 64.0 + {
                let t = self.cam.target;
                let c = [
                    (self.bounds.0[0] + self.bounds.1[0]) / 2.0,
                    (self.bounds.0[1] + self.bounds.1[1]) / 2.0,
                    (self.bounds.0[2] + self.bounds.1[2]) / 2.0,
                ];
                let d = sub(t, c);
                dot(d, d).sqrt()
            };
            let mvp = self.cam.mvp(aspect, depth);
            let back = self.cam.basis().2;
            let range = self.layers.as_ref().map(|l| [l.lo as f32, l.hi as f32]).unwrap_or([0.0, 1e9]);
            if let (Some(l), Some(gl)) = (&mut self.layers, frame.gl()) {
                if let Some(m) = l.worker.rx.try_iter().last() {
                    if let Err(e) = r.lock().unwrap().set_caps(gl, &m) {
                        eprintln!("schemview: {e}");
                    }
                }
            }
            let r = r.clone();
            let cb = eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                if let Ok(mut r) = r.lock() {
                    r.paint(painter.gl(), &mvp, back, range);
                }
            });
            ui.painter().set(scene_slot, egui::PaintCallback { rect, callback: Arc::new(cb) });
        }

        // ---- overlay
        let painter = ui.painter();
        let font = egui::FontId::proportional(13.0);
        let dim = Color32::from_gray(170);
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No file".into());
        let text_w = (rect.width() - 100.0).max(100.0);
        let origin = Pos2::new(rect.left() + 10.0, rect.top() + 8.0);
        let g = painter.layout(name, egui::FontId::proportional(15.0), Color32::from_gray(235), text_w);
        let mut block = egui::Rect::from_min_size(origin, g.size());
        let mut parts = vec![(origin, g)];
        if self.has_mesh && self.loading_since.is_none() {
            let pos = Pos2::new(origin.x, block.bottom() + 2.0);
            let g = painter.layout(self.summary.clone(), font.clone(), dim, text_w);
            block = block.union(egui::Rect::from_min_size(pos, g.size()));
            parts.push((pos, g));
        }
        if self.show_info || self.loading_since.is_some() {
            painter.rect_filled(block.expand2(egui::vec2(6.0, 4.0)), 4.0, PANEL);
            for (pos, g) in parts {
                painter.galley(pos, g, Color32::WHITE);
            }
        }
        if let Some(t) = self.loading_since {
            let dots = ".".repeat(1 + (t.elapsed().as_millis() / 300 % 3) as usize);
            let g = painter.layout_no_wrap(format!("Loading{dots}"), egui::FontId::proportional(20.0), Color32::from_gray(220));
            boxed(painter, rect.center() - g.size() / 2.0, g);
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        } else if let Some(e) = &self.error {
            let g = painter.layout(format!("Could not open file:\n{e}"), egui::FontId::proportional(16.0), Color32::from_rgb(255, 170, 150), rect.width() * 0.8);
            boxed(painter, rect.center() - g.size() / 2.0, g);
        } else if !self.has_mesh {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop a .litematic / .schematic / .schem / .nbt file here",
                egui::FontId::proportional(18.0),
                Color32::from_gray(200),
            );
        }
        let hint = if self.show_help {
            "Left-drag: orbit   ·   Scroll / pinch / + −: zoom   ·   Right/middle or Shift+drag: pan\nArrows: orbit   ·   R / double-click: reset view   ·   I: file info on/off   ·   drop a file to open it   ·   Q / Esc: quit   ·   H: hide help\nLayer bar (right): drag the top / bottom handle, drag between them to move both, scroll on it, double-click: all layers\nPage Up / Down: top layer   ·   Shift + Page Up / Down: bottom layer"
        } else {
            "H: help"
        };
        let g = painter.layout(hint.to_string(), font, Color32::from_gray(140), text_w);
        let pos = Pos2::new(rect.left() + 10.0, rect.bottom() - 8.0 - g.size().y);
        boxed(painter, pos, g);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(f) = self.selftest else { return };
        if !self.has_mesh {
            return;
        }
        use egui::{Event, PointerButton as B};
        let m = egui::Modifiers::default();
        let p = |x: f32, y: f32| Pos2::new(x, y);
        let press = |x, y, button, pressed| Event::PointerButton { pos: p(x, y), button, pressed, modifiers: m };
        let c = &self.cam;
        match f {
            0 => raw.events.push(Event::PointerMoved(p(400.0, 300.0))),
            1 => {
                eprintln!("selftest start yaw={:.3} pitch={:.3} target={:?} scale={:.2}", c.yaw, c.pitch, c.target, c.scale);
                raw.events.push(press(400.0, 300.0, B::Primary, true))
            }
            2..=11 => raw.events.push(Event::PointerMoved(p(400.0 + 15.0 * (f - 1) as f32, 300.0 + 5.0 * (f - 1) as f32))),
            12 => {
                raw.events.push(press(550.0, 350.0, B::Primary, false));
                eprintln!("selftest orbit frames dt: {:?}", ctx.input(|i| i.unstable_dt));
            }
            13 => {
                eprintln!("selftest after orbit yaw={:.3} pitch={:.3}", c.yaw, c.pitch);
                raw.events.push(press(550.0, 350.0, B::Secondary, true))
            }
            14..=23 => raw.events.push(Event::PointerMoved(p(550.0 + 10.0 * (f - 13) as f32, 350.0))),
            24 => raw.events.push(press(650.0, 350.0, B::Secondary, false)),
            25 => {
                eprintln!("selftest after pan target={:?}", c.target);
                raw.events.push(Event::Key { key: egui::Key::R, physical_key: None, pressed: true, repeat: false, modifiers: m });
            }
            27 => eprintln!("selftest after reset yaw={:.3} pitch={:.3} target={:?} scale={:.2}", c.yaw, c.pitch, c.target, c.scale),
            // Layer bar, moved like a real mouse: press on a handle, then move away from the press point.
            28..=60 => {
                if let Some(l) = &mut self.layers {
                    let span = l.max - l.min;
                    // Per frame: 8 px steps, for the top handle downwards and then the bottom handle upwards.
                    let step = 8.0f32;
                    match f {
                        28 => {
                            eprintln!("selftest layers before lo={} hi={} ({} layers)", l.lo, l.hi, span + 1);
                            self.st_press = l.hi_handle;
                            raw.events.push(Event::PointerMoved(self.st_press));
                        }
                        29 | 40 => raw.events.push(press(self.st_press.x, self.st_press.y, B::Primary, true)),
                        30..=37 => raw.events.push(Event::PointerMoved(p(self.st_press.x, self.st_press.y + step * (f - 29) as f32))),
                        38 | 49 => raw.events.push(press(self.st_press.x, self.st_press.y, B::Primary, false)),
                        39 => {
                            eprintln!("selftest top handle dragged down {} px: lo={} hi={}", step * 8.0, l.lo, l.hi);
                            self.st_press = l.lo_handle;
                            raw.events.push(Event::PointerMoved(self.st_press));
                        }
                        41..=48 => raw.events.push(Event::PointerMoved(p(self.st_press.x, self.st_press.y - step * (f - 40) as f32))),
                        50 => {
                            eprintln!("selftest bottom handle dragged up {} px: lo={} hi={}", step * 8.0, l.lo, l.hi);
                            let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
                            raw.events.push(key(egui::Key::PageDown, m));
                        }
                        51 => {
                            eprintln!("selftest after PageDown lo={} hi={}", l.lo, l.hi);
                            raw.events.push(Event::Key { key: egui::Key::PageUp, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT });
                        }
                        52 => eprintln!("selftest after Shift+PageUp lo={} hi={}", l.lo, l.hi),
                        _ => {}
                    }
                }
            }
            61 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => {}
        }
        self.selftest = Some(f + 1);
        ctx.request_repaint();
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let (Some(gl), Some(r)) = (gl, &self.renderer) {
            if let Ok(mut r) = r.lock() {
                r.destroy(gl);
            }
        }
    }
}

// ---------------------------------------------------------------- startup

fn path_from_arg(a: &str) -> PathBuf {
    if let Some(rest) = a.strip_prefix("file://") {
        // Percent-decode a file:// URI.
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let b = rest.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        return PathBuf::from(String::from_utf8_lossy(&out).into_owned());
    }
    PathBuf::from(a)
}

fn icon() -> egui::IconData {
    // Small isometric cube, drawn procedurally.
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let (cx, cy, s) = (32.0f32, 33.0f32, 27.0f32);
    for py in 0..n {
        for px in 0..n {
            let x = (px as f32 + 0.5 - cx) / s;
            let y = (py as f32 + 0.5 - cy) / s;
            // Hexagon split into top / left / right faces.
            let a = 0.866f32;
            let inside = x.abs() <= a && (y.abs() + x.abs() * 0.5774) <= 1.0;
            if !inside {
                continue;
            }
            let c: [u8; 3] = if y <= -0.5774 * x.abs() {
                [118, 178, 96]
            } else if x < 0.0 {
                [138, 118, 100]
            } else {
                [104, 90, 78]
            };
            let i = (py * n + px) * 4;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

fn run_stats(files: &[String]) -> i32 {
    let mut rc = 0;
    for f in files {
        let p = path_from_arg(f);
        match load_guarded(&p) {
            Ok(l) => println!("OK   {}\n     {}", p.display(), l.summary.replace('\n', "\n     ")),
            Err(e) => {
                println!("FAIL {}\n     {e}", p.display());
                rc = 1;
            }
        }
    }
    rc
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--stats").unwrap_or(false) {
        std::process::exit(run_stats(&args[1..]));
    }
    if args.first().map(|a| a == "--help" || a == "-h").unwrap_or(false) {
        println!("schemview [FILE]         view a .litematic / .schematic / .schem / .nbt file\nschemview --stats FILE…  load and mesh without a window, print statistics");
        return;
    }
    let path = args.iter().find(|a| !a.starts_with("--")).map(|a| path_from_arg(a));

    let mut last_err = String::new();
    // Try with MSAA first, then without, so odd drivers still get a window.
    for msaa in [4u16, 0] {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Schematic Viewer")
                .with_app_id("schemview")
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([320.0, 240.0])
                .with_drag_and_drop(true)
                .with_icon(Arc::new(icon()))
                .with_maximized(std::env::var_os("SCHEMVIEW_MAXIMIZED").is_some()),
            multisampling: msaa,
            depth_buffer: 24,
            ..Default::default()
        };
        let p = path.clone();
        match eframe::run_native("schemview", options, Box::new(move |cc| Ok(Box::new(App::new(cc, p))))) {
            Ok(()) => return,
            Err(e) => {
                last_err = e.to_string();
                eprintln!("schemview: window creation failed with msaa={msaa}: {last_err}");
            }
        }
    }
    eprintln!("schemview: giving up: {last_err}");
    std::process::exit(1);
}
