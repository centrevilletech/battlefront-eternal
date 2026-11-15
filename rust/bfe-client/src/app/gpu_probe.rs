//! A first-run measurement of the GPU's fill rate, for the starting quality preset.
//!
//! Twelve blended full-screen passes of four texture reads each into a 1280x720
//! target, timed by the wall clock around glFinish (some drivers' timer queries
//! do not run at wall-clock rate). About a frame's worth of a textured scene: a
//! Radeon R5 (Kabini) takes ~40 ms, a Radeon 890M ~3 ms. Runs once, on a fresh
//! profile, before the menus draw.
use glow::HasContext;
use std::time::Instant;

const WIDTH: i32 = 1280;
const HEIGHT: i32 = 720;
const PASSES: u32 = 12;

/// Milliseconds per million pixels drawn, or None when the probe cannot run.
pub fn fill_ms_per_mpix(gl: &glow::Context) -> Option<f64> {
    unsafe {
        let es = gl.version().is_embedded;
        let prefix = if es { "#version 300 es\nprecision highp float;\n" } else { "#version 330 core\n" };
        let compile = |kind, body: &str| -> Option<glow::Shader> {
            let shader = gl.create_shader(kind).ok()?;
            gl.shader_source(shader, &format!("{prefix}{body}"));
            gl.compile_shader(shader);
            if gl.get_shader_compile_status(shader) { Some(shader) } else { gl.delete_shader(shader); None }
        };
        let vs = compile(glow::VERTEX_SHADER, "out vec2 uv;void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=p;gl_Position=vec4(p*2.-1.,0.,1.);}")?;
        let fs = compile(glow::FRAGMENT_SHADER, "in vec2 uv;out vec4 color;uniform sampler2D image;void main(){vec2 s=uv*4.;vec4 c=texture(image,s)*.4+texture(image,s*1.7+.13)*.3+texture(image,s*.6+.31)*.2+texture(image,s*2.3+.57)*.1;color=vec4(c.rgb*(.8+.2*c.a),.5);}")?;
        let program = gl.create_program().ok()?;
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        let linked = gl.get_program_link_status(program);
        let pixels: Vec<u8> = (0..256 * 256 * 4u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
        let source = gl.create_texture().ok();
        let target = gl.create_texture().ok();
        let fb = gl.create_framebuffer().ok();
        let vao = gl.create_vertex_array().ok();
        let mut result = None;
        if let (true, Some(source), Some(target), Some(fb), Some(vao)) = (linked, source, target, fb, vao) {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(source));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, 256, 256, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&pixels)));
            gl.generate_mipmap(glow::TEXTURE_2D);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.bind_texture(glow::TEXTURE_2D, Some(target));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, WIDTH, HEIGHT, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(target), 0);
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE {
                gl.bind_texture(glow::TEXTURE_2D, Some(source));
                gl.use_program(Some(program));
                if let Some(at) = gl.get_uniform_location(program, "image") { gl.uniform_1_i32(Some(&at), 0); }
                gl.bind_vertex_array(Some(vao));
                gl.viewport(0, 0, WIDTH, HEIGHT);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::SCISSOR_TEST);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                // The first draw pays for compiling and first use.
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                gl.finish();
                let start = Instant::now();
                for _ in 0..PASSES { gl.draw_arrays(glow::TRIANGLES, 0, 3); }
                gl.finish();
                let ms = start.elapsed().as_secs_f64() * 1000.;
                if gl.get_error() == glow::NO_ERROR {
                    result = Some(ms / (PASSES as f64 * WIDTH as f64 * HEIGHT as f64 / 1e6));
                }
                gl.disable(glow::BLEND);
            }
        }
        gl.bind_vertex_array(None);
        gl.use_program(None);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        if let Some(t) = source { gl.delete_texture(t); }
        if let Some(t) = target { gl.delete_texture(t); }
        if let Some(f) = fb { gl.delete_framebuffer(f); }
        if let Some(v) = vao { gl.delete_vertex_array(v); }
        gl.delete_program(program);
        result
    }
}
