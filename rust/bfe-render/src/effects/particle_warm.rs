//! A shared context can specialize particle programs without stalling the
//! window's present queue. Buffers, programs and textures share names across
//! contexts. Vertex arrays and framebuffers belong to the context drawing them.
use super::{blending, check_warm_errors, sides, Blend, EffectsPass, UNIT_EMITTERS, UNIT_TRACKS};
use crate::core::{Core, GlState, MultiDraw, Stats, SHADOW_BINDING, UNIT_MAP};
use glow::{Context, HasContext};

pub struct SharedParticleDraws {
    corners: glow::Buffer,
    indices: glow::Buffer,
    batches: Vec<Material>,
}

struct Material {
    program: glow::Program,
    instances: glow::Buffer,
    corner: u32,
    attributes: Vec<(u32, usize)>,
    stride: usize,
    textures: Vec<(u32, Option<glow::Texture>)>,
    blend: Blend,
    side: u32,
    depth: (bool, u32, bool),
}

impl EffectsPass<Context> {
    /// Capture the immutable resources of the scene being admitted.
    pub fn shared_batches<M: MultiDraw>(
        &self,
        core: &Core<Context, M>,
        handles: impl Iterator<Item = u32>,
    ) -> SharedParticleDraws {
        let batches = handles.map(|id| {
            let b = &self.batches[id as usize];
            let mut textures = b.samplers.clone();
            textures.push((UNIT_MAP, b.texture));
            if let Some(owner) = b.owner.map(|i| &self.owners[i]) {
                textures.extend([(UNIT_TRACKS, Some(owner.tracks)), (UNIT_EMITTERS, Some(owner.emitters))]);
            }
            Material {
                program: core.programs[b.program].program,
                instances: b.vbo,
                corner: b.corner,
                attributes: b.locations.iter().copied().zip(b.sizes.iter().copied()).collect(),
                stride: b.stride,
                textures,
                blend: b.blend,
                side: b.side,
                depth: b.depth.unwrap_or((true, glow::LEQUAL, false)),
            }
        }).collect();
        SharedParticleDraws { corners: self.quad, indices: self.quad_index, batches }
    }
}

impl SharedParticleDraws {
    /// Draw one instance of each material into a private one-pixel target with
    /// the battle target's colour and depth formats. The caller owns the queue.
    pub fn draw(&self, gl: &Context) -> Result<(), String> {
        let _target = Scratch::new(gl)?;
        let mut state = GlState::default();
        let mut stats = Stats::default();
        unsafe {
            gl.viewport(0, 0, 1, 1);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.indices));
            for b in &self.batches {
                gl.use_program(Some(b.program));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.corners));
                gl.enable_vertex_attrib_array(b.corner);
                gl.vertex_attrib_pointer_f32(b.corner, 2, glow::FLOAT, false, 0, 0);
                gl.vertex_attrib_divisor(b.corner, 0);
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.instances));
                let mut offset = 0;
                for &(location, size) in &b.attributes {
                    gl.enable_vertex_attrib_array(location);
                    gl.vertex_attrib_pointer_f32(location, size as i32, glow::FLOAT, false, (b.stride * 4) as i32, offset);
                    gl.vertex_attrib_divisor(location, 1);
                    offset += (size * 4) as i32;
                }
                for &(unit, texture) in &b.textures {
                    state.bind_texture(gl, &mut stats, unit, texture);
                }
                sides(gl, &mut state, &mut stats, b.side, false);
                state.depth_test(gl, &mut stats, b.depth.0);
                state.depth_func(gl, &mut stats, b.depth.1);
                state.depth_write(gl, &mut stats, b.depth.2);
                blending(gl, &mut state, &mut stats, &b.blend);
                gl.draw_elements_instanced(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0, 1);
                for &(location, _) in &b.attributes {
                    gl.disable_vertex_attrib_array(location);
                }
                gl.disable_vertex_attrib_array(b.corner);
                check_warm_errors(gl)?;
            }
            gl.finish();
        }
        check_warm_errors(gl)
    }
}

struct Scratch<'a> {
    gl: &'a Context,
    vertices: Option<glow::VertexArray>,
    target: Option<glow::Framebuffer>,
    attachments: [Option<glow::Renderbuffer>; 2],
    uniforms: Option<glow::Buffer>,
}

impl<'a> Scratch<'a> {
    fn new(gl: &'a Context) -> Result<Self, String> {
        let mut scratch = Self { gl, vertices: None, target: None, attachments: [None; 2], uniforms: None };
        unsafe {
            scratch.vertices = Some(gl.create_vertex_array()?);
            gl.bind_vertex_array(scratch.vertices);
            scratch.target = Some(gl.create_framebuffer()?);
            gl.bind_framebuffer(glow::FRAMEBUFFER, scratch.target);
            let formats = [
                (glow::RGBA8, glow::COLOR_ATTACHMENT0),
                (glow::DEPTH24_STENCIL8, glow::DEPTH_STENCIL_ATTACHMENT),
            ];
            for (slot, (format, attachment)) in scratch.attachments.iter_mut().zip(formats) {
                *slot = Some(gl.create_renderbuffer()?);
                gl.bind_renderbuffer(glow::RENDERBUFFER, *slot);
                gl.renderbuffer_storage(glow::RENDERBUFFER, format, 1, 1);
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, attachment, glow::RENDERBUFFER, *slot);
            }
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("particle loading framebuffer status {status:#x}"));
            }
            scratch.uniforms = Some(gl.create_buffer()?);
            gl.bind_buffer(glow::UNIFORM_BUFFER, scratch.uniforms);
            const WORDS: usize = crate::shader::FRAME_FLOATS;
            const { assert!(WORDS >= crate::shader::SHADOW_FLOATS); }
            gl.buffer_data_u8_slice(glow::UNIFORM_BUFFER, &[0; WORDS * 4], glow::STATIC_DRAW);
            for binding in [0, SHADOW_BINDING] {
                gl.bind_buffer_base(glow::UNIFORM_BUFFER, binding, scratch.uniforms);
            }
        }
        check_warm_errors(gl)?;
        Ok(scratch)
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        unsafe {
            if let Some(vao) = self.vertices { self.gl.delete_vertex_array(vao); }
            if let Some(fb) = self.target { self.gl.delete_framebuffer(fb); }
            for rb in self.attachments.into_iter().flatten() { self.gl.delete_renderbuffer(rb); }
            if let Some(ubo) = self.uniforms { self.gl.delete_buffer(ubo); }
        }
    }
}
