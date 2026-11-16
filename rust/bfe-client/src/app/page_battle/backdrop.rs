//! The battle behind the screen spawn screen. The 2004 look's hologram room hides it completely, so it is not
//! drawn. In the other looks it is darkened behind the screen; on a machine that cannot hold 45 frames a
//! second the battle and the stage over it (body, hologram) are drawn every other frame and that frame kept
//! and shown in between, so the screen's chrome (pointer, picks) keeps twice the rate.
use glow::HasContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backdrop {
    /// Draw the battle (and keep it when the machine is slow).
    Draw,
    /// Show the frame kept from the last draw.
    Reuse,
    /// Nothing shows through: clear only.
    Clear,
}

#[derive(Default)]
pub(super) struct Kept {
    target: Option<(glow::Framebuffer, glow::Renderbuffer, [i32; 2])>,
    valid: bool,
    reused: bool,
    frame_ms: f64,
    slow: bool,
}

impl Kept {
    /// This frame's choice, from the look (0: the 2004 room) and the last frame's length.
    pub fn choose(&mut self, look: usize, dt: f64) -> Backdrop {
        if look == 0 {
            self.valid = false;
            return Backdrop::Clear;
        }
        let ms = dt * 1000.;
        self.frame_ms = if self.frame_ms == 0. { ms } else { self.frame_ms + (ms - self.frame_ms) * 0.1 };
        // Slow below 45 fps, fast again above 70 (the halved world itself lifts the rate).
        if self.frame_ms > 22. {
            self.slow = true;
        } else if self.frame_ms < 14. {
            self.slow = false;
        }
        if self.slow && self.valid && !self.reused {
            self.reused = true;
            Backdrop::Reuse
        } else {
            self.reused = false;
            Backdrop::Draw
        }
    }
    pub fn slow(&self) -> bool {
        self.slow
    }
    /// After a draw on a slow machine: copy the canvas's colour.
    pub fn keep(&mut self, gl: &glow::Context, canvas: glow::Framebuffer, size: [i32; 2]) {
        if !self.slow {
            self.valid = false;
            return;
        }
        unsafe {
            if self.target.is_some_and(|t| t.2 != size) {
                self.release(gl);
            }
            if self.target.is_none() {
                let (Ok(fb), Ok(color)) = (gl.create_framebuffer(), gl.create_renderbuffer()) else {
                    return;
                };
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, size[0], size[1]);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(color));
                self.target = Some((fb, color, size));
            }
            let (fb, _, _) = self.target.unwrap();
            blit(gl, canvas, fb, size);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(canvas));
        }
        self.valid = true;
    }
    /// Instead of a draw: the kept frame, or a clear, into the canvas, ready for the stage.
    pub fn show(&self, gl: &glow::Context, canvas: glow::Framebuffer, size: [i32; 2], choice: Backdrop) {
        unsafe {
            match (choice, self.target) {
                (Backdrop::Reuse, Some((fb, _, _))) => blit(gl, fb, canvas, size),
                _ => {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(canvas));
                    gl.disable(glow::SCISSOR_TEST);
                    gl.color_mask(true, true, true, true);
                    gl.clear_color(0., 0., 0., 1.);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(canvas));
            gl.viewport(0, 0, size[0], size[1]);
            gl.depth_mask(true);
            gl.clear(glow::DEPTH_BUFFER_BIT);
        }
    }
    /// The screen closed: the next opening starts from a fresh draw.
    pub fn forget(&mut self) {
        self.valid = false;
        self.reused = false;
    }
    pub fn release(&mut self, gl: &glow::Context) {
        if let Some((fb, color, _)) = self.target.take() {
            unsafe {
                gl.delete_framebuffer(fb);
                gl.delete_renderbuffer(color);
            }
        }
        self.valid = false;
    }
}

unsafe fn blit(gl: &glow::Context, from: glow::Framebuffer, to: glow::Framebuffer, size: [i32; 2]) {
    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(from));
    gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(to));
    gl.disable(glow::SCISSOR_TEST);
    gl.blit_framebuffer(0, 0, size[0], size[1], 0, 0, size[0], size[1], glow::COLOR_BUFFER_BIT, glow::NEAREST);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_slow_machine_draws_the_world_every_other_frame_and_the_room_never() {
        let mut kept = Kept::default();
        assert_eq!(kept.choose(0, 0.05), Backdrop::Clear);
        // Fast: every frame draws.
        for _ in 0..30 {
            assert_eq!(kept.choose(2, 1. / 120.), Backdrop::Draw);
        }
        // Slow: once a frame is kept, draws and reuses alternate.
        for _ in 0..40 {
            kept.choose(1, 0.06);
        }
        assert!(kept.slow());
        kept.valid = true;
        let a = kept.choose(1, 0.06);
        let b = kept.choose(1, 0.06);
        assert_ne!(a, b);
        assert!([a, b].contains(&Backdrop::Reuse));
    }
}
