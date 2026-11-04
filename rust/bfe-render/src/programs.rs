// Source-owned programs. Names belong to a family, independently of the draw
// kind and pass numbers. The manifest also declares browser phase layouts.
use crate::{core::{Core, MultiDraw}, shader::Kind};
use glow::HasContext;
#[path = "program_catalog.rs"]
mod catalog;

pub struct Spec {
    pub key: &'static str,
    pub vertex: &'static str,
    pub fragment: &'static str,
    pub attributes: &'static [(&'static str, u32)],
    pub uniforms: &'static [(&'static str, u32)],
}

// These programs use ShaderMaterial's small, explicit GLSL vocabulary. No
// colour-space, tone-map or lighting chunks are injected. Arithmetic in the
// bodies is unchanged from render-passes, graphics, fxaa, upscaler and batch.
const PREFIX: &str = "#version 300 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;\nprecision highp sampler2DShadow;\nprecision highp samplerCube;\nprecision highp sampler3D;\nprecision highp sampler2DArray;\nprecision highp isampler2D;\nprecision highp isampler3D;\nprecision highp isamplerCube;\nprecision highp isampler2DArray;\nprecision highp usampler2D;\nprecision highp usampler3D;\nprecision highp usamplerCube;\nprecision highp usampler2DArray;\n";
const VERTEX: &str = "#define attribute in\n#define varying out\n#define texture2D texture\nuniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nin vec3 position;\nin vec2 uv;\n";
const FRAGMENT: &str = "#define varying in\nlayout(location = 0) out highp vec4 pc_fragColor;\n#define gl_FragColor pc_fragColor\n#define texture2D texture\n";

pub fn spec(key: &str) -> Result<&'static Spec, String> {
    catalog::PROGRAMS.iter().find(|s| s.key == key).ok_or_else(|| format!("unknown shader variant: {key}"))
}

pub fn sources(s: &Spec) -> (String, String) {
    (format!("{PREFIX}{VERTEX}{}", s.vertex), format!("{PREFIX}{FRAGMENT}{}", s.fragment))
}

fn uniform_type(ty: u32) -> u32 {
    match ty {
        glow::FLOAT => 1, glow::FLOAT_VEC2 => 2, glow::FLOAT_VEC3 => 3, glow::FLOAT_VEC4 => 4,
        glow::INT | glow::BOOL => 5, glow::FLOAT_MAT3 => 9, glow::FLOAT_MAT4 => 16,
        glow::SAMPLER_2D => 32, glow::SAMPLER_3D => 33, _ => 0,
    }
}

impl<G: HasContext, M: MultiDraw> Core<G, M> {
    // The cache belongs to this context. Context loss discards the bridge;
    // handles are never shared with a new context or another Core.
    pub fn add_owned_program(&mut self, key: &str) -> Result<u32, String> {
        if let Some((_, id)) = self.owned_programs.iter().find(|(k, _)| k == key) { return Ok(*id); }
        let s = spec(key)?;
        let (vs, fs) = sources(s);
        let attributes = s.attributes.iter().map(|(n, l)| (n.to_string(), *l)).collect::<Vec<_>>();
        let uniforms = s.uniforms.iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>();
        let id = self.add_program(Kind::Plain, &vs, &fs, &attributes, &uniforms)?;
        let gl = &self.gl;
        let program = self.programs[id as usize].program;
        let validation = (|| unsafe {
            for i in 0..gl.get_active_uniforms(program) {
                let u = gl.get_active_uniform(program, i).ok_or("missing active uniform")?;
                if u.size != 1 || !s.uniforms.iter().any(|(n, t)| *n == u.name && *t == uniform_type(u.utype)) {
                    return Err(format!("{key}: undeclared uniform {}", u.name));
                }
            }
            for i in 0..gl.get_active_attributes(program) {
                let a = gl.get_active_attribute(program, i).ok_or("missing active attribute")?;
                if !s.attributes.iter().any(|(n, l)| *n == a.name && gl.get_attrib_location(program, n) == Some(*l)) {
                    return Err(format!("{key}: undeclared attribute {}", a.name));
                }
            }
            Ok(())
        })();
        if let Err(error) = validation {
            self.programs.pop();
            unsafe { gl.delete_program(program); }
            return Err(error);
        }
        self.owned_programs.push((key.to_string(), id));
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_is_closed_and_sources_need_no_three_expansion() {
        assert!(spec("post-hud/unknown").is_err());
        for s in catalog::PROGRAMS {
            let (v, f) = sources(s);
            assert!(v.starts_with("#version 300 es\n") && f.starts_with("#version 300 es\n"));
            assert!(!v.contains("#include") && !f.contains("#include"));
            if s.key.starts_with("post-hud/") { assert!(s.uniforms.len() <= 12); }
        }
        assert_eq!(catalog::PROGRAMS.iter().filter(|s| s.key.starts_with("post-hud/")).count(), 12);
    }
}
