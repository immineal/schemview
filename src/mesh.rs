//! Builds face-culled, AO-shaded, textured triangle meshes from a schematic (multi-threaded).

use crate::assets::{Baked, Bakery, Fluid, Quad};
use crate::blocks::Conn;
use crate::formats::Schem;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Vert {
    pub pos: [f32; 3],
    pub uv: [u16; 2],
    pub layer: u16,
    /// Block layer (bits 0-13) and cap flags: CAP_DOWN = only drawn when this layer is the lowest shown,
    /// CAP_UP = only drawn when it is the highest shown. The vertex shader hides layers outside the range.
    pub y: u16,
    pub col: [u8; 4],
    /// The face's UV rect in 1/256 of the texture (min u, min v, 256 - max u, 256 - max v), rounded
    /// inwards, so texture smoothing never reaches past it. Model UVs sit on that grid.
    pub rect: [u8; 4],
}

#[derive(Default)]
pub struct Part {
    pub verts: Vec<Vert>,
    pub idx: Vec<u32>,
}

impl Part {
    fn append(&mut self, mut o: Part) {
        let base = self.verts.len() as u32;
        self.verts.append(&mut o.verts);
        self.idx.extend(o.idx.iter().map(|i| i + base));
    }
}

pub struct Mesh {
    pub opaque: Part,
    pub trans: Part,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub blocks: u64,
}

const NORMALS: [[i64; 3]; 6] = [[-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1]];
const SHADE: [f32; 6] = [0.6, 0.6, 0.5, 1.0, 0.8, 0.8];
const AO: [f32; 4] = [0.5, 0.67, 0.83, 1.0];
pub const Y_MASK: u16 = 0x3fff;
pub const CAP_DOWN: u16 = 0x4000;
pub const CAP_UP: u16 = 0x8000;
const WATER_TINT: [u8; 3] = [63, 118, 228];

struct Ctx<'a> {
    s: &'a Schem,
    bk: &'a Bakery,
    /// Layers outside [clip0, clip1] count as air (used for the faces exposed by the layer cut).
    clip: [i64; 2],
}

impl Ctx<'_> {
    #[inline]
    fn cell(&self, x: i64, y: i64, z: i64) -> Option<&Baked> {
        let [sx, sy, sz] = self.s.size;
        if x < 0 || y < self.clip[0] || z < 0 || x >= sx as i64 || y >= sy as i64 || y > self.clip[1] || z >= sz as i64 {
            return None;
        }
        Some(&self.bk.bakes[self.s.blocks[self.s.idx(x as usize, y as usize, z as usize)] as usize])
    }
    #[inline]
    fn occ(&self, p: [i64; 3]) -> bool {
        self.cell(p[0], p[1], p[2]).map(|b| b.occludes).unwrap_or(false)
    }
}

#[inline]
fn noise(x: usize, y: usize, z: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663) ^ (z as u32).wrapping_mul(83492791);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    0.955 + (h & 0xff) as f32 / 255.0 * 0.09
}

pub fn build(s: &Schem, bk: &Bakery) -> Mesh {
    let ctx = Ctx { s, bk, clip: [0, i64::MAX] };
    let sy = s.size[1];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 32).min(sy.max(1));
    let per = sy.div_ceil(threads.max(1)).max(1);
    let results: Vec<(Part, Part, [i64; 6], u64)> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let ctx = &ctx;
                sc.spawn(move || mesh_range(ctx, t * per, ((t + 1) * per).min(sy)))
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("mesher thread panicked")).collect()
    });
    let mut opaque = Part::default();
    let mut trans = Part::default();
    let mut bb = [i64::MAX, i64::MAX, i64::MAX, i64::MIN, i64::MIN, i64::MIN];
    let mut blocks = 0;
    opaque.verts.reserve(results.iter().map(|r| r.0.verts.len()).sum());
    opaque.idx.reserve(results.iter().map(|r| r.0.idx.len()).sum());
    for (o, t, b, n) in results {
        opaque.append(o);
        trans.append(t);
        for a in 0..3 {
            bb[a] = bb[a].min(b[a]);
            bb[a + 3] = bb[a + 3].max(b[a + 3]);
        }
        blocks += n;
    }
    if blocks == 0 {
        bb = [0, 0, 0, s.size[0] as i64, s.size[1] as i64, s.size[2] as i64];
    }
    Mesh {
        opaque,
        trans,
        min: [bb[0] as f32, bb[1] as f32, bb[2] as f32],
        max: [bb[3] as f32, bb[4] as f32, bb[5] as f32],
        blocks,
    }
}

