//! Native GL submission adapter. Image and target lifetime belongs to bfe-render.
pub use bfe_render::resources::GpuResources as Resources;
use glow::HasContext;

/// Sorted refraction copies must preserve host framebuffer handles. GLES has
/// no multi-draw entrypoint in glow; issuing the retained ranges is equivalent.
pub struct Ranges {
    gl: glow::Context,
    saved: (Option<glow::Framebuffer>, Option<glow::Framebuffer>),
}
impl Ranges {
    pub fn new(gl: glow::Context) -> Self {
        Self {
            gl,
            saved: (None, None),
        }
    }
}
impl bfe_render::core::MultiDraw for Ranges {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], ty: u32, offsets: &mut [i32]) {
        for (&count, &offset) in counts.iter().zip(offsets.iter()) {
            unsafe {
                self.gl.draw_elements(mode, count, ty, offset);
            }
        }
    }
    fn save_framebuffers(&mut self) -> bool {
        unsafe {
            self.saved = (
                self.gl
                    .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                self.gl
                    .get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            );
        }
        true
    }
    fn read_saved_draw_framebuffer(&self) {
        unsafe {
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, self.saved.0);
        }
    }
    fn restore_framebuffers(&self) {
        unsafe {
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.saved.0);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, self.saved.1);
        }
    }
}
