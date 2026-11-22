//! Real Minecraft look: reads blockstates, block models and textures from the user's installed
//! Minecraft client jar at runtime (nothing is bundled), and bakes each palette entry into quads.

use crate::blocks::{self, Bx, Conn, Info, Kind};
use crate::formats::BlockState;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

// ---------------------------------------------------------------- baked output

/// A quad in block-local coordinates (0..1). Vertex order is counter-clockwise seen from outside.
#[derive(Clone, Debug)]
pub struct Quad {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub tex: u16,
    pub color: [u8; 3],
    /// Face index (0 -X, 1 +X, 2 -Y, 3 +Y, 4 -Z, 5 +Z) whose neighbour hides this quad, or -1.
    pub cull: i8,
    /// Axis-aligned face index of the quad normal, or -1.
    pub face: i8,
    pub shade: f32,
    pub ao: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fluid {
    None,
    Water,
    Lava,
}

#[derive(Clone)]
pub struct Baked {
    pub quads: Vec<Quad>,
    pub translucent: bool,
    pub occludes: bool,
    /// Full-cube see-through block (glass, slime, ...): hides faces shared with the same block.
    pub self_cull: bool,
    pub group: u32,
    pub fluid: Fluid,
    /// Only the fluid, no block geometry (water / lava source).
    pub fluid_only: bool,
    /// Flat-colour fallback geometry (gets subtle per-block colour noise).
    pub flat: bool,
    pub air: bool,
    /// Vertex alpha (255 for textured blocks; texture alpha does the rest).
    pub alpha: u8,
    /// Fences / panes / walls without connection properties: 16 variants indexed by N=1,E=2,S=4,W=8.
    pub conn_variants: Option<Vec<Baked>>,
    pub conn: Option<Conn>,
    pub is_gate: bool,
}

impl Baked {
    fn empty() -> Self {
        Baked {
            quads: vec![],
            translucent: false,
            occludes: false,
            self_cull: false,
            group: 0,
            fluid: Fluid::None,
            fluid_only: false,
            flat: false,
            air: true,
            alpha: 255,
            conn_variants: None,
            conn: None,
            is_gate: false,
        }
    }
}

pub struct TexData {
    pub size: u32,
    /// RGBA8 layers of size*size; layer 0 is plain white.
    pub layers: Vec<Vec<u8>>,
}

pub struct Bakery {
    pub bakes: Vec<Baked>,
    pub tex: TexData,
    pub water_tex: u16,
    pub lava_tex: u16,
    pub source: Option<String>,
}

// ---------------------------------------------------------------- jar discovery

fn version_key(name: &str) -> Option<Vec<u64>> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    name.split('.').map(|p| p.parse().ok()).collect()
}

/// Newest release client jar from the default launcher dir (override: SCHEMVIEW_JAR=/path/to/client.jar).
pub fn find_jar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SCHEMVIEW_JAR") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    // Vanilla launcher: <root>/versions/<v>/<v>.jar.
    let mut vanilla = vec![];
    if let Some(h) = &home {
        vanilla.push(h.join(".minecraft/versions"));
        vanilla.push(h.join(".var/app/com.mojang.Minecraft/.minecraft/versions"));
        vanilla.push(h.join("Library/Application Support/minecraft/versions"));
    }
    if let Some(a) = env("APPDATA") {
        vanilla.push(a.join(".minecraft").join("versions"));
    }
    // Prism / MultiMC style launchers: <root>/libraries/com/mojang/minecraft/<v>/minecraft-<v>-client.jar.
    let mut prism = vec![];
    if let Some(h) = &home {
        prism.push(h.join(".local/share/PrismLauncher"));
        prism.push(h.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"));
        prism.push(h.join("Library/Application Support/PrismLauncher"));
    }
    if let Some(a) = env("APPDATA") {
        prism.push(a.join("PrismLauncher"));
    }
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    let mut consider = |name: &str, jar: PathBuf| {
        let Some(k) = version_key(name) else { return };
        // The modern blockstate / model format exists since 1.13.
        if k.first() == Some(&1) && k.get(1).copied().unwrap_or(0) < 13 {
            return;
        }
        if std::fs::metadata(&jar).map(|m| m.len() > 1_000_000).unwrap_or(false) && best.as_ref().map(|(bk, _)| k > *bk).unwrap_or(true) {
            best = Some((k, jar));
        }
    };
    for root in vanilla {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            consider(&name, e.path().join(format!("{name}.jar")));
        }
    }
    for root in prism {
        let Ok(rd) = std::fs::read_dir(root.join("libraries/com/mojang/minecraft")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            consider(&name, e.path().join(format!("minecraft-{name}-client.jar")));
        }
    }
    best.map(|(_, p)| p)
}

