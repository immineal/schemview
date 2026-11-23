//! OpenGL (glow) renderer for the block mesh, drawn inside an egui paint callback.

use crate::assets::TexData;
use crate::mesh::{Mesh, Part, Vert};
use eframe::glow::{self, HasContext};
use std::sync::{mpsc, Arc};

/// Translucent quads (centre + 6 indices) for back-to-front sorting.
struct SortData {
    centers: Vec<[f32; 3]>,
    idx: Vec<[u32; 6]>,
}

fn sorted_indices(d: &SortData, back: [f32; 3]) -> Vec<u32> {
    // Far-to-near: ascending distance along the view direction. Pack (orderable key, index) into u64.
    let mut keys: Vec<u64> = d
        .centers
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let b = (c[0] * back[0] + c[1] * back[1] + c[2] * back[2]).to_bits();
            let ord = if b & 0x8000_0000 != 0 { !b } else { b | 0x8000_0000 };
            ((ord as u64) << 32) | i as u64
        })
        .collect();
    keys.sort_unstable();
    let mut out = Vec::with_capacity(keys.len() * 6);
    for k in keys {
        out.extend_from_slice(&d.idx[(k & 0xffff_ffff) as usize]);
    }
    out
}

/// Sorts big translucent sets on a worker thread so orbiting stays at full frame rate.
struct Sorter {
    data: Arc<SortData>,
    tx: mpsc::Sender<[f32; 3]>,
    rx: mpsc::Receiver<([f32; 3], Vec<u32>)>,
    pending: bool,
}

impl Sorter {
    fn new(data: SortData, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let data = Arc::new(data);
        let (tx, wrx) = mpsc::channel::<[f32; 3]>();
        let (wtx, rx) = mpsc::channel();
        let d2 = data.clone();
        let _ = std::thread::Builder::new().name("sorter".into()).spawn(move || {
            while let Ok(mut dir) = wrx.recv() {
                while let Ok(newer) = wrx.try_recv() {
                    dir = newer;
                }
                if wtx.send((dir, sorted_indices(&d2, dir))).is_err() {
                    break;
                }
                repaint();
            }
        });
        Sorter { data, tx, rx, pending: false }
    }
}

struct Gpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    count: i32,
}

pub struct Renderer {
    program: glow::Program,
    u_mvp: Option<glow::UniformLocation>,
    u_cut: Option<glow::UniformLocation>,
    u_a2c: Option<glow::UniformLocation>,
    u_texsize: Option<glow::UniformLocation>,
    tex_size: f32,
    /// MSAA samples of the window framebuffer (None = not queried yet).
    samples: Option<i32>,
    u_tex: Option<glow::UniformLocation>,
    u_range: Option<glow::UniformLocation>,
    texture: Option<glow::Texture>,
    opaque: Option<Gpu>,
    trans: Option<Gpu>,
    /// Faces closing the layer cut (opaque, translucent), rebuilt whenever the range changes.
    caps: [Option<Gpu>; 2],
    sorter: Option<Sorter>,
    sorted_for: [f32; 3],
    repaint: Arc<dyn Fn() + Send + Sync>,
}

/// Above this many translucent quads, skip per-view sorting (keeps huge builds smooth).
const SORT_LIMIT: usize = 2_000_000;

