// Source-owned programs. Names belong to a family, independently of the draw
// kind and pass numbers. The manifest also declares browser phase layouts.
use crate::{core::{Core, MultiDraw}, shader::Kind};
use glow::HasContext;
#[path = "program_catalog.rs"]
mod catalog;

pub struct Spec {
    pub key: &'static str,
    pub kind: Kind,
    pub vertex_prefix: &'static str,
    pub fragment_prefix: &'static str,
    pub expanded: bool,
    pub lamps: bool,
    pub vertex: &'static str,
    pub fragment: &'static str,
    pub attributes: &'static [(&'static str, u32)],
    pub uniforms: &'static [(&'static str, u32)],
    pub uniform_sizes: &'static [(&'static str, u32)],
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
    if s.expanded { return (format!("{}{}", s.vertex_prefix, s.vertex), format!("{}{}", s.fragment_prefix, s.fragment)); }
    // Interface model and projector variants use the same finite prefixes as
    // expanded world sources. Apply them to body sources before their text;
    // dropping a skin/colour define silently links the wrong active inputs.
    (format!("{PREFIX}{VERTEX}{}{}", s.vertex_prefix, s.vertex), format!("{PREFIX}{FRAGMENT}{}{}", s.fragment_prefix, s.fragment))
}

fn uniform_type(ty: u32) -> u32 {
    match ty {
        glow::FLOAT => 1, glow::FLOAT_VEC2 => 2, glow::FLOAT_VEC3 => 3, glow::FLOAT_VEC4 => 4,
        glow::INT | glow::BOOL => 5, glow::FLOAT_MAT3 => 9, glow::FLOAT_MAT4 => 16,
        glow::SAMPLER_2D => 32, glow::SAMPLER_3D => 33, glow::SAMPLER_2D_SHADOW => 34, _ => 0,
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
        let id = self.add_program_lamps(s.kind, &vs, &fs, &attributes, &uniforms, s.lamps)?;
        let gl = &self.gl;
        let program = self.programs[id as usize].program;
        let validation = (|| unsafe {
            // glow's per-uniform block-index query is unsupported on WebGL.
            // Enumerate each block's member indices through the portable API.
            let count = gl.get_active_uniforms(program);
            let mut members = vec![false; count as usize];
            for block in 0..gl.get_program_parameter_i32(program, glow::ACTIVE_UNIFORM_BLOCKS) as u32 {
                let name = gl.get_active_uniform_block_name(program, block);
                if s.kind != Kind::World || !matches!(name.as_str(), "BfeFrame" | "BfeShadow") {
                    return Err(format!("{key}: undeclared uniform block {name}"));
                }
                let count = gl.get_active_uniform_block_parameter_i32(program, block, glow::UNIFORM_BLOCK_ACTIVE_UNIFORMS);
                let mut indices = vec![0; count as usize];
                gl.get_active_uniform_block_parameter_i32_slice(program, block, glow::UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES, &mut indices);
                for index in indices { members[index as usize] = true; }
            }
            for i in 0..count {
                if members[i as usize] { continue; }
                let u = gl.get_active_uniform(program, i).ok_or("missing active uniform")?;
                let name = u.name.strip_suffix("[0]").unwrap_or(&u.name);
                let size = s.uniform_sizes.iter().find(|(n, _)| *n == name).map_or(1, |(_, size)| *size);
                if u.size != size as i32 || !s.uniforms.iter().any(|(n, t)| *n == name && *t == uniform_type(u.utype)) {
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
    fn body_variants_apply_their_declared_feature_prefixes() {
        let s = spec("interface/model-skin-color").unwrap();
        let (vertex, _) = sources(s);
        assert!(vertex.contains("#define USE_SKINNING\n#define USE_COLOR\n"));
        let (vertex, _) = sources(spec("interface/basic-map").unwrap());
        assert!(vertex.contains("#define USE_MAP\n"));
    }
    #[test]
    fn expanded_world_sources_keep_their_vocabulary_and_use_frame_rewrite() {
        let s = Spec { key: "world/test", vertex_prefix: "", fragment_prefix: "", kind: Kind::World, expanded: true, lamps: false,
            vertex: "#version 300 es\nuniform mat4 projectionMatrix;\nuniform mat4 viewMatrix;\nin vec3 position;\nvoid main() { gl_Position = projectionMatrix * viewMatrix * vec4(position, 1.0); }\n",
            fragment: "#version 300 es\nprecision highp float;\nout vec4 color;\nvoid main() { color = vec4(1.0); }\n",
            attributes: &[("position", 0)], uniforms: &[], uniform_sizes: &[] };
        let (v, f) = sources(&s);
        assert_eq!(v, s.vertex);
        assert_eq!(f, s.fragment);
        let rewritten = crate::shader::rewrite_lamps(&v, crate::shader::Stage::Vertex, s.kind, s.lamps).unwrap();
        assert!(rewritten.contains("uniform BfeFrame"));
        assert!(rewritten.contains("projectionMatrix * viewMatrix * vec4(position, 1.0)"));
    }
    #[test]
    fn registry_is_closed_and_sources_need_no_three_expansion() {
        assert!(spec("post-hud/unknown").is_err());
        for s in catalog::PROGRAMS {
            let (v, f) = sources(s);
            assert!(v.starts_with("#version 300 es\n") && f.starts_with("#version 300 es\n"));
            assert!(!v.contains("#include") && !f.contains("#include"));
            if s.key.starts_with("world/") {
                for (source, stage) in [(&v, crate::shader::Stage::Vertex), (&f, crate::shader::Stage::Fragment)] {
                    crate::shader::rewrite_lamps(source, stage, s.kind, s.lamps).unwrap_or_else(|e| panic!("{}: {e}", s.key));
                }
            }
            if s.key.starts_with("post-hud/") { assert!(s.uniforms.len() <= 12); }
        }
        assert_eq!(catalog::PROGRAMS.iter().filter(|s| s.key.starts_with("post-hud/")).count(), 12);
    }
}
