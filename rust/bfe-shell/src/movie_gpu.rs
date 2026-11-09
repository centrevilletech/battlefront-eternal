//! Plane upload, BT.601 conversion, Catmull-Rom upscale and RCAS at strength 0.7.
//! Upscale and RCAS equations port src/shell/video-sharpen.js (AMD MIT notice).
use crate::movies::{MoviePlayer, MAX_LAYERS};
use glow::HasContext;

struct Slot {
    planes: [glow::Texture; 3],
    rgb: glow::Texture,
    scaled: glow::Texture,
    dimensions: [i32; 2],
    output: [i32; 2],
    serial: u64,
}
/// One composited texture can be drawn by Classic or both page side backdrops.
/// Its rows match ordinary interface textures. The caller resets its GL cache
/// after this pass, since uploads intentionally bypass the battle state cache.
pub struct MovieTexture {
    gl: glow::Context,
    slots: [Slot; MAX_LAYERS],
    pub texture: glow::Texture,
    size: [i32; 2],
    target: glow::Framebuffer,
    vao: glow::VertexArray,
    yuv: glow::Program,
    upscale: glow::Program,
    sharpen: glow::Program,
    backdrop: glow::Program,
    yuv_uniforms: [Option<glow::UniformLocation>; 3],
    upscale_uniforms: [Option<glow::UniformLocation>; 4],
    sharpen_uniforms: [Option<glow::UniformLocation>; 3],
}
impl MovieTexture {
    pub fn new(gl: glow::Context) -> Result<Self, String> {
        unsafe {
            let yuv = program(&gl, include_str!("movie_shaders/yuv.glsl"))?;
            let upscale = program(&gl, include_str!("movie_shaders/upscale.glsl"))?;
            let sharpen = program(&gl, include_str!("movie_shaders/sharpen.glsl"))?;
            let backdrop = program(&gl, include_str!("movie_shaders/backdrop.glsl"))?;
            let mut slots = Vec::with_capacity(MAX_LAYERS);
            for _ in 0..MAX_LAYERS {
                slots.push(Slot {
                    planes: [texture(&gl)?, texture(&gl)?, texture(&gl)?],
                    rgb: texture(&gl)?,
                    scaled: texture(&gl)?,
                    dimensions: [0; 2],
                    output: [0; 2],
                    serial: 0,
                });
            }
            Ok(Self {
                texture: texture(&gl)?,
                target: gl.create_framebuffer()?,
                vao: gl.create_vertex_array()?,
                slots: slots.try_into().ok().unwrap(),
                size: [0; 2],
                yuv_uniforms: ["planeY", "planeU", "planeV"]
                    .map(|name| gl.get_uniform_location(yuv, name)),
                upscale_uniforms: ["source", "flip", "sourceSize", "crop"]
                    .map(|name| gl.get_uniform_location(upscale, name)),
                sharpen_uniforms: ["source", "sharpness", "opacity"]
                    .map(|name| gl.get_uniform_location(sharpen, name)),
                yuv,
                upscale,
                sharpen,
                backdrop,
                gl,
            })
        }
    }
    pub fn draw(&mut self, player: &MoviePlayer, size: [i32; 2]) {
        let size = [size[0].clamp(1, 4096), size[1].clamp(1, 4096)];
        let gl = &self.gl;
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);
            gl.color_mask(true, true, true, true);
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.target));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            for (i, layer) in player.layers.iter().enumerate() {
                let Some(layer) = layer else {
                    continue;
                };
                let Some(frame) = layer.frame.as_ref() else {
                    continue;
                };
                let slot = &mut self.slots[i];
                let dims = [frame.width as i32, frame.height as i32];
                let dirty = layer.changed || slot.serial != layer.serial || slot.output != size;
                if !dirty {
                    continue;
                }
                for plane in 0..3 {
                    gl.active_texture(glow::TEXTURE0 + plane as u32);
                    gl.bind_texture(glow::TEXTURE_2D, Some(slot.planes[plane]));
                    let [w, h] = if plane == 0 {
                        dims
                    } else {
                        [(dims[0] + 1) / 2, (dims[1] + 1) / 2]
                    };
                    if slot.dimensions != dims {
                        allocate(gl, glow::R8 as i32, glow::RED, [w, h]);
                    }
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        w,
                        h,
                        glow::RED,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&frame.planes[plane])),
                    );
                }
                gl.use_program(Some(self.yuv));
                for unit in 0..3 {
                    gl.uniform_1_i32(self.yuv_uniforms[unit].as_ref(), unit as i32);
                }
                gl.active_texture(glow::TEXTURE3);
                if slot.dimensions != dims {
                    gl.bind_texture(glow::TEXTURE_2D, Some(slot.rgb));
                    allocate(gl, glow::RGBA8 as i32, glow::RGBA, dims);
                }
                attach(gl, slot.rgb);
                gl.viewport(0, 0, dims[0], dims[1]);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                if slot.output != size {
                    gl.bind_texture(glow::TEXTURE_2D, Some(slot.scaled));
                    allocate(gl, glow::RGBA8 as i32, glow::RGBA, size);
                }
                attach(gl, slot.scaled);
                gl.viewport(0, 0, size[0], size[1]);
                gl.use_program(Some(self.upscale));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(slot.rgb));
                gl.uniform_1_i32(self.upscale_uniforms[0].as_ref(), 0);
                gl.uniform_1_i32(self.upscale_uniforms[1].as_ref(), 0);
                gl.uniform_2_f32(
                    self.upscale_uniforms[2].as_ref(),
                    dims[0] as f32,
                    dims[1] as f32,
                );
                let source_aspect = dims[0] as f32 / dims[1] as f32;
                let target_aspect = size[0] as f32 / size[1] as f32;
                gl.uniform_2_f32(
                    self.upscale_uniforms[3].as_ref(),
                    (target_aspect / source_aspect).min(1.),
                    (source_aspect / target_aspect).min(1.),
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                slot.dimensions = dims;
                slot.output = size;
                slot.serial = layer.serial;
            }
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            if self.size != size {
                allocate(gl, glow::RGBA8 as i32, glow::RGBA, size);
                self.size = size;
            }
            attach(gl, self.texture);
            gl.viewport(0, 0, size[0], size[1]);
            gl.use_program(Some(self.backdrop));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.use_program(Some(self.sharpen));
            gl.uniform_1_i32(self.sharpen_uniforms[0].as_ref(), 0);
            gl.uniform_1_f32(self.sharpen_uniforms[1].as_ref(), 2f32.powf(-0.6));
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            for i in player.draw_order() {
                let Some(layer) = player.layers[i].as_ref() else {
                    continue;
                };
                if layer.frame.is_none() {
                    continue;
                }
                gl.bind_texture(glow::TEXTURE_2D, Some(self.slots[i].scaled));
                gl.uniform_1_f32(self.sharpen_uniforms[2].as_ref(), layer.opacity as f32);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
            gl.bind_vertex_array(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.active_texture(glow::TEXTURE0);
        }
    }
}
unsafe fn allocate(gl: &glow::Context, internal: i32, format: u32, size: [i32; 2]) {
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        internal,
        size[0],
        size[1],
        0,
        format,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(None),
    );
}
unsafe fn attach(gl: &glow::Context, texture: glow::Texture) {
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(texture),
        0,
    );
}
unsafe fn texture(gl: &glow::Context) -> Result<glow::Texture, String> {
    let texture = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
    for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
        gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::LINEAR as i32);
    }
    for parameter in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
        gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::CLAMP_TO_EDGE as i32);
    }
    Ok(texture)
}
unsafe fn program(gl: &glow::Context, fragment: &str) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    for (kind, source) in [
        (
            glow::VERTEX_SHADER,
            include_str!("movie_shaders/vertex.glsl"),
        ),
        (glow::FRAGMENT_SHADER, fragment),
    ] {
        let shader = gl.create_shader(kind)?;
        let source = if gl.version().is_embedded {
            source.to_string()
        } else {
            source
                .replace("#version 300 es", "#version 330 core")
                .replace("precision highp float;", "")
        };
        gl.shader_source(shader, &source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            gl.delete_program(program);
            return Err(error);
        }
        gl.attach_shader(program, shader);
        gl.delete_shader(shader);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let error = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(error);
    }
    Ok(program)
}
impl Drop for MovieTexture {
    fn drop(&mut self) {
        unsafe {
            for slot in &self.slots {
                for tex in slot.planes {
                    self.gl.delete_texture(tex);
                }
                self.gl.delete_texture(slot.rgb);
                self.gl.delete_texture(slot.scaled);
            }
            self.gl.delete_texture(self.texture);
            self.gl.delete_framebuffer(self.target);
            self.gl.delete_vertex_array(self.vao);
            for program in [self.yuv, self.upscale, self.sharpen, self.backdrop] {
                self.gl.delete_program(program);
            }
        }
    }
}