/// Quads of a block; fences / panes / walls without properties connect to their neighbours here.
fn geometry<'a>(c: &Ctx, b: &'a Baked, cell: [i64; 3]) -> (&'a [Quad], bool) {
    match (&b.conn_variants, b.conn) {
        (Some(vars), Some(conn)) => {
            let link = |dx: i64, dz: i64| -> bool {
                c.cell(cell[0] + dx, cell[1], cell[2] + dz)
                    .map(|n| n.occludes || n.conn == Some(conn) || (conn != Conn::Pane && n.is_gate))
                    .unwrap_or(false)
            };
            let mask = link(0, -1) as usize | (link(1, 0) as usize) << 1 | (link(0, 1) as usize) << 2 | (link(-1, 0) as usize) << 3;
            let v = &vars[mask];
            (&v.quads, v.flat)
        }
        _ => (&b.quads, b.flat),
    }
}

#[inline]
fn culled(c: &Ctx, b: &Baked, cell: [i64; 3], face: usize) -> bool {
    let n = NORMALS[face];
    match c.cell(cell[0] + n[0], cell[1] + n[1], cell[2] + n[2]) {
        Some(nb) => nb.occludes || (b.self_cull && nb.self_cull && nb.group == b.group),
        None => false,
    }
}

#[inline]
fn ytag(y: i64) -> u16 {
    (y.max(0) as u64).min(Y_MASK as u64) as u16
}

/// The faces that the full mesh culls against the layer above `hi` (tagged CAP_UP) and below `lo`
/// (tagged CAP_DOWN). With the layers outside [lo, hi] hidden, these close the cut surfaces.
pub fn build_caps(s: &Schem, bk: &Bakery, lo: usize, hi: usize) -> Mesh {
    let full = Ctx { s, bk, clip: [0, i64::MAX] };
    let cut = Ctx { s, bk, clip: [lo as i64, hi as i64] };
    let mut op = Part::default();
    let mut tr = Part::default();
    let [sx, sy, sz] = s.size;
    for (y, face, flag) in [(hi, 3usize, CAP_UP), (lo, 2usize, CAP_DOWN)] {
        if y >= sy || (face == 3 && y + 1 >= sy) || (face == 2 && y == 0) {
            continue;
        }
        let tag = ytag(y as i64) | flag;
        for z in 0..sz {
            let row = s.idx(0, y, z);
            for x in 0..sx {
                let b = &bk.bakes[s.blocks[row + x] as usize];
                if b.air {
                    continue;
                }
                let cell = [x as i64, y as i64, z as i64];
                let (quads, flat) = geometry(&full, b, cell);
                let nz = if flat { noise(x, y, z) } else { 1.0 };
                let part = if b.translucent { &mut tr } else { &mut op };
                if culled(&full, b, cell, face) {
                    for q in quads.iter().filter(|q| q.cull == face as i8) {
                        emit(&cut, part, cell, q, nz, b.alpha, tag);
                    }
                }
                if b.fluid != Fluid::None {
                    // Fluid top/bottom hidden by the same fluid (or a solid block underneath).
                    let n = NORMALS[face];
                    let np = [cell[0], cell[1] + n[1], cell[2]];
                    let same = full.cell(np[0], np[1], np[2]).map(|nb| nb.fluid == b.fluid).unwrap_or(false);
                    if same || (face == 2 && full.occ(np)) {
                        let h = if face == 3 { 1.0 } else if same_fluid_above(&full, cell, b.fluid) { 1.0 } else { 14.0 / 16.0 };
                        emit_fluid_face(&cut, &mut op, &mut tr, cell, b.fluid, face, h, tag);
                    }
                }
            }
        }
    }
    Mesh { opaque: op, trans: tr, min: [0.0; 3], max: [0.0; 3], blocks: 0 }
}

fn same_fluid_above(c: &Ctx, cell: [i64; 3], fl: Fluid) -> bool {
    c.cell(cell[0], cell[1] + 1, cell[2]).map(|b| b.fluid == fl).unwrap_or(false)
}

