//! Loaders: Litematica (.litematic), MCEdit/Schematica (.schematic), Sponge (.schem v1-3), vanilla structure (.nbt).

use crate::legacy;
use crate::nbt::{self, Tag};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockState {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl BlockState {
    pub fn air() -> Self {
        BlockState { name: "air".into(), props: vec![] }
    }
    pub fn prop(&self, k: &str) -> Option<&str> {
        self.props.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str())
    }
    /// Parse "minecraft:oak_stairs[facing=north,half=bottom]".
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        let (name, rest) = match s.find('[') {
            Some(i) => (&s[..i], Some(&s[i + 1..])),
            None => (s, None),
        };
        let mut props = vec![];
        if let Some(r) = rest {
            let r = r.trim_end_matches(']');
            for kv in r.split(',') {
                if let Some((k, v)) = kv.split_once('=') {
                    props.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
        }
        BlockState { name: normalize(name), props }
    }
}

pub fn normalize(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    match n.strip_prefix("minecraft:") {
        Some(s) => s.to_string(),
        None => n,
    }
}

pub fn is_air_name(n: &str) -> bool {
    matches!(n, "air" | "cave_air" | "void_air" | "" | "structure_void" | "moving_piston" | "light")
}

pub struct Schem {
    /// x, y, z
    pub size: [usize; 3],
    /// palette index per cell, index = (y * size_z + z) * size_x + x. 0 is always air.
    pub blocks: Vec<u16>,
    pub palette: Vec<BlockState>,
    pub format: &'static str,
    pub title: Option<String>,
    pub author: Option<String>,
}

impl Schem {
    #[inline]
    pub fn idx(&self, x: usize, y: usize, z: usize) -> usize {
        (y * self.size[2] + z) * self.size[0] + x
    }
}

const MAX_CELLS: usize = 800_000_000;

struct Builder {
    size: [usize; 3],
    blocks: Vec<u16>,
    palette: Vec<BlockState>,
    lookup: HashMap<BlockState, u16>,
}

impl Builder {
    fn new(size: [usize; 3]) -> Result<Self, String> {
        let cells = size[0].checked_mul(size[1]).and_then(|v| v.checked_mul(size[2])).ok_or("dimensions overflow")?;
        if cells > MAX_CELLS {
            return Err(format!("schematic too large: {}x{}x{} ({} cells)", size[0], size[1], size[2], cells));
        }
        let mut blocks = Vec::new();
        blocks.try_reserve_exact(cells).map_err(|_| "out of memory allocating block grid")?;
        blocks.resize(cells, 0u16);
        let air = BlockState::air();
        let mut lookup = HashMap::new();
        lookup.insert(air.clone(), 0);
        Ok(Builder { size, blocks, palette: vec![air], lookup })
    }
    fn intern(&mut self, mut st: BlockState) -> u16 {
        if is_air_name(&st.name) {
            return 0;
        }
        st.props.sort();
        if let Some(&i) = self.lookup.get(&st) {
            return i;
        }
        if self.palette.len() >= u16::MAX as usize {
            return 0;
        }
        let i = self.palette.len() as u16;
        self.palette.push(st.clone());
        self.lookup.insert(st, i);
        i
    }
    fn finish(self, format: &'static str, title: Option<String>, author: Option<String>) -> Schem {
        Schem { size: self.size, blocks: self.blocks, palette: self.palette, format, title, author }
    }
}

fn compound_state(t: &Tag) -> BlockState {
    let name = t.get("Name").and_then(|n| n.str()).unwrap_or("air");
    let mut props = vec![];
    if let Some(p) = t.get("Properties").and_then(|p| p.compound()) {
        for (k, v) in p {
            let v = match v {
                Tag::String(s) => s.clone(),
                other => other.int().map(|i| i.to_string()).unwrap_or_default(),
            };
            props.push((k.clone(), v));
        }
    }
    BlockState { name: normalize(name), props }
}

fn vec3(t: Option<&Tag>) -> Option<[i64; 3]> {
    let t = t?;
    if let Some(c) = t.compound() {
        return Some([c.get("x")?.int()?, c.get("y")?.int()?, c.get("z")?.int()?]);
    }
    let v = t.ints()?;
    if v.len() >= 3 {
        Some([v[0], v[1], v[2]])
    } else {
        None
    }
}

pub fn load(data: &[u8]) -> Result<Schem, String> {
    let root = nbt::read_file(data)?;
    let mut root = &root;
    // Sponge v3 wraps everything in a "Schematic" compound.
    if let Some(inner) = root.get("Schematic") {
        if inner.compound().is_some() {
            root = inner;
        }
    }
    if root.get("Regions").is_some() {
        return load_litematic(root);
    }
    if let Some(Tag::Compound(_)) = root.get("Blocks") {
        return load_sponge(root, 3);
    }
    if root.get("Palette").is_some() && root.get("BlockData").is_some() {
        return load_sponge(root, 2);
    }
    if root.get("Blocks").and_then(|b| b.bytes()).is_some() {
        return load_mcedit(root);
    }
    if root.get("blocks").is_some() && root.get("size").is_some() {
        return load_structure(root);
    }
    Err("unrecognised schematic format (no Regions / Blocks / Palette / blocks tag)".into())
}

fn load_litematic(root: &Tag) -> Result<Schem, String> {
    let regions = root.get("Regions").and_then(|r| r.compound()).ok_or("Regions is not a compound")?;
    struct R<'a> {
        min: [i64; 3],
        size: [i64; 3],
        tag: &'a Tag,
    }
    let mut names: Vec<&String> = regions.keys().collect();
    names.sort();
    let mut rs = vec![];
    for n in names {
        let t = &regions[n];
        let (Some(pos), Some(sz)) = (vec3(t.get("Position")), vec3(t.get("Size"))) else { continue };
        if sz.iter().any(|&s| s == 0) {
            continue;
        }
        let mut min = [0i64; 3];
        let mut size = [0i64; 3];
        for a in 0..3 {
            min[a] = pos[a] + if sz[a] < 0 { sz[a] + 1 } else { 0 };
            size[a] = sz[a].abs();
        }
        rs.push(R { min, size, tag: t });
    }
    if rs.is_empty() {
        return Err("litematic has no non-empty regions".into());
    }
    let mut gmin = [i64::MAX; 3];
    let mut gmax = [i64::MIN; 3];
    for r in &rs {
        for a in 0..3 {
            gmin[a] = gmin[a].min(r.min[a]);
            gmax[a] = gmax[a].max(r.min[a] + r.size[a]);
        }
    }
    let gsize = [(gmax[0] - gmin[0]) as usize, (gmax[1] - gmin[1]) as usize, (gmax[2] - gmin[2]) as usize];
    let mut b = Builder::new(gsize)?;
    for r in &rs {
        let pal: Vec<u16> = r
            .tag
            .get("BlockStatePalette")
            .and_then(|p| p.list())
            .unwrap_or(&[])
            .iter()
            .map(|e| compound_state(e))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|s| b.intern(s))
            .collect();
        if pal.is_empty() {
            continue;
        }
        let longs = r.tag.get("BlockStates").and_then(|l| l.longs()).unwrap_or(&[]);
        let bits = (64 - ((pal.len() as u64).saturating_sub(1)).leading_zeros()).max(2) as u64;
        let mask = (1u64 << bits) - 1;
        let [sx, sy, sz] = [r.size[0] as usize, r.size[1] as usize, r.size[2] as usize];
        let off = [(r.min[0] - gmin[0]) as usize, (r.min[1] - gmin[1]) as usize, (r.min[2] - gmin[2]) as usize];
        let total = sx * sy * sz;
        let avail = (longs.len() as u64 * 64 / bits) as usize;
        let n = total.min(avail);
        let mut i = 0usize;
        'outer: for y in 0..sy {
            for z in 0..sz {
                let row = b.size[0] * ((y + off[1]) * b.size[2] + (z + off[2])) + off[0];
                for x in 0..sx {
                    if i >= n {
                        break 'outer;
                    }
                    let start = i as u64 * bits;
                    let sl = (start >> 6) as usize;
                    let el = ((start + bits - 1) >> 6) as usize;
                    let o = (start & 63) as u32;
                    let mut v = (longs[sl] as u64) >> o;
                    if el != sl {
                        v |= (longs[el] as u64) << (64 - o);
                    }
                    let v = (v & mask) as usize;
                    i += 1;
                    if let Some(&g) = pal.get(v) {
                        if g != 0 {
                            b.blocks[row + x] = g;
                        }
                    }
                }
            }
        }
    }
    let meta = root.get("Metadata");
    let title = meta.and_then(|m| m.get("Name")).and_then(|n| n.str()).map(String::from);
    let author = meta.and_then(|m| m.get("Author")).and_then(|n| n.str()).map(String::from);
    Ok(b.finish("Litematica", title, author))
}