struct Jar {
    zip: zip::ZipArchive<std::io::BufReader<std::fs::File>>,
}

impl Jar {
    fn open(p: &Path) -> Option<Jar> {
        let f = std::fs::File::open(p).ok()?;
        let zip = zip::ZipArchive::new(std::io::BufReader::with_capacity(1 << 16, f)).ok()?;
        Some(Jar { zip })
    }
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut f = self.zip.by_name(path).ok()?;
        let mut v = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut v).ok()?;
        Some(v)
    }
    fn json(&mut self, path: &str) -> Option<Value> {
        serde_json::from_slice(&self.read(path)?).ok()
    }
}

// ---------------------------------------------------------------- textures

struct Img {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn decode_png(data: &[u8]) -> Option<Img> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let src = &buf[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(n * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(&src[..n * 4]),
        png::ColorType::Rgb => {
            for c in src.chunks_exact(3).take(n) {
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for c in src.chunks_exact(2).take(n) {
                rgba.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &c in src.iter().take(n) {
                rgba.extend_from_slice(&[c, c, c, 255]);
            }
        }
        _ => return None,
    }
    if rgba.len() != n * 4 {
        return None;
    }
    Some(Img { w, h, rgba })
}

struct TexStore {
    imgs: Vec<Img>,
    opaque: Vec<bool>,
    map: HashMap<String, Option<u16>>,
}

impl TexStore {
    fn new() -> Self {
        TexStore { imgs: vec![Img { w: 1, h: 1, rgba: vec![255; 4] }], opaque: vec![true], map: HashMap::new() }
    }
    /// "minecraft:block/stone" -> layer index.
    fn get(&mut self, jar: &mut Jar, name: &str) -> Option<u16> {
        if let Some(r) = self.map.get(name) {
            return *r;
        }
        let (ns, p) = name.split_once(':').unwrap_or(("minecraft", name));
        let r = jar.read(&format!("assets/{ns}/textures/{p}.png")).and_then(|d| decode_png(&d)).and_then(|mut img| {
            // Animated strip: keep the first frame.
            if img.h > img.w && img.w > 0 {
                img.rgba.truncate((img.w * img.w * 4) as usize);
                img.h = img.w;
            }
            if self.imgs.len() >= u16::MAX as usize {
                return None;
            }
            let opaque = img.rgba.chunks_exact(4).all(|c| c[3] == 255);
            if !opaque {
                // Give fully transparent pixels the average colour of the visible ones, so mipmaps of
                // cutout textures (leaves, plants) don't bleed white/black into the distance.
                let (mut sum, mut n) = ([0u64; 3], 0u64);
                for c in img.rgba.chunks_exact(4).filter(|c| c[3] > 0) {
                    for k in 0..3 {
                        sum[k] += c[k] as u64;
                    }
                    n += 1;
                }
                if n > 0 {
                    let avg = sum.map(|v| (v / n) as u8);
                    for c in img.rgba.chunks_exact_mut(4).filter(|c| c[3] == 0) {
                        c[..3].copy_from_slice(&avg);
                    }
                }
            }
            self.imgs.push(img);
            self.opaque.push(opaque);
            Some((self.imgs.len() - 1) as u16)
        });
        self.map.insert(name.to_string(), r);
        r
    }
    fn finish(self) -> (TexData, Vec<bool>) {
        let size = self.imgs.iter().skip(1).map(|i| i.w.max(i.h)).max().unwrap_or(16).clamp(16, 64).next_power_of_two();
        let layers = self
            .imgs
            .iter()
            .map(|img| {
                let mut out = vec![0u8; (size * size * 4) as usize];
                for y in 0..size {
                    for x in 0..size {
                        let sx = (x * img.w / size).min(img.w - 1);
                        let sy = (y * img.h / size).min(img.h - 1);
                        let s = ((sy * img.w + sx) * 4) as usize;
                        let d = ((y * size + x) * 4) as usize;
                        out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
                    }
                }
                out
            })
            .collect();
        (TexData { size, layers }, self.opaque)
    }
}

// ---------------------------------------------------------------- models

struct Model {
    /// variable -> (texture reference, force_translucent)
    textures: HashMap<String, (String, bool)>,
    elements: Option<Rc<Vec<Value>>>,
    ao: bool,
}

struct Ctx {
    jar: Jar,
    tex: TexStore,
    models: HashMap<String, Option<Rc<Model>>>,
    states: HashMap<String, Option<Rc<Value>>>,
}

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

impl Ctx {
    fn model(&mut self, name: &str, depth: u32) -> Option<Rc<Model>> {
        let key = strip_ns(name).to_string();
        if let Some(m) = self.models.get(&key) {
            return m.clone();
        }
        let r = self.load_model(&key, depth);
        self.models.insert(key, r.clone());
        r
    }
    fn load_model(&mut self, key: &str, depth: u32) -> Option<Rc<Model>> {
        if depth > 32 || key.starts_with("builtin/") {
            return None;
        }
        let path = if key.contains('/') { key.to_string() } else { format!("block/{key}") };
        let j = self.jar.json(&format!("assets/minecraft/models/{path}.json"))?;
        let parent = j.get("parent").and_then(|p| p.as_str()).and_then(|p| self.model(p, depth + 1));
        let mut textures = parent.as_ref().map(|p| p.textures.clone()).unwrap_or_default();
        if let Some(t) = j.get("textures").and_then(|t| t.as_object()) {
            for (k, v) in t {
                if let Some(s) = v.as_str() {
                    textures.insert(k.clone(), (s.to_string(), false));
                } else if let Some(s) = v.get("sprite").and_then(|s| s.as_str()) {
                    let ft = v.get("force_translucent").and_then(|f| f.as_bool()).unwrap_or(false);
                    textures.insert(k.clone(), (s.to_string(), ft));
                }
            }
        }
        let elements = match j.get("elements").and_then(|e| e.as_array()) {
            Some(e) => Some(Rc::new(e.clone())),
            None => parent.as_ref().and_then(|p| p.elements.clone()),
        };
        let ao = j
            .get("ambientocclusion")
            .and_then(|a| a.as_bool())
            .unwrap_or_else(|| parent.as_ref().map(|p| p.ao).unwrap_or(true));
        Some(Rc::new(Model { textures, elements, ao }))
    }
    fn blockstate(&mut self, name: &str) -> Option<Rc<Value>> {
        if let Some(s) = self.states.get(name) {
            return s.clone();
        }
        let r = self.jar.json(&format!("assets/minecraft/blockstates/{name}.json")).map(Rc::new);
        self.states.insert(name.to_string(), r.clone());
        r
    }
    /// Resolve "#var" chains to a texture layer; also reports `force_translucent`.
    fn resolve_tex(&mut self, m: &Model, r: &str) -> Option<(u16, bool)> {
        let mut r = r.to_string();
        let mut ft = false;
        for _ in 0..16 {
            match r.strip_prefix('#') {
                Some(var) => {
                    let (nr, f) = m.textures.get(var)?.clone();
                    ft |= f;
                    r = nr;
                }
                None => break,
            }
        }
        if r.starts_with('#') {
            return None;
        }
        let full = if r.contains(':') { r } else { format!("minecraft:{r}") };
        self.tex.get(&mut self.jar, &full).map(|t| (t, ft))
    }
}

// ---------------------------------------------------------------- geometry helpers

const DIRS: [&str; 6] = ["west", "east", "down", "up", "north", "south"];
const DIRV: [[f32; 3]; 6] = [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]];

fn dir_index(s: &str) -> Option<usize> {
    let s = match s {
        "bottom" => "down",
        "top" => "up",
        o => o,
    };
    DIRS.iter().position(|d| *d == s)
}

fn face_index_of(n: [f32; 3]) -> i8 {
    for (i, d) in DIRV.iter().enumerate() {
        if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] > 0.999 {
            return i as i8;
        }
    }
    -1
}

/// Vertex positions of a face of box f..t (Minecraft FaceInfo order).
fn face_verts(dir: usize, f: [f32; 3], t: [f32; 3]) -> [[f32; 3]; 4] {
    match dir {
        2 => [[f[0], f[1], t[2]], [f[0], f[1], f[2]], [t[0], f[1], f[2]], [t[0], f[1], t[2]]],
        3 => [[f[0], t[1], f[2]], [f[0], t[1], t[2]], [t[0], t[1], t[2]], [t[0], t[1], f[2]]],
        4 => [[t[0], t[1], f[2]], [t[0], f[1], f[2]], [f[0], f[1], f[2]], [f[0], t[1], f[2]]],
        5 => [[f[0], t[1], t[2]], [f[0], f[1], t[2]], [t[0], f[1], t[2]], [t[0], t[1], t[2]]],
        0 => [[f[0], t[1], f[2]], [f[0], f[1], f[2]], [f[0], f[1], t[2]], [f[0], t[1], t[2]]],
        _ => [[t[0], t[1], t[2]], [t[0], f[1], t[2]], [t[0], f[1], f[2]], [t[0], t[1], f[2]]],
    }
}

/// UV in 0..16 of a point p (0..16) projected on an axis-aligned face (world-aligned / default mapping).
fn plane_uv(dir: usize, p: [f32; 3]) -> [f32; 2] {
    match dir {
        3 => [p[0], p[2]],
        2 => [p[0], 16.0 - p[2]],
        4 => [16.0 - p[0], 16.0 - p[1]],
        5 => [p[0], 16.0 - p[1]],
        0 => [p[2], 16.0 - p[1]],
        _ => [16.0 - p[2], 16.0 - p[1]],
    }
}

fn default_uv(dir: usize, f: [f32; 3], t: [f32; 3]) -> [f32; 4] {
    match dir {
        2 => [f[0], 16.0 - t[2], t[0], 16.0 - f[2]],
        3 => [f[0], f[2], t[0], t[2]],
        4 => [16.0 - t[0], 16.0 - t[1], 16.0 - f[0], 16.0 - f[1]],
        5 => [f[0], 16.0 - t[1], t[0], 16.0 - f[1]],
        0 => [f[2], 16.0 - t[1], t[2], 16.0 - f[1]],
        _ => [16.0 - t[2], 16.0 - t[1], 16.0 - f[2], 16.0 - f[1]],
    }
}

type M3 = [[f32; 3]; 3];
fn rot_axis(axis: usize, deg: f32) -> M3 {
    let (s, c) = deg.to_radians().sin_cos();
    match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}
fn mv(m: &M3, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}
fn mm(a: &M3, b: &M3) -> M3 {
    let mut r = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}
fn snap(v: f32) -> f32 {
    let r = v.round();
    if (v - r).abs() < 1e-4 {
        r
    } else {
        v
    }
}

fn shade_of(n: [f32; 3]) -> f32 {
    n[0] * n[0] * 0.6 + n[2] * n[2] * 0.8 + n[1] * n[1] * if n[1] > 0.0 { 1.0 } else { 0.5 }
}

fn arr3(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
}

/// Model-level transform: rotation about the block centre (x first, then y), plus uvlock.
struct Xf {
    m: M3,
    rotated: bool,
    uvlock: bool,
}

impl Xf {
    fn new(x: f32, y: f32, uvlock: bool) -> Self {
        let m = mm(&rot_axis(1, -y), &rot_axis(0, -x));
        Xf { m, rotated: x != 0.0 || y != 0.0, uvlock }
    }
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let q = mv(&self.m, [p[0] - 8.0, p[1] - 8.0, p[2] - 8.0]);
        [snap(q[0] + 8.0), snap(q[1] + 8.0), snap(q[2] + 8.0)]
    }
}