fn mesh_range(c: &Ctx, y0: usize, y1: usize) -> (Part, Part, [i64; 6], u64) {
    let mut op = Part::default();
    let mut tr = Part::default();
    let mut bb = [i64::MAX, i64::MAX, i64::MAX, i64::MIN, i64::MIN, i64::MIN];
    let mut count = 0u64;
    let [sx, _, sz] = c.s.size;
    for y in y0..y1 {
        for z in 0..sz {
            let row = c.s.idx(0, y, z);
            for x in 0..sx {
                let b = &c.bk.bakes[c.s.blocks[row + x] as usize];
                if b.air {
                    continue;
                }
                count += 1;
                let cell = [x as i64, y as i64, z as i64];
                bb[0] = bb[0].min(cell[0]);
                bb[1] = bb[1].min(cell[1]);
                bb[2] = bb[2].min(cell[2]);
                bb[3] = bb[3].max(cell[0] + 1);
                bb[4] = bb[4].max(cell[1] + 1);
                bb[5] = bb[5].max(cell[2] + 1);

                let (quads, flat) = geometry(c, b, cell);
                let nz = if flat { noise(x, y, z) } else { 1.0 };
                let part = if b.translucent { &mut tr } else { &mut op };
                let tag = ytag(cell[1]);
                for q in quads {
                    if q.cull >= 0 && culled(c, b, cell, q.cull as usize) {
                        continue;
                    }
                    emit(c, part, cell, q, nz, b.alpha, tag);
                }

                if b.fluid != Fluid::None {
                    emit_fluid(c, &mut op, &mut tr, cell, b.fluid, tag);
                }
            }
        }
    }
    (op, tr, bb, count)
}

fn emit(c: &Ctx, part: &mut Part, cell: [i64; 3], q: &Quad, nz: f32, alpha: u8, tag: u16) {
    let mut ao = [3u8; 4];
    if q.ao && q.face >= 0 {
        let f = q.face as usize;
        let axis = f / 2;
        let positive = f % 2 == 1;
        let plane = q.pos[0][axis];
        let boundary = if positive { plane >= 0.999 } else { plane <= 0.001 };
        let mut layer = cell;
        if boundary {
            for a in 0..3 {
                layer[a] += NORMALS[f][a];
            }
        }
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let side = |t: f32| -> i64 {
            if t <= 0.001 {
                -1
            } else if t >= 0.999 {
                1
            } else {
                0
            }
        };
        for k in 0..4 {
            let du = side(q.pos[k][u]);
            let dv = side(q.pos[k][v]);
            let mut p1 = layer;
            p1[u] += du;
            let mut p2 = layer;
            p2[v] += dv;
            let mut p3 = layer;
            p3[u] += du;
            p3[v] += dv;
            let s1 = (du != 0 && c.occ(p1)) as u8;
            let s2 = (dv != 0 && c.occ(p2)) as u8;
            let cc = (du != 0 && dv != 0 && c.occ(p3)) as u8;
            ao[k] = if s1 == 1 && s2 == 1 { 0 } else { 3 - (s1 + s2 + cc) };
        }
    }
    let base = part.verts.len() as u32;
    // Quantize UVs towards the middle of the face's UV rect: plain truncation can move an edge
    // a hair into the neighbouring (maybe transparent) texel, e.g. 6/16 -> texel 5.99994.
    let mid = [0, 1].map(|a| q.uv.iter().map(|u| u[a]).sum::<f32>() * 0.25);
    let unorm = |u: f32, m: f32| -> u16 {
        let x = u * 65535.0;
        (if u < m - 1e-6 { x.ceil() } else if u > m + 1e-6 { x.floor() } else { x.round() }).clamp(0.0, 65535.0) as u16
    };
    let lo = [0, 1].map(|a| q.uv.iter().map(|u| u[a]).fold(f32::MAX, f32::min));
    let hi = [0, 1].map(|a| q.uv.iter().map(|u| u[a]).fold(f32::MIN, f32::max));
    let in256 = |d: f32| (d * 256.0 - 1e-3).ceil().clamp(0.0, 255.0) as u8;
    let rect = [in256(lo[0]), in256(lo[1]), in256(1.0 - hi[0]), in256(1.0 - hi[1])];
    for k in 0..4 {
        let l = q.shade * AO[ao[k] as usize] * nz;
        let cl = |x: u8| (x as f32 * l).clamp(0.0, 255.0) as u8;
        part.verts.push(Vert {
            pos: [cell[0] as f32 + q.pos[k][0], cell[1] as f32 + q.pos[k][1], cell[2] as f32 + q.pos[k][2]],
            uv: [unorm(q.uv[k][0], mid[0]), unorm(q.uv[k][1], mid[1])],
            layer: q.tex,
            y: tag,
            col: [cl(q.color[0]), cl(q.color[1]), cl(q.color[2]), alpha],
            rect,
        });
    }
    let flip = (ao[0] as u32 + ao[2] as u32) < (ao[1] as u32 + ao[3] as u32);
    let t: [u32; 6] = if flip { [1, 2, 3, 1, 3, 0] } else { [0, 1, 2, 0, 2, 3] };
    part.idx.extend(t.iter().map(|i| base + i));
}