fn dims_whl(root: &Tag) -> Result<[usize; 3], String> {
    let g = |k: &str| -> Result<usize, String> {
        let v = root.get(k).and_then(|t| t.int()).ok_or(format!("missing {k}"))?;
        // Shorts are unsigned in these formats.
        Ok(if v < 0 { (v as i16 as u16) as usize } else { v as usize })
    };
    Ok([g("Width")?, g("Height")?, g("Length")?])
}

fn load_sponge(root: &Tag, version: u32) -> Result<Schem, String> {
    let size = dims_whl(root)?;
    let (pal_tag, data) = if version >= 3 {
        let blocks = root.get("Blocks").ok_or("missing Blocks")?;
        (blocks.get("Palette"), blocks.get("Data").and_then(|d| d.bytes()))
    } else {
        (root.get("Palette"), root.get("BlockData").and_then(|d| d.bytes()))
    };
    let pal_tag = pal_tag.and_then(|p| p.compound()).ok_or("missing block Palette")?;
    let data = data.ok_or("missing block data")?;
    let mut b = Builder::new(size)?;
    let max = pal_tag.values().filter_map(|v| v.int()).max().unwrap_or(0).clamp(0, 1 << 20) as usize;
    let mut map = vec![0u16; max + 1];
    for (k, v) in pal_tag {
        if let Some(i) = v.int() {
            if i >= 0 && (i as usize) <= max {
                map[i as usize] = b.intern(BlockState::parse(k));
            }
        }
    }
    let total = b.blocks.len();
    let mut p = 0usize;
    let mut i = 0usize;
    while i < total && p < data.len() {
        let mut v: u32 = 0;
        let mut shift = 0;
        loop {
            let byte = data[p];
            p += 1;
            v |= ((byte & 0x7f) as u32) << shift;
            shift += 7;
            if byte & 0x80 == 0 || p >= data.len() || shift > 28 {
                break;
            }
        }
        b.blocks[i] = map.get(v as usize).copied().unwrap_or(0);
        i += 1;
    }
    let meta = root.get("Metadata");
    let title = meta.and_then(|m| m.get("Name")).and_then(|n| n.str()).map(String::from);
    let author = meta.and_then(|m| m.get("Author")).and_then(|n| n.str()).map(String::from);
    Ok(b.finish(if version >= 3 { "Sponge v3" } else { "Sponge v1/v2" }, title, author))
}