// ---------------------------------------------------------------- tints & render types

fn tint_for(st: &BlockState) -> [u8; 3] {
    let n = st.name.as_str();
    let grass = [145, 189, 89];
    if n.contains("spruce_leaves") {
        return [97, 153, 97];
    }
    if n.contains("birch_leaves") {
        return [128, 167, 85];
    }
    if n.ends_with("_leaves") || n == "vine" || n.contains("mangrove_leaves") {
        return [119, 171, 47];
    }
    match n {
        "water" | "bubble_column" => [63, 118, 228],
        "lily_pad" => [32, 128, 48],
        "redstone_wire" => {
            let p: f32 = st.prop("power").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let f = p / 15.0;
            let r = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
            let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
            let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
            [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
        }
        "pumpkin_stem" | "melon_stem" => {
            let a: u32 = st.prop("age").and_then(|s| s.parse().ok()).unwrap_or(7).min(7);
            [(a * 32) as u8, (255 - a * 8) as u8, (a * 4) as u8]
        }
        "attached_pumpkin_stem" | "attached_melon_stem" => [224, 199, 28],
        _ => grass,
    }
}

fn is_translucent(n: &str) -> bool {
    n.contains("stained_glass")
        || matches!(
            n,
            "ice" | "frosted_ice" | "slime_block" | "honey_block" | "water" | "bubble_column" | "nether_portal" | "tinted_glass"
        )
}

fn particle_fallback(n: &str) -> bool {
    n.ends_with("chest") || n.ends_with("shulker_box") || n.ends_with("_sign") || n == "decorated_pot" || n.ends_with("_bed")
}

// ---------------------------------------------------------------- baking

impl Ctx {
    fn bake_model(&mut self, m: &Model, xf: &Xf, tint: [u8; 3], out: &mut Vec<Quad>, full_opaque: &mut bool, translucent: &mut bool) {
        let Some(elements) = m.elements.clone() else { return };
        for e in elements.iter() {
            let (Some(f), Some(t)) = (arr3(e.get("from")), arr3(e.get("to"))) else { continue };
            let shade_on = e.get("shade").and_then(|s| s.as_bool()).unwrap_or(true);
            let shade_override = e.get("shade_direction_override").and_then(|s| s.as_str()).and_then(dir_index);
            // Element rotation.
            let mut erot: Option<(M3, [f32; 3], [f32; 3])> = None;
            if let Some(r) = e.get("rotation") {
                let origin = arr3(r.get("origin")).unwrap_or([8.0, 8.0, 8.0]);
                let mut mat: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
                let mut scale = [1.0f32; 3];
                let rescale = r.get("rescale").and_then(|v| v.as_bool()).unwrap_or(false);
                if let (Some(ax), Some(ang)) = (r.get("axis").and_then(|a| a.as_str()), r.get("angle").and_then(|a| a.as_f64())) {
                    let ai = match ax {
                        "x" => 0,
                        "y" => 1,
                        _ => 2,
                    };
                    mat = rot_axis(ai, ang as f32);
                    if rescale && ang != 0.0 {
                        let s = 1.0 / (ang as f32).to_radians().cos().abs().max(0.1);
                        for (k, sc) in scale.iter_mut().enumerate() {
                            if k != ai {
                                *sc = s;
                            }
                        }
                    }
                } else {
                    for (ai, k) in ["x", "y", "z"].iter().enumerate() {
                        if let Some(a) = r.get(*k).and_then(|v| v.as_f64()) {
                            mat = mm(&rot_axis(ai, a as f32), &mat);
                        }
                    }
                }
                erot = Some((mat, origin, scale));
            }
            let is_full = erot.is_none() && f == [0.0; 3] && t == [16.0; 3];
            let mut faces_seen = 0;
            let Some(faces) = e.get("faces").and_then(|f| f.as_object()) else { continue };
            for (dname, fc) in faces {
                let Some(dir) = dir_index(dname) else { continue };
                let tex = fc.get("texture").and_then(|t| t.as_str()).and_then(|t| self.resolve_tex(m, t));
                let Some((tex, ft)) = tex else { continue };
                *translucent |= ft;
                if is_full {
                    faces_seen += 1;
                    if !self.tex.opaque[tex as usize] {
                        *full_opaque = false;
                    }
                }
                let uv4 = match fc.get("uv").and_then(|u| u.as_array()) {
                    Some(a) if a.len() == 4 => {
                        let g = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
                        [g(0), g(1), g(2), g(3)]
                    }
                    _ => default_uv(dir, f, t),
                };
                let frot = (fc.get("rotation").and_then(|r| r.as_i64()).unwrap_or(0).rem_euclid(360) / 90) as usize;
                let corners = [[uv4[0], uv4[1]], [uv4[0], uv4[3]], [uv4[2], uv4[3]], [uv4[2], uv4[1]]];
                let mut pos = face_verts(dir, f, t);
                let mut uv = [[0f32; 2]; 4];
                for i in 0..4 {
                    uv[i] = corners[(i + frot) % 4];
                }
                for p in pos.iter_mut() {
                    if let Some((mat, o, sc)) = &erot {
                        let q = mv(mat, [p[0] - o[0], p[1] - o[1], p[2] - o[2]]);
                        *p = [q[0] * sc[0] + o[0], q[1] * sc[1] + o[1], q[2] * sc[2] + o[2]];
                    }
                    *p = xf.point(*p);
                }
                let a = [pos[1][0] - pos[0][0], pos[1][1] - pos[0][1], pos[1][2] - pos[0][2]];
                let b = [pos[2][0] - pos[0][0], pos[2][1] - pos[0][1], pos[2][2] - pos[0][2]];
                let mut nrm = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
                let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
                if len < 1e-6 {
                    // Degenerate (e.g. first triangle collapsed): try the other diagonal.
                    let c = [pos[3][0] - pos[0][0], pos[3][1] - pos[0][1], pos[3][2] - pos[0][2]];
                    nrm = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
                }
                let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
                if len < 1e-6 {
                    continue;
                }
                let nrm = [nrm[0] / len, nrm[1] / len, nrm[2] / len];
                let face = face_index_of(nrm);
                if xf.uvlock && xf.rotated && face >= 0 {
                    for i in 0..4 {
                        uv[i] = plane_uv(face as usize, pos[i]);
                    }
                }
                let cull = fc
                    .get("cullface")
                    .and_then(|c| c.as_str())
                    .and_then(dir_index)
                    .map(|d| face_index_of(mv(&xf.m, DIRV[d])))
                    .unwrap_or(-1);
                let shade = if !shade_on {
                    1.0
                } else if let Some(d) = shade_override {
                    shade_of(mv(&xf.m, DIRV[d]))
                } else {
                    shade_of(nrm)
                };
                let color = if fc.get("tintindex").is_some() { tint } else { [255, 255, 255] };
                out.push(Quad {
                    pos: pos.map(|p| [p[0] / 16.0, p[1] / 16.0, p[2] / 16.0]),
                    uv: uv.map(|u| [(u[0] / 16.0).clamp(0.0, 1.0), (u[1] / 16.0).clamp(0.0, 1.0)]),
                    tex,
                    color,
                    cull,
                    face,
                    shade,
                    ao: m.ao,
                });
            }
            if is_full && faces_seen < 6 {
                *full_opaque = false;
            }
        }
    }

    fn applies(&self, bs: &Value, st: &BlockState) -> Vec<(String, f32, f32, bool)> {
        let get = |v: &Value| -> Option<(String, f32, f32, bool)> {
            let v = if let Some(a) = v.as_array() { a.first()? } else { v };
            Some((
                v.get("model")?.as_str()?.to_string(),
                v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                v.get("uvlock").and_then(|x| x.as_bool()).unwrap_or(false),
            ))
        };
        let mut out = vec![];
        if let Some(vars) = bs.get("variants").and_then(|v| v.as_object()) {
            let mut best: Option<(i32, &Value)> = None;
            for (k, v) in vars {
                let mut score = 0;
                for cond in k.split(',').filter(|c| !c.is_empty()) {
                    let Some((pk, pv)) = cond.split_once('=') else { continue };
                    match st.prop(pk) {
                        Some(x) if x == pv => score += 1,
                        Some(_) => score -= 4,
                        None => {}
                    }
                }
                if best.map(|(s, _)| score > s).unwrap_or(true) {
                    best = Some((score, v));
                }
            }
            if let Some((_, v)) = best {
                out.extend(get(v));
            }
        }
        if let Some(parts) = bs.get("multipart").and_then(|m| m.as_array()) {
            for p in parts {
                let ok = match p.get("when") {
                    None => true,
                    Some(w) => when_matches(w, st),
                };
                if ok {
                    if let Some(a) = p.get("apply") {
                        out.extend(get(a));
                    }
                }
            }
        }
        out
    }

    /// -> (quads, occludes, force_translucent)
    fn bake_state(&mut self, st: &BlockState, info: &Info) -> Option<(Vec<Quad>, bool, bool)> {
        let name = st.name.as_str();
        if name.contains(':') {
            return None;
        }
        let bs = self.blockstate(name).or_else(|| {
            let alias = match name {
                "grass_path" => "dirt_path",
                "grass" => "short_grass",
                "chain" => "iron_chain",
                "iron_chain" => "chain",
                "sign" => "oak_sign",
                "wall_sign" => "oak_wall_sign",
                "short_grass" => "grass",
                _ => return None,
            };
            self.blockstate(alias)
        })?;
        let tint = tint_for(st);
        let mut quads = vec![];
        let mut full_opaque = true;
        let mut has_full = false;
        let mut translucent = false;
        for (model, x, y, uvlock) in self.applies(&bs, st) {
            let Some(m) = self.model(&model, 0) else { continue };
            if let Some(els) = &m.elements {
                if els.iter().any(|e| {
                    e.get("rotation").is_none() && arr3(e.get("from")) == Some([0.0; 3]) && arr3(e.get("to")) == Some([16.0; 3])
                }) {
                    has_full = true;
                }
            }
            let xf = Xf::new(x, y, uvlock);
            self.bake_model(&m, &xf, tint, &mut quads, &mut full_opaque, &mut translucent);
        }
        if quads.is_empty() {
            // Block entity (chest, sign, ...): simple boxes, textured with the model's particle texture where it fits.
            let ptex = if particle_fallback(name) {
                self.applies(&bs, st)
                    .first()
                    .and_then(|(m, ..)| self.model(m, 0))
                    .and_then(|m| m.textures.get("particle").map(|t| t.0.clone()))
                    .and_then(|p| self.tex.get(&mut self.jar, &if p.contains(':') { p } else { format!("minecraft:{p}") }))
            } else {
                None
            };
            let q = flat_quads(info, ptex);
            return if q.is_empty() { None } else { Some((q, false, false)) };
        }
        Some((quads, has_full && full_opaque && !translucent, translucent))
    }
}

fn when_matches(w: &Value, st: &BlockState) -> bool {
    let Some(o) = w.as_object() else { return false };
    if let Some(or) = o.get("OR").and_then(|v| v.as_array()) {
        return or.iter().any(|c| when_matches(c, st));
    }
    if let Some(and) = o.get("AND").and_then(|v| v.as_array()) {
        return and.iter().all(|c| when_matches(c, st));
    }
    o.iter().all(|(k, v)| {
        let want = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        };
        match st.prop(k) {
            Some(have) => want.split('|').any(|w| w == have),
            None => false,
        }
    })
}

