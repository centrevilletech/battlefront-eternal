//! Own the canvas storage independently of the window's pixel ownership test.
//! X11 may discard fragments behind another window. Battles render into this
//! fixed linear target, then present it to the drawable at the swap boundary.
use glow::HasContext;
pub struct Canvas {
    gl: glow::Context,
    pub framebuffer: glow::Framebuffer,
    color: glow::Renderbuffer,
    depth: glow::Renderbuffer,
    size: [i32; 2],
}
impl Canvas {
    pub fn new(gl: glow::Context, size: [i32; 2]) -> Result<Self, String> {
        unsafe {
            let framebuffer = gl.create_framebuffer()?;
            let color = gl.create_renderbuffer()?;
            let depth = gl.create_renderbuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, size[0], size[1]);
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(color),
            );
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH24_STENCIL8, size[0], size[1]);
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_STENCIL_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth),
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_renderbuffer(color);
                gl.delete_renderbuffer(depth);
                gl.delete_framebuffer(framebuffer);
                return Err("native canvas framebuffer is incomplete".into());
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            Ok(Self {
                gl,
                framebuffer,
                color,
                depth,
                size,
            })
        }
    }
    pub fn show(&self) {
        unsafe {
            let read = self
                .gl
                .get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
            let draw = self
                .gl
                .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
            let clipped = self.gl.is_enabled(glow::SCISSOR_TEST);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.framebuffer));
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            self.gl.blit_framebuffer(
                0,
                0,
                self.size[0],
                self.size[1],
                0,
                0,
                self.size[0],
                self.size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
            self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            if clipped {
                self.gl.enable(glow::SCISSOR_TEST);
            }
        }
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_framebuffer(self.framebuffer);
            self.gl.delete_renderbuffer(self.color);
            self.gl.delete_renderbuffer(self.depth);
        }
    }
}
