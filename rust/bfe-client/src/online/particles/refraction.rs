//! Screen-space particle materials share one sorted copy of the scene.
use super::{Batch, BattleContent, Bridge, Particles, Prepared, Resources, MISSION_CAPACITY};
use bfe_render::effects as draw;

pub(super) struct Material {
    definition: usize,
    image: usize,
    settings: [f32; 2],
}

pub(super) fn materials(content: &BattleContent, prepared: &Prepared) -> Vec<Material> {
    let mut result = Vec::new();
    if let Some((_, library)) = content.effects.iter().find(|(n, _)| n == "battle") {
        for (definition, d) in library.descriptor["definitions"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let g = &d["geometry"];
            if g["blend"] != "refraction" {
                continue;
            }
            let Some(image) = g["texture"].as_str().and_then(|s| prepared.textures.get(s)) else {
                continue;
            };
            result.push(Material {
                definition,
                image: *image,
                settings: [
                    g["blurValue"].as_f64().unwrap_or(0.) as f32,
                    g["blurRes"].as_f64().unwrap_or(0.) as f32,
                ],
            });
        }
    }
    result
}

pub(in crate::online) struct Refraction {
    source: u32,
    sample: u32,
    texture: u32,
    size: [i32; 2],
    batches: Vec<u32>,
}

fn target(size: [i32; 2]) -> String {
    serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,
        "format":1023,"dataType":1009,"srgb":false,"renderTarget":true})
    .to_string()
}

impl Refraction {
    pub(super) fn new(
        materials: &[Material],
        particles: &mut Particles,
        textures: &[u32],
        gpu: &mut Resources,
        bridge: &mut Bridge,
    ) -> Result<Self, String> {
        let size = [1, 1];
        let source = gpu.source(&target(size))?;
        let sample = gpu.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":true,"compare":null}"#)?;
        gpu.upload(sample)?;
        let texture = bridge.r.core.add_texture(gpu.texture(sample)?);
        let spec = bfe_render::programs::spec("effects/particle-cpu")?;
        let (vertex, _) = bfe_render::programs::sources(spec);
        let vertex = refraction_vertex(&vertex)?;
        let attribs: Vec<_> = spec
            .attributes
            .iter()
            .map(|(name, slot)| ((*name).to_owned(), *slot))
            .collect();
        let uniforms = [
            "modelViewMatrix",
            "projectionMatrix",
            "map",
            "sceneColor",
            "blurSettings",
        ]
        .map(str::to_owned);
        let program = bridge.r.core.add_program(
            bfe_render::shader::Kind::Effect,
            &vertex,
            include_str!("refraction.frag"),
            &attribs,
            &uniforms,
        )?;
        let blend = [
            1,
            glow::FUNC_ADD,
            glow::FUNC_ADD,
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        ];
        let mut keys = Vec::new();
        let mut batches = Vec::new();
        for m in materials {
            let key = (m.image, m.settings);
            let slot = if let Some((_, slot)) = keys.iter().find(|(k, _)| *k == key) {
                *slot
            } else {
                // All six instance attributes use the shared 21-float CPU particle layout.
                let handle = bridge.fx_add_batch(
                    program,
                    textures[m.image] as i32,
                    &blend,
                    3,
                    &[4, 3, 5, 4, 6, 4, 7, 3, 8, 3, 9, 4],
                    -1,
                );
                bridge.fx_batch_interleaved(handle);
                bridge.fx_batch_sampler(handle, "sceneColor", 12, texture as i32);
                bridge.r.effects.batch_constants(
                    &bridge.r.core,
                    handle,
                    &[("blurSettings", &m.settings)],
                );
                bridge
                    .r
                    .effects
                    .prepare_batch(&bridge.r.core, handle, MISSION_CAPACITY);
                let slot = particles.batches.len();
                particles.batches.push(Batch {
                    handle,
                    analytic: false,
                    at: particles.words,
                    stride: 21,
                    capacity: MISSION_CAPACITY,
                    count: 0,
                    order: 0,
                });
                particles.words += MISSION_CAPACITY * 21;
                batches.push(handle);
                keys.push((key, slot));
                slot
            };
            particles.effect_batches[m.definition] = Some(slot);
        }
        bridge.fx_reserve(particles.words as u32);
        Ok(Self {
            source,
            sample,
            texture,
            size,
            batches,
        })
    }

    pub(in crate::online) fn frame(
        &mut self,
        screen: [i32; 2],
        gpu: &mut Resources,
        bridge: &mut Bridge,
    ) {
        let count = bridge.r.effects.input[draw::IN_DRAWS] as usize;
        let is_blur = |row: &[f64]| {
            row[0] == draw::KIND_BATCH as f64 && self.batches.contains(&(row[1] as u32))
        };
        let rows =
            &bridge.r.effects.input[draw::DRAW_AT..draw::DRAW_AT + count * draw::DRAW_STRIDE];
        if !rows.chunks_exact(draw::DRAW_STRIDE).any(is_blur) {
            return;
        }
        let mut ready = count < draw::MAX_DRAWS && screen.iter().all(|&n| n > 0);
        if ready && self.size != screen {
            ready = gpu
                .source_desc(self.source, &target(screen))
                .and_then(|_| gpu.upload(self.sample))
                .and_then(|_| gpu.texture(self.sample))
                .and_then(|gl| bridge.r.replace_sample_texture(self.texture as usize, gl))
                .is_ok();
            if ready {
                self.size = screen;
            }
        }
        for row in bridge.r.effects.input[draw::DRAW_AT..]
            .chunks_exact_mut(draw::DRAW_STRIDE)
            .take(count)
            .filter(|r| is_blur(r))
        {
            row[draw::DRAW_KEY + 1] = 10.5;
            if !ready {
                row[2] = 0.;
            }
        }
        if !ready {
            return;
        }
        let row = &mut bridge.r.effects.input[draw::DRAW_AT + count * draw::DRAW_STRIDE..]
            [..draw::DRAW_STRIDE];
        row.fill(0.);
        row[..4].copy_from_slice(&[
            draw::KIND_COPY as f64,
            self.texture as f64,
            screen[0] as f64,
            screen[1] as f64,
        ]);
        row[draw::DRAW_KEY + 1] = 10.49;
        row[draw::DRAW_KEY + 5] = self.texture as f64;
        bridge.r.effects.input[draw::IN_DRAWS] = (count + 1) as f64;
    }
}

