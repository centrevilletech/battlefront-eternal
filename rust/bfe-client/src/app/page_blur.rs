//! Native compositor for PageTheme's CSS backdrop-filter. The game renderer
//! still owns the world frame; this only filters its finished colour target.
use glow::HasContext;
pub(super) struct PageBlur {
    program: glow::Program,
    vao: glow::VertexArray,
    textures: [glow::Texture; 2],
    targets: [glow::Framebuffer; 2],
    direction: Option<glow::UniformLocation>,
    size: [i32; 2],
}
impl PageBlur {
    pub fn new(gl: &glow::Context, size: [i32; 2]) -> Result<Self, String> {
        unsafe {
            let es = gl.get_parameter_string(glow::VERSION).contains("OpenGL ES");
            let prefix = if es {
                "#version 300 es\nprecision highp float;\n"
            } else {
                "#version 330 core\n"
            };
            let compile = |kind, body: &str| -> Result<glow::Shader, String> {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, &format!("{prefix}{body}"));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    return Err(error);
                }
                Ok(shader)
            };
            let vs=compile(glow::VERTEX_SHADER,"out vec2 uv;void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=p;gl_Position=vec4(p*2.-1.,0.,1.);}")?;
            let fs=compile(glow::FRAGMENT_SHADER,"in vec2 uv;out vec4 color;uniform sampler2D image;uniform vec2 direction;void main(){vec4 sum=vec4(0.);float weights=0.;for(int i=-6;i<=6;i++){float f=float(i)*.5;float weight=exp(-.5*f*f);sum+=texture(image,uv+direction*f)*weight;weights+=weight;}color=sum/weights;}")?;
            let program = gl.create_program()?;
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            if !gl.get_program_link_status(program) {
                let error = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(error);
            }
            let textures = [gl.create_texture()?, gl.create_texture()?];
            let targets = [gl.create_framebuffer()?, gl.create_framebuffer()?];
            for i in 0..2 {
                gl.bind_texture(glow::TEXTURE_2D, Some(textures[i]));
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    size[0],
                    size[1],
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(targets[i]));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(textures[i]),
                    0,
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("page blur framebuffer is incomplete".into());
                }
            }
            let direction = gl.get_uniform_location(program, "direction");
            let vao = gl.create_vertex_array()?;
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "image").as_ref(), 0);
            Ok(Self {
                program,
                vao,
                textures,
                targets,
                direction,
                size,
            })
        }
    }
    pub fn apply(&self, gl: &glow::Context, target: glow::Framebuffer, sigma: f64) {
        unsafe {
            let [w, h] = self.size;
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.color_mask(true, true, true, true);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.targets[0]));
            gl.blit_framebuffer(
                0,
                0,
                w,
                h,
                0,
                0,
                w,
                h,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.viewport(0, 0, w, h);
            for pass in 0..2 {
                gl.bind_framebuffer(
                    glow::FRAMEBUFFER,
                    Some(if pass == 0 { self.targets[1] } else { target }),
                );
                gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[pass]));
                gl.uniform_2_f32(
                    self.direction.as_ref(),
                    if pass == 0 {
                        sigma as f32 / w as f32
                    } else {
                        0.
                    },
                    if pass == 1 {
                        sigma as f32 / h as f32
                    } else {
                        0.
                    },
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
        }
    }
    pub fn resize(&mut self, gl: &glow::Context, size: [i32; 2]) {
        if self.size == size {
            return;
        }
        self.size = size;
        unsafe {
            for texture in self.textures {
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    size[0],
                    size[1],
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
            }
        }
    }
    pub fn dispose(&self, gl: &glow::Context) {
        unsafe {
            for texture in self.textures {
                gl.delete_texture(texture);
            }
            for target in self.targets {
                gl.delete_framebuffer(target);
            }
            gl.delete_vertex_array(self.vao);
            gl.delete_program(self.program);
        }
    }
}
