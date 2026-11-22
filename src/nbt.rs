//! Minimal, defensive NBT reader (big-endian Java edition format).

use std::collections::HashMap;
use std::io::Read;

#[derive(Debug, Clone)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(HashMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(m) => m.get(key),
            _ => None,
        }
    }
    pub fn compound(&self) -> Option<&HashMap<String, Tag>> {
        match self {
            Tag::Compound(m) => Some(m),
            _ => None,
        }
    }
    pub fn list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }
    /// Any integer-like tag as i64.
    pub fn int(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            Tag::Float(v) => Some(v as i64),
            Tag::Double(v) => Some(v as i64),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Tag::ByteArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn longs(&self) -> Option<&[i64]> {
        match self {
            Tag::LongArray(v) => Some(v),
            _ => None,
        }
    }
    /// Int array or list of ints, as Vec<i64>.
    pub fn ints(&self) -> Option<Vec<i64>> {
        match self {
            Tag::IntArray(v) => Some(v.iter().map(|&x| x as i64).collect()),
            Tag::List(v) => v.iter().map(|t| t.int()).collect(),
            Tag::ByteArray(v) => Some(v.iter().map(|&x| x as i8 as i64).collect()),
            Tag::LongArray(v) => Some(v.clone()),
            _ => None,
        }
    }
}

/// Decompress (gzip / zlib / raw) and parse the root compound.
pub fn read_file(data: &[u8]) -> Result<Tag, String> {
    let raw: Vec<u8>;
    let bytes: &[u8] = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        let mut out = Vec::with_capacity(data.len() * 8);
        flate2::read::MultiGzDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| format!("gzip decompression failed: {e}"))?;
        raw = out;
        &raw
    } else if data.len() >= 2 && data[0] == 0x78 && (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 == 0 {
        let mut out = Vec::with_capacity(data.len() * 8);
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib decompression failed: {e}"))?;
        raw = out;
        &raw
    } else {
        data
    };
    let mut r = Reader { b: bytes, p: 0 };
    let ty = r.u8()?;
    if ty != 10 {
        return Err(format!("not an NBT file (root tag type {ty}, expected compound)"));
    }
    let _name = r.string()?;
    r.payload(10, 0)
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

const MAX_DEPTH: u32 = 512;

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.b.len() - self.p {
            return Err("unexpected end of NBT data (file truncated or corrupt)".into());
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        let s = self.take(2)?;
        Ok(i16::from_be_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        let s = self.take(4)?;
        Ok(i32::from_be_bytes(s.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        let s = self.take(8)?;
        Ok(i64::from_be_bytes(s.try_into().unwrap()))
    }
    fn len(&mut self, elem: usize) -> Result<usize, String> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(0);
        }
        let n = n as usize;
        if n.saturating_mul(elem.max(1)) > self.b.len() - self.p {
            return Err("NBT array length exceeds data (corrupt file)".into());
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.i16()? as u16 as usize;
        let s = self.take(n)?;
        Ok(String::from_utf8_lossy(s).into_owned())
    }
    fn payload(&mut self, ty: u8, depth: u32) -> Result<Tag, String> {
        if depth > MAX_DEPTH {
            return Err("NBT nesting too deep".into());
        }
        Ok(match ty {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len(1)?;
                Tag::ByteArray(self.take(n)?.to_vec())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let et = self.u8()?;
                let n = self.len(if et == 0 { 0 } else { 1 })?;
                let mut v = Vec::with_capacity(n.min(1 << 20));
                if et != 0 {
                    for _ in 0..n {
                        v.push(self.payload(et, depth + 1)?);
                    }
                }
                Tag::List(v)
            }
            10 => {
                let mut m = HashMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    let v = self.payload(t, depth + 1)?;
                    m.insert(k, v);
                }
                Tag::Compound(m)
            }
            11 => {
                let n = self.len(4)?;
                let s = self.take(n * 4)?;
                Tag::IntArray(s.chunks_exact(4).map(|c| i32::from_be_bytes(c.try_into().unwrap())).collect())
            }
            12 => {
                let n = self.len(8)?;
                let s = self.take(n * 8)?;
                Tag::LongArray(s.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
            }
            t => return Err(format!("unknown NBT tag type {t} (corrupt file)")),
        })
    }
}