/// Flat-colour (or single-texture) quads from the simplified box shapes in `blocks`.
fn flat_quads(info: &Info, tex: Option<u16>) -> Vec<Quad> {
    let full = [Bx { min: [0.0; 3], max: [16.0; 3], color: None }];
    let boxes: &[Bx] = match info.kind {
        Kind::Air => return vec![],
        Kind::Full => &full,
        Kind::Boxes => &info.boxes,
    };
    boxes_to_quads(boxes, info, tex)
}

fn boxes_to_quads(boxes: &[Bx], info: &Info, tex: Option<u16>) -> Vec<Quad> {
    let mut out = vec![];
    for b in boxes {
        for dir in 0..6 {
            let pos = face_verts(dir, b.min, b.max);
            let axis = dir / 2;
            let on_boundary = if dir % 2 == 1 { b.max[axis] >= 16.0 } else { b.min[axis] <= 0.0 };
            let base = match (tex, b.color) {
                (Some(_), None) => [255, 255, 255],
                (_, Some(c)) => c,
                (None, None) => info.face_colors.map(|fc| fc[dir]).unwrap_or(info.color),
            };
            out.push(Quad {
                pos: pos.map(|p| [p[0] / 16.0, p[1] / 16.0, p[2] / 16.0]),
                uv: pos.map(|p| {
                    let u = plane_uv(dir, p);
                    [(u[0] / 16.0).clamp(0.0, 1.0), (u[1] / 16.0).clamp(0.0, 1.0)]
                }),
                tex: if b.color.is_some() { 0 } else { tex.unwrap_or(0) },
                color: base,
                cull: if on_boundary { dir as i8 } else { -1 },
                face: dir as i8,
                shade: shade_of(DIRV[dir]),
                ao: true,
            });
        }
    }
    out
}

