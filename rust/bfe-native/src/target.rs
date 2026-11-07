//! A fixed capture-sized target isolates replay pixels from window size and
//! compositor formats. Both native hosts resolve the same RGBA8 sample layout.
use glow::HasContext;
use sha2::{Digest, Sha256};

// Attachments belong to one current context. Explicit deletion is used because
// a context-free Rust Drop could run after winit has destroyed the GL surface.
// The host invokes delete while its context and renderer are still alive.
pub struct Target {
    draw: glow::Framebuffer,
    resolved: glow::Framebuffer,
    color: glow::Renderbuffer,
    depth: glow::Renderbuffer,
    pixels: glow::Renderbuffer,
    pub width: i32,
    pub height: i32,
}

impl Target {
    // Match the headless replay's linear RGBA8 colour and depth24 exactly.
    // Window samples and framebuffer colour space never influence this target.
    // Configuring samples here also permits an ordinary single-sample window.
    pub fn new(gl: &glow::Context, width: i32, height: i32, samples: i32) -> Result<Self, String> {
        unsafe {
            if width <= 0 || height <= 0 || width > gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE)
                || height > gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE)
                || samples < 0 || samples > gl.get_parameter_i32(glow::MAX_SAMPLES) {
                return Err("unsupported capture framebuffer dimensions or samples".into());
            }
            let draw = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(draw));
            let color = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
            gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::RGBA8, width, height);
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(color));
            let depth = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
            gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::DEPTH_COMPONENT24, width, height);
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(depth));
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("incomplete capture framebuffer".into());
            }
            let resolved = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(resolved));
            let pixels = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(pixels));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, width, height);
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(pixels));
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("incomplete resolve framebuffer".into());
            }
            Ok(Self { draw, resolved, color, depth, pixels, width, height })
        }
    }

    // Restore viewport and framebuffer after the output blit. Renderer phase
    // state is reset internally, but target selection belongs to the host.
    pub fn bind(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.draw));
            gl.viewport(0, 0, self.width, self.height);
        }
    }

    // Scissor state belongs to the preceding draw. It must not clip either
    // resolve or window presentation when capture-sized attachments are copied.
    fn resolve(&self, gl: &glow::Context) {
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.draw));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.resolved));
            gl.blit_framebuffer(0, 0, self.width, self.height, 0, 0, self.width, self.height, glow::COLOR_BUFFER_BIT, glow::NEAREST);
        }
    }

    /// Resolve once before scaling: multisample blits cannot resize the image.
    pub fn show(&self, gl: &glow::Context, width: i32, height: i32) {
        self.resolve(gl);
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.resolved));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
            gl.blit_framebuffer(0, 0, self.width, self.height, 0, 0, width, height, glow::COLOR_BUFFER_BIT, glow::NEAREST);
        }
    }

    /// Hash one named capture frame, never whichever frame happens to end a timed run.
    pub fn hash(&self, gl: &glow::Context) -> Result<String, String> {
        self.resolve(gl);
        // Read bottom-to-top RGBA bytes in both hosts. No screenshot encoder,
        // compositor gamma conversion or row-flipping step affects the hash.
        let mut bytes = vec![0; self.width as usize * self.height as usize * 4];
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.resolved));
            gl.read_pixels(0, 0, self.width, self.height, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut bytes)));
            let error = gl.get_error();
            if error != glow::NO_ERROR { return Err(format!("frame readback GL error {error}")); }
        }
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    // The owning GL context must still be current when releasing attachments.
    pub fn delete(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.draw);
            gl.delete_framebuffer(self.resolved);
            gl.delete_renderbuffer(self.color);
            gl.delete_renderbuffer(self.depth);
            gl.delete_renderbuffer(self.pixels);
        }
    }
}