fn load_mcedit(root: &Tag) -> Result<Schem, String> {
    let size = dims_whl(root)?;
    let blocks = root.get("Blocks").and_then(|b| b.bytes()).unwrap_or(&[]);
    let datav = root.get("Data").and_then(|b| b.bytes()).unwrap_or(&[]);
    let add = root.get("AddBlocks").or_else(|| root.get("Add")).and_then(|b| b.bytes());
    // Optional Schematica name mapping (id -> name), useful for modded / remapped ids.
    let mut id_names: HashMap<u16, String> = HashMap::new();
    if let Some(m) = root.get("SchematicaMapping").and_then(|m| m.compound()) {
        for (k, v) in m {
            if let Some(i) = v.int() {
                id_names.insert(i as u16, normalize(k));
            }
        }
    }
    let mut b = Builder::new(size)?;
    let total = b.blocks.len();
    let layer = size[0] * size[2];
    let mut cache: HashMap<u32, u16> = HashMap::new();
    let n = total.min(blocks.len());
    let mut ids = vec![0u16; n];
    for i in 0..n {
        let mut id = blocks[i] as u16;
        if let Some(a) = add {
            if let Some(&ab) = a.get(i >> 1) {
                let hi = if i & 1 == 0 { (ab >> 4) & 15 } else { ab & 15 };
                id |= (hi as u16) << 8;
            }
        }
        ids[i] = id;
    }
    for i in 0..n {
        let id = ids[i];
        if id == 0 {
            continue;
        }
        let data = datav.get(i).copied().unwrap_or(0) & 15;
        let lower = if legacy::is_door(id) && data & 8 != 0 && i >= layer && ids[i - layer] == id {
            datav.get(i - layer).copied().unwrap_or(0) & 15
        } else {
            0
        };
        let key = (id as u32) << 8 | (data as u32) << 4 | lower as u32;
        let g = match cache.get(&key) {
            Some(&g) => g,
            None => {
                let st = match id_names.get(&id) {
                    // Modded ids: use the mapped name. Vanilla ids keep full meta decoding.
                    Some(name) if name.contains(':') => {
                        BlockState { name: name.clone(), props: vec![] }
                    }
                    _ => legacy::legacy_state(id, data, lower),
                };
                let g = b.intern(st);
                cache.insert(key, g);
                g
            }
        };
        b.blocks[i] = g;
    }
    Ok(b.finish("MCEdit / Schematica", None, None))
}

fn load_structure(root: &Tag) -> Result<Schem, String> {
    let s = vec3(root.get("size")).ok_or("bad structure size")?;
    let size = [s[0].max(0) as usize, s[1].max(0) as usize, s[2].max(0) as usize];
    let pal_list = root
        .get("palette")
        .and_then(|p| p.list())
        .or_else(|| root.get("palettes").and_then(|p| p.list()).and_then(|l| l.first()).and_then(|p| p.list()))
        .unwrap_or(&[]);
    let mut b = Builder::new(size)?;
    let pal: Vec<u16> = pal_list.iter().map(compound_state).collect::<Vec<_>>().into_iter().map(|s| b.intern(s)).collect();
    for e in root.get("blocks").and_then(|l| l.list()).unwrap_or(&[]) {
        let (Some(p), Some(st)) = (vec3(e.get("pos")), e.get("state").and_then(|s| s.int())) else { continue };
        if (0..3).any(|a| p[a] < 0 || p[a] as usize >= size[a]) {
            continue;
        }
        if let Some(&g) = pal.get(st as usize) {
            let i = b.size[0] * (p[1] as usize * b.size[2] + p[2] as usize) + p[0] as usize;
            b.blocks[i] = g;
        }
    }
    Ok(b.finish("Structure (.nbt)", None, None))
}