// Project a radially expanded corner for blur particles, applying the game's
// particle opacity curve and 2x blur boost before the near-eye fade.
pub(super) fn refraction_vertex(vertex: &str) -> Result<String, String> {
    let vertex = super::swbf2_near_fade_vertex(vertex)?;
    let replace_once = |src: String, anchor: &str, body: &str| -> Result<String, String> {
        if src.matches(anchor).count() != 1 {
            return Err(format!("refraction vertex shader missing anchor: {anchor}"));
        }
        Ok(src.replacen(anchor, body, 1))
    };
    let vertex = replace_once(
        vertex,
        "  varying vec4 vColor;",
        "  uniform vec2 blurSettings;\n  varying vec4 vColor;\n  varying vec4 vRefractClip;",
    )?;
    let vertex = replace_once(
        vertex,
        "    vColor = iColor;",
        "    float blurRaw = clamp(iColor.a, 0.0, 1.0);\n\
         float blurInv = 1.0 - blurRaw;\n\
         float blurAlpha = min((((1.0 - blurInv * blurInv) + blurRaw) * 0.5) * 2.0, 1.0);\n\
         vColor = vec4(iColor.rgb, blurAlpha);",
    )?;
    let vertex = replace_once(
        vertex,
        "    vec4 mv;",
        "    vec4 mv;\n    vec2 refractShift = vec2(0.0);",
    )?;
    let vertex = replace_once(
        vertex,
        "gl_Position = vec4(2.0, 2.0, 2.0, 1.0);\nreturn;",
        "gl_Position = vec4(2.0, 2.0, 2.0, 1.0);\nvRefractClip = gl_Position;\nreturn;",
    )?;
    let vertex = replace_once(
        vertex,
        "      mv.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;",
        "      vec2 offset = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y);\n\
         mv.xy += offset * size;\n\
         refractShift = offset * (2.0 * blurSettings.x);",
    )?;
    replace_once(
        vertex,
        "    gl_Position = projectionMatrix * mv;",
        "    vRefractClip = projectionMatrix * vec4(mv.xy + refractShift, mv.zw);\n\
         gl_Position = projectionMatrix * mv;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blur_alpha(raw: f32) -> f32 {
        let a = raw.clamp(0.0, 1.0);
        let inv = 1.0 - a;
        ((((1.0 - inv * inv) + a) * 0.5) * 2.0).min(1.0)
    }

    fn copy_lod(width: f32, height: f32, min_lod: f32) -> f32 {
        let copy = (width.max(height) / 256.0).max(1.0).log2().max(0.0);
        copy.max(min_lod.max(0.0))
    }

    #[test]
    fn refraction_shaders_rewrite_and_project_radially_displaced_corners() {
        let spec = bfe_render::programs::spec("effects/particle-cpu").unwrap();
        let (base_vertex, _) = bfe_render::programs::sources(spec);
        let vertex = refraction_vertex(&base_vertex).unwrap();
        let fragment = include_str!("refraction.frag");
        assert!(vertex.contains("uniform vec2 blurSettings;"));
        assert!(vertex.contains("varying vec4 vRefractClip;"));
        assert!(vertex.contains("refractShift = offset * (2.0 * blurSettings.x);"));
        assert!(fragment.contains("textureLod(sceneColor, uv, lod).rgb * vColor.rgb"));
        assert!(fragment.contains("float alpha = mask.a * vColor.a;"));
        let vs = bfe_render::shader::rewrite_lamps(
            &vertex,
            bfe_render::shader::Stage::Vertex,
            bfe_render::shader::Kind::Effect,
            false,
        )
        .unwrap();
        let fs = bfe_render::shader::rewrite_lamps(
            fragment,
            bfe_render::shader::Stage::Fragment,
            bfe_render::shader::Kind::Effect,
            false,
        )
        .unwrap();
        let _ = bfe_render::shader::to_desktop_glsl(&vs, false);
        let _ = bfe_render::shader::to_desktop_glsl(&fs, false);
        assert_eq!(blur_alpha(0.0), 0.0);
        assert!((blur_alpha(0.25) - 0.6875).abs() < 1e-6);
        assert_eq!(blur_alpha(0.5), 1.0);
        assert_eq!(copy_lod(128.0, 128.0, 0.0), 0.0);
        assert!((copy_lod(1024.0, 512.0, 1.0) - 2.0).abs() < 1e-6);
        assert!((copy_lod(512.0, 512.0, 3.0) - 3.0).abs() < 1e-6);
    }
}