const VS: &str = r#"
in vec3 a_pos;
in vec2 a_uv;
in float a_layer;
in vec4 a_col;
in float a_y;
uniform mat4 u_mvp;
uniform vec2 u_range;
out vec4 v_col;
out vec3 v_uv;
void main() {
    v_col = a_col;
    v_uv = vec3(a_uv, a_layer);
    // Layer cut: bits 0-13 = block layer, bit 14 = only at the lowest shown layer, bit 15 = only at the highest.
    float y = mod(a_y, 16384.0);
    float flag = floor(a_y / 16384.0);
    bool show = y >= u_range.x && y <= u_range.y
        && (flag < 0.5 || (flag < 1.5 && y == u_range.x) || (flag > 1.5 && y == u_range.y));
    gl_Position = show ? u_mvp * vec4(a_pos, 1.0) : vec4(2.0, 2.0, 2.0, 1.0);
}
"#;
const FS: &str = r#"
precision highp float;
precision highp sampler2DArray;
uniform sampler2DArray u_tex;
uniform float u_cut;
uniform float u_a2c;
uniform float u_texsize;
in vec4 v_col;
in vec3 v_uv;
out vec4 o_col;
void main() {
    vec4 t = texture(u_tex, v_uv);
    vec4 c = t * v_col;
    if (u_cut < 0.1) {
        // Translucent pass: plain blending.
        if (c.a < u_cut) discard;
        o_col = c;
        return;
    }
    // Cutout pass (leaves, glass, plants). Mipmapping thins out the alpha of distant cutout
    // textures; scale it back up by the mip level so leaves keep their coverage.
    vec2 px = v_uv.xy * u_texsize;
    vec2 dx = dFdx(px), dy = dFdy(px);
    float lod = max(0.0, 0.5 * log2(max(dot(dx, dx), dot(dy, dy))));
    float a = t.a * (1.0 + lod * 0.25);
    if (u_a2c > 0.5) {
        // With MSAA: a sharp but anti-aliased edge via alpha-to-coverage instead of a hard
        // threshold, so cutout edges don't sparkle while the view moves.
        a = (a - u_cut) / max(fwidth(a), 0.0001) + 0.5;
        if (a <= 0.0) discard;
        o_col = vec4(c.rgb, clamp(a, 0.0, 1.0));
    } else {
        if (a < u_cut) discard;
        o_col = vec4(c.rgb, 1.0);
    }
}
"#;