/// Water / lava as a slightly lowered box, culled against solid blocks and the same fluid.
fn emit_fluid(c: &Ctx, op: &mut Part, tr: &mut Part, cell: [i64; 3], fl: Fluid, tag: u16) {
    let same = |p: [i64; 3]| c.cell(p[0], p[1], p[2]).map(|b| b.fluid == fl).unwrap_or(false);
    let h = if same_fluid_above(c, cell, fl) { 1.0 } else { 14.0 / 16.0 };
    for f in 0..6 {
        let n = NORMALS[f];
        let np = [cell[0] + n[0], cell[1] + n[1], cell[2] + n[2]];
        if same(np) || ((f != 3 || h >= 1.0) && c.occ(np)) {
            continue;
        }
        emit_fluid_face(c, op, tr, cell, fl, f, h, tag);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_fluid_face(c: &Ctx, op: &mut Part, tr: &mut Part, cell: [i64; 3], fl: Fluid, f: usize, h: f32, tag: u16) {
    let textured = match fl {
        Fluid::Water => c.bk.water_tex != 0,
        _ => c.bk.lava_tex != 0,
    };
    let (tex, tint, alpha, part) = match fl {
        Fluid::Water => (c.bk.water_tex, if textured { WATER_TINT } else { [52, 92, 220] }, if textured { 255 } else { 150 }, tr),
        _ => (c.bk.lava_tex, if textured { [255, 255, 255] } else { [207, 92, 15] }, 255, op),
    };
    let pos = fluid_face(f, h);
    let axis = f / 2;
    let q = Quad {
        pos,
        uv: pos.map(|p| match axis {
            1 => [p[0], p[2]],
            0 => [p[2], 1.0 - p[1]],
            _ => [p[0], 1.0 - p[1]],
        }),
        tex,
        color: tint,
        cull: -1,
        face: f as i8,
        shade: if fl == Fluid::Lava { 1.0 } else { SHADE[f] },
        ao: false,
    };
    emit(c, part, cell, &q, 1.0, alpha, tag);
}

fn fluid_face(f: usize, h: f32) -> [[f32; 3]; 4] {
    let (a, b) = ([0.0f32, 0.0, 0.0], [1.0f32, h, 1.0]);
    match f {
        2 => [[a[0], a[1], b[2]], [a[0], a[1], a[2]], [b[0], a[1], a[2]], [b[0], a[1], b[2]]],
        3 => [[a[0], b[1], a[2]], [a[0], b[1], b[2]], [b[0], b[1], b[2]], [b[0], b[1], a[2]]],
        4 => [[b[0], b[1], a[2]], [b[0], a[1], a[2]], [a[0], a[1], a[2]], [a[0], b[1], a[2]]],
        5 => [[a[0], b[1], b[2]], [a[0], a[1], b[2]], [b[0], a[1], b[2]], [b[0], b[1], b[2]]],
        0 => [[a[0], b[1], a[2]], [a[0], a[1], a[2]], [a[0], a[1], b[2]], [a[0], b[1], b[2]]],
        _ => [[b[0], b[1], b[2]], [b[0], a[1], b[2]], [b[0], a[1], a[2]], [b[0], b[1], a[2]]],
    }
}