fn conn_props(conn: Conn, mask: u8) -> Vec<(String, String)> {
    let names = ["north", "east", "south", "west"];
    let mut v = vec![];
    for (i, n) in names.iter().enumerate() {
        let on = mask & (1 << i) != 0;
        let val = match conn {
            Conn::Wall => {
                if on {
                    "low"
                } else {
                    "none"
                }
            }
            _ => {
                if on {
                    "true"
                } else {
                    "false"
                }
            }
        };
        v.push((n.to_string(), val.to_string()));
    }
    if conn == Conn::Wall {
        v.push(("up".into(), "true".into()));
    }
    v
}

fn fluid_of(st: &BlockState) -> (Fluid, bool) {
    match st.name.as_str() {
        "water" | "bubble_column" => (Fluid::Water, true),
        "lava" => (Fluid::Lava, true),
        "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => (Fluid::Water, false),
        _ if st.prop("waterlogged") == Some("true") => (Fluid::Water, false),
        _ => (Fluid::None, false),
    }
}

fn hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Bake every palette entry. Uses the Minecraft jar when available, flat colours otherwise.
pub fn bake_palette(palette: &[BlockState]) -> Bakery {
    let jar_path = if std::env::var_os("SCHEMVIEW_FLAT").is_some() { None } else { find_jar() };
    let mut ctx = jar_path.as_deref().and_then(Jar::open).map(|jar| Ctx {
        jar,
        tex: TexStore::new(),
        models: HashMap::new(),
        states: HashMap::new(),
    });
    let source = if ctx.is_some() {
        jar_path.as_ref().and_then(|p| p.file_stem()).map(|s| format!("textures from Minecraft {}", s.to_string_lossy()))
    } else {
        None
    };
    let mut bakes = Vec::with_capacity(palette.len());
    for st in palette {
        let info = blocks::info_for(st);
        let mut b = Baked::empty();
        if info.kind == Kind::Air {
            bakes.push(b);
            continue;
        }
        b.air = false;
        b.group = hash(&st.name);
        b.conn = info.conn;
        b.is_gate = st.name.ends_with("_fence_gate");
        let (fluid, fluid_only) = fluid_of(st);
        b.fluid = fluid;
        b.fluid_only = fluid_only;
        let name = st.name.as_str();
        if !fluid_only {
            let mut baked_with_jar = false;
            let mut forced_translucent = false;
            if let Some(c) = ctx.as_mut() {
                if let Some(conn) = info.auto_conn {
                    let mut vars = Vec::with_capacity(16);
                    for mask in 0..16u8 {
                        let mut s2 = st.clone();
                        s2.props.retain(|(k, _)| !matches!(k.as_str(), "north" | "east" | "south" | "west" | "up"));
                        s2.props.extend(conn_props(conn, mask));
                        let mut v = Baked::empty();
                        v.air = false;
                        match c.bake_state(&s2, &info) {
                            Some((q, _, ft)) => {
                                v.quads = q;
                                forced_translucent |= ft;
                            }
                            None => {
                                let n = |i: u8| mask & (1 << i) != 0;
                                v.quads = boxes_to_quads(&blocks::conn_boxes(conn, n(0), n(1), n(2), n(3), [false; 4]), &info, None);
                                v.flat = true;
                            }
                        }
                        vars.push(v);
                    }
                    b.conn_variants = Some(vars);
                    baked_with_jar = true;
                } else if let Some((q, occ, ft)) = c.bake_state(st, &info) {
                    b.quads = q;
                    forced_translucent |= ft;
                    b.occludes = occ && !is_translucent(name) && !ft;
                    baked_with_jar = true;
                }
            }
            if !baked_with_jar {
                b.flat = true;
                if let Some(conn) = info.auto_conn {
                    let mut vars = Vec::with_capacity(16);
                    for mask in 0..16u8 {
                        let n = |i: u8| mask & (1 << i) != 0;
                        let mut v = Baked::empty();
                        v.air = false;
                        v.flat = true;
                        v.quads = boxes_to_quads(&blocks::conn_boxes(conn, n(0), n(1), n(2), n(3), [false; 4]), &info, None);
                        vars.push(v);
                    }
                    b.conn_variants = Some(vars);
                } else {
                    b.quads = flat_quads(&info, None);
                    b.occludes = info.occludes();
                }
            }
            b.translucent = if b.flat { info.alpha != 255 } else { is_translucent(name) || forced_translucent };
            b.alpha = if b.flat { info.alpha } else { 255 };
            // Full-cube see-through blocks hide faces against themselves (glass, slime, ice...).
            let full_cube = info.kind == Kind::Full;
            b.self_cull = full_cube && !b.occludes && !name.ends_with("_leaves");
        }
        if std::env::var_os("SCHEMVIEW_BAKE").is_some() {
            eprintln!("BAKE {} flat={} quads={} occ={} trans={} texs={:?}", st.name, b.flat, b.quads.len(), b.occludes, b.translucent, b.quads.iter().map(|q| q.tex).collect::<std::collections::BTreeSet<_>>());
        }
        bakes.push(b);
    }
    let (water_tex, lava_tex, tex) = match ctx {
        Some(mut c) => {
            let w = c.tex.get(&mut c.jar, "minecraft:block/water_still").unwrap_or(0);
            let l = c.tex.get(&mut c.jar, "minecraft:block/lava_still").unwrap_or(0);
            (w, l, c.tex.finish().0)
        }
        None => (0, 0, TexStore::new().finish().0),
    };
    Bakery { bakes, tex, water_tex, lava_tex, source }
}