impl Renderer {
    pub fn new(gl: &glow::Context, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        unsafe {
            let header = if gl.version().is_embedded { "#version 300 es\n" } else { "#version 330\n" };
            let program = gl.create_program()?;
            let mut shaders = vec![];
            for (ty, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
                let sh = gl.create_shader(ty)?;
                gl.shader_source(sh, &format!("{header}{src}"));
                gl.compile_shader(sh);
                if !gl.get_shader_compile_status(sh) {
                    return Err(format!("shader compile failed: {}", gl.get_shader_info_log(sh)));
                }
                gl.attach_shader(program, sh);
                shaders.push(sh);
            }
            gl.bind_attrib_location(program, 0, "a_pos");
            gl.bind_attrib_location(program, 1, "a_uv");
            gl.bind_attrib_location(program, 2, "a_layer");
            gl.bind_attrib_location(program, 3, "a_col");
            gl.bind_attrib_location(program, 4, "a_y");
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!("shader link failed: {}", gl.get_program_info_log(program)));
            }
            for sh in shaders {
                gl.detach_shader(program, sh);
                gl.delete_shader(sh);
            }
            let u_mvp = gl.get_uniform_location(program, "u_mvp");
            let u_cut = gl.get_uniform_location(program, "u_cut");
            let u_a2c = gl.get_uniform_location(program, "u_a2c");
            let u_texsize = gl.get_uniform_location(program, "u_texsize");
            let u_tex = gl.get_uniform_location(program, "u_tex");
            let u_range = gl.get_uniform_location(program, "u_range");
            if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
                let mut fb = 0;
                if !gl.version().is_embedded {
                    fb = gl.get_framebuffer_attachment_parameter_i32(glow::FRAMEBUFFER, glow::DEPTH, glow::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE);
                }
                eprintln!("GL: {} | {:?} | depth bits {fb}", gl.get_parameter_string(glow::RENDERER), gl.version());
            }
            Ok(Renderer {
                program,
                u_mvp,
                u_cut,
                u_a2c,
                u_texsize,
                tex_size: 16.0,
                samples: None,
                u_tex,
                u_range,
                texture: None,
                opaque: None,
                trans: None,
                caps: [None, None],
                sorter: None,
                sorted_for: [0.0; 3],
                repaint,
            })
        }
    }

    unsafe fn upload(gl: &glow::Context, p: &Part) -> Result<Option<Gpu>, String> {
        if p.idx.is_empty() {
            return Ok(None);
        }
        let vao = gl.create_vertex_array()?;
        gl.bind_vertex_array(Some(vao));
        let vbo = gl.create_buffer()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let vbytes = std::slice::from_raw_parts(p.verts.as_ptr() as *const u8, std::mem::size_of_val(p.verts.as_slice()));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vbytes, glow::STATIC_DRAW);
        let stride = std::mem::size_of::<Vert>() as i32;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 2, glow::UNSIGNED_SHORT, true, stride, 12);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 1, glow::UNSIGNED_SHORT, false, stride, 16);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 4, glow::UNSIGNED_BYTE, true, stride, 20);
        gl.enable_vertex_attrib_array(4);
        gl.vertex_attrib_pointer_f32(4, 1, glow::UNSIGNED_SHORT, false, stride, 18);
        let ibo = gl.create_buffer()?;
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        let ibytes = std::slice::from_raw_parts(p.idx.as_ptr() as *const u8, std::mem::size_of_val(p.idx.as_slice()));
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, ibytes, glow::STATIC_DRAW);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        let err = gl.get_error();
        if err == glow::OUT_OF_MEMORY {
            gl.delete_vertex_array(vao);
            gl.delete_buffer(vbo);
            gl.delete_buffer(ibo);
            return Err("GPU out of memory while uploading mesh".into());
        }
        Ok(Some(Gpu { vao, vbo, ibo, count: p.idx.len() as i32 }))
    }

    unsafe fn upload_textures(&mut self, gl: &glow::Context, t: &TexData) -> Result<(), String> {
        let max_layers = gl.get_parameter_i32(glow::MAX_ARRAY_TEXTURE_LAYERS).max(256) as usize;
        let n = t.layers.len().min(max_layers);
        let s = t.size as i32;
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        let mut data = Vec::with_capacity(t.layers.iter().take(n).map(|l| l.len()).sum());
        for l in t.layers.iter().take(n) {
            data.extend_from_slice(l);
        }
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            glow::RGBA8 as i32,
            s,
            s,
            n as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&data)),
        );
        let levels = (t.size as f32).log2() as i32;
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, levels);
        gl.generate_mipmap(glow::TEXTURE_2D_ARRAY);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MIN_FILTER, glow::NEAREST_MIPMAP_LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        self.texture = Some(tex);
        Ok(())
    }

    pub fn set_mesh(&mut self, gl: &glow::Context, m: &Mesh, t: &TexData) -> Result<(), String> {
        self.clear(gl);
        unsafe {
            self.upload_textures(gl, t)?;
            self.tex_size = t.size as f32;
            self.opaque = Self::upload(gl, &m.opaque)?;
            self.trans = Self::upload(gl, &m.trans)?;
        }
        let nq = m.trans.idx.len() / 6;
        if nq > 0 && nq <= SORT_LIMIT {
            let centers = (0..nq)
                .map(|q| {
                    let mut c = [0f32; 3];
                    for v in &m.trans.verts[q * 4..q * 4 + 4] {
                        for a in 0..3 {
                            c[a] += v.pos[a] * 0.25;
                        }
                    }
                    c
                })
                .collect();
            let idx = m.trans.idx.chunks_exact(6).map(|c| c.try_into().unwrap()).collect();
            self.sorter = Some(Sorter::new(SortData { centers, idx }, self.repaint.clone()));
        }
        self.sorted_for = [0.0; 3];
        Ok(())
    }

    unsafe fn delete(gl: &glow::Context, g: Option<Gpu>) {
        if let Some(g) = g {
            gl.delete_vertex_array(g.vao);
            gl.delete_buffer(g.vbo);
            gl.delete_buffer(g.ibo);
        }
    }

    /// Replace the cut-surface faces (see `mesh::build_caps`).
    pub fn set_caps(&mut self, gl: &glow::Context, m: &Mesh) -> Result<(), String> {
        unsafe {
            for g in [self.caps[0].take(), self.caps[1].take()] {
                Self::delete(gl, g);
            }
            self.caps = [Self::upload(gl, &m.opaque)?, Self::upload(gl, &m.trans)?];
        }
        Ok(())
    }

    pub fn clear(&mut self, gl: &glow::Context) {
        if let Some(t) = self.texture.take() {
            unsafe { gl.delete_texture(t) };
        }
        self.sorter = None;
        for g in [self.opaque.take(), self.trans.take(), self.caps[0].take(), self.caps[1].take()] {
            unsafe { Self::delete(gl, g) };
        }
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        self.clear(gl);
        unsafe { gl.delete_program(self.program) };
    }

    unsafe fn upload_order(&self, gl: &glow::Context, buf: &[u32]) {
        if let Some(g) = &self.trans {
            if buf.len() as i32 != g.count {
                return;
            }
            gl.bind_vertex_array(Some(g.vao));
            let bytes = std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4);
            gl.buffer_sub_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, 0, bytes);
            gl.bind_vertex_array(None);
        }
    }

    fn sort_translucent(&mut self, gl: &glow::Context, back: [f32; 3]) {
        let Some(s) = &mut self.sorter else { return };
        let mut done: Option<([f32; 3], Vec<u32>)> = None;
        while let Ok(r) = s.rx.try_recv() {
            done = Some(r);
            s.pending = false;
        }
        let n = s.data.idx.len();
        if let Some((dir, buf)) = done {
            unsafe { self.upload_order(gl, &buf) };
            self.sorted_for = dir;
        }
        let d = self.sorted_for;
        // Re-sort on practically any turn for small sets; big sets (sorted on a worker) after ~2 degrees.
        let thresh = if n > 50_000 { 0.9994 } else { 0.9999995 };
        if d[0] * back[0] + d[1] * back[1] + d[2] * back[2] > thresh {
            return;
        }
        let Some(s) = &mut self.sorter else { return };
        if n <= 50_000 {
            let buf = sorted_indices(&s.data, back);
            unsafe { self.upload_order(gl, &buf) };
            self.sorted_for = back;
        } else if !s.pending {
            s.pending = s.tx.send(back).is_ok();
        }
    }

    /// `back` = unit vector from the scene towards the viewer; `range` = lowest and highest layer shown.
    pub fn paint(&mut self, gl: &glow::Context, mvp: &[f32; 16], back: [f32; 3], range: [f32; 2]) {
        let t0 = std::time::Instant::now();
        self.sort_translucent(gl, back);
        if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
            eprintln!("paint: sort+upload {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
        }
        unsafe {
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.front_face(glow::CCW);
            gl.disable(glow::BLEND);
            gl.use_program(Some(self.program));
            gl.uniform_matrix_4_f32_slice(self.u_mvp.as_ref(), false, mvp);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, self.texture);
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);
            let samples = *self.samples.get_or_insert_with(|| {
                let n = gl.get_parameter_i32(glow::SAMPLES);
                if std::env::var_os("SCHEMVIEW_DEBUG").is_some() {
                    eprintln!("GL: framebuffer samples {n}");
                }
                n
            });
            let a2c = samples > 1 && std::env::var_os("SCHEMVIEW_NO_A2C").is_none();
            // Never touch the window's alpha channel: a compositor could show the desktop through it.
            gl.color_mask(true, true, true, false);
            gl.uniform_1_f32(self.u_cut.as_ref(), 0.5);
            gl.uniform_1_f32(self.u_a2c.as_ref(), if a2c { 1.0 } else { 0.0 });
            gl.uniform_1_f32(self.u_texsize.as_ref(), self.tex_size);
            gl.uniform_2_f32(self.u_range.as_ref(), range[0], range[1]);
            if a2c {
                gl.enable(glow::SAMPLE_ALPHA_TO_COVERAGE);
            }
            for g in [&self.opaque, &self.caps[0]].into_iter().flatten() {
                gl.bind_vertex_array(Some(g.vao));
                gl.draw_elements(glow::TRIANGLES, g.count, glow::UNSIGNED_INT, 0);
            }
            if a2c {
                gl.disable(glow::SAMPLE_ALPHA_TO_COVERAGE);
            }
            if self.trans.is_some() || self.caps[1].is_some() {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.depth_mask(false);
                gl.uniform_1_f32(self.u_cut.as_ref(), 0.004);
                for g in [&self.trans, &self.caps[1]].into_iter().flatten() {
                    gl.bind_vertex_array(Some(g.vao));
                    gl.draw_elements(glow::TRIANGLES, g.count, glow::UNSIGNED_INT, 0);
                }
                gl.depth_mask(true);
            }
            gl.color_mask(true, true, true, true);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
        }
    }
}
