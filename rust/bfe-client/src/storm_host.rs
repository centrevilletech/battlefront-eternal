//! StormSky's retained panorama, horizon materials and lightning. Cloud noise,
//! shaders and strike arithmetic come from the existing renderer/environment owners.
use crate::{render_bridge::Bridge, resources::Resources, scene::View};
use bfe_content_native::{geometry::{Attribute, Geometry, Scalar}, loader::{Blend, MaterialKind, Mesh}};
use bfe_render::{far as far, post, programs, shader,
    frame_plan::layout::{binding as b, facts as f, kind as k, meta as m, role as r}};
use serde_json::json;
use three::Matrix4;
const SIZE: [i32; 2] = [768, 384];
const HAZE: [f64; 3] = [0.065, 0.085, 0.11];

#[derive(Clone, Copy, Default)]
pub struct Light { pub texture: u32, pub flash: f64, pub position: [f64; 3] }
struct Item { slot: usize, fields: Vec<(&'static str, usize, usize)>, follow: Option<[f64; 2]> }
impl Item {
    fn set(&self, bridge: &mut Bridge, name: &str, values: &[f64]) {
        if let Some((_, at, count)) = self.fields.iter().find(|(n, _, _)| *n == name) {
            bridge.r.far.input[*at..*at + count].copy_from_slice(&values[..*count]);
        }
    }
}
pub struct StormHost {
    pub enabled: bool,
    storm: bfe_environment::storm::Storm,
    sky: Item,
    horizon: Vec<Item>,
    bolt: usize,
    target: u32,
    texture: u32,
    noise: u32,
    cloud_program: u32,
    wind: [f64; 2],
}
fn member(name: &str) -> usize {
    shader::FRAME_OFFSETS[shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).expect("frame member")]
}
fn item(bridge: &mut Bridge, key: &'static str, geometry: &Geometry, texture: Option<u32>, flags: u32,
    blend: &[u32; 6], order: f64, slot: &mut usize) -> Result<Item, String> {
    let spec = programs::spec(key)?;
    let program = bridge.add_owned_program(key)?;
    let geometry = geometry.register(&mut bridge.r, key)?;
    let types: Vec<_> = spec.uniforms.iter().map(|(_, ty)| match ty { 1 => glow::FLOAT, 2 => glow::FLOAT_VEC2,
        3 => glow::FLOAT_VEC3, 4 => glow::FLOAT_VEC4, 16 => glow::FLOAT_MAT4, 32 => glow::SAMPLER_2D, _ => 0 }).collect();
    let names = spec.uniforms.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(",");
    let table = bridge.far_table(program, &names, &types, &vec![1; types.len()]).ok_or("storm far table")?;
    let at = *slot;
    let id = bridge.far_item(program, table, geometry, flags | far::F_PLACED | far::F_COLOR_WRITE,
        glow::LEQUAL, blend, &[0., order, 0., at as f64], &[0.; 4], at as u32);
    bridge.r.far.input[at + 1..at + 17].copy_from_slice(&Matrix4::default().elements);
    *slot += 17;
    let mut fields = Vec::new();
    for (index, (name, size)) in spec.uniforms.iter().enumerate() {
        let source = match *name { "projectionMatrix" => Some(far::SRC_PROJECTION), "viewMatrix" => Some(far::SRC_VIEW),
            "cameraPosition" => Some(far::SRC_EYE), "modelMatrix" => Some(far::SRC_MODEL), _ => None };
        if let Some(source) = source { bridge.far_source(id, index as u32, source, &[], 0); }
        else if *size == 32 { bridge.far_source(id, index as u32, far::SRC_TEXTURE, &[], texture.ok_or("storm image")?); }
        else {
            bridge.far_source(id, index as u32, far::SRC_INPUT, &[], *slot as u32);
            fields.push((*name, *slot, *size as usize));
            *slot += *size as usize;
        }
    }
    Ok(Item { slot: at, fields, follow: None })
}
impl StormHost {
    pub fn hide(&self, input: &mut [f64]) {
        input[self.sky.slot] = 0.;
        input[self.bolt] = 0.;
        for layer in &self.horizon { input[layer.slot] = 0.; }
    }
    pub fn new(bridge: &mut Bridge, resources: &mut Resources, meshes: &[Mesh], textures: &[u32],
        water: &serde_json::Value, slot: &mut usize) -> Result<Self, String> {
        let source = resources.source(&json!({"width":SIZE[0],"height":SIZE[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string())?;
        let sample = resources.sample(source, r#"{"wrapS":1000,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"mipmaps":false,"flip":false,"premultiply":false,"alignment":1,"compare":null}"#)?;
        let target = resources.target(&json!({"width":SIZE[0],"height":SIZE[1],"color":sample,"depth":null,"depthBuffer":false,"stencil":false,"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string())?;
        resources.upload(sample)?;
        let texture = bridge.r.core.add_texture(resources.texture(sample)?);
        let mut random = bfe_environment::random::Random::new(8217);
        let pixels: Vec<u8> = (0..64*64*64).map(|_| (random.next() * 255.) as u8).collect();
        let source = resources.source(&json!({"width":64,"height":64,"depth":64,"target":glow::TEXTURE_3D,"format":1028,"dataType":1009,"srgb":false}).to_string())?;
        resources.source_pixels(source, &pixels)?;
        let sample = resources.sample(source, r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1006,"mag":1006,"anisotropy":1,"mipmaps":false,"flip":false,"premultiply":false,"alignment":1,"compare":null}"#)?;
        resources.upload(sample)?;
        let noise = bridge.r.core.add_texture(resources.texture(sample)?);
        let quad = Geometry {
            buffers: vec![[-1f32,-1.,0., 1.,-1.,0., -1.,1.,0., 1.,1.,0.].iter().flat_map(|x|x.to_le_bytes()).collect(),
                [0f32,0., 1.,0., 0.,1., 1.,1.].iter().flat_map(|x|x.to_le_bytes()).collect()],
            attributes: vec![Attribute{name:"position".into(),buffer:0,size:3,scalar:Scalar::Float,normalized:false,stride:0,offset:0},
                Attribute{name:"uv".into(),buffer:1,size:2,scalar:Scalar::Float,normalized:false,stride:0,offset:0}],
            indices: vec![0,1,2,2,1,3], vertices:4, groups:vec![],
        };
        let sky = item(bridge, "effects/storm-sky-s0", &quad, Some(texture), 0, &[0;6], -99999.5, slot)?;
        let mut horizon = Vec::new();
        for mesh in meshes.iter().filter(|mesh| mesh.material.kind == MaterialKind::Sky && mesh.material.record["skyLayer"].as_f64().unwrap_or(0.) > 0.) {
            let dst = if mesh.material.blend == Blend::Additive {glow::ONE} else {glow::ONE_MINUS_SRC_ALPHA};
            let blend = [glow::FUNC_ADD,glow::FUNC_ADD,glow::SRC_ALPHA,dst,glow::SRC_ALPHA,dst];
            let mut layer = item(bridge, "effects/storm-horizon-blended-s2", &mesh.geometry,
                mesh.material.texture.map(|id| textures[id]), 2 | far::F_BLEND, &blend,
                -100000. + mesh.material.record["skyLayer"].as_f64().unwrap_or(0.), slot)?;
            layer.follow = serde_json::from_value(mesh.material.record["skyFollow"].clone()).ok();
            horizon.push(layer);
        }
        let bolt = *slot;
        bridge.effect_far_mesh(bfe_render::effect_layout::STORM_LIGHTNING as u32, false, bolt as u32, 0, bolt as u32)?;
        *slot += 21;
        let mut wind = std::array::from_fn(|i| water["look"]["WindDirection"][i].as_f64().unwrap_or(if i == 0 {1.} else {0.}));
        let length = (wind[0]*wind[0]+wind[1]*wind[1]).sqrt();
        if length > 0. { wind = wind.map(|x| x * (1./length)); } else { wind = [1.,0.]; }
        Ok(Self { enabled:true, storm:Default::default(), sky, horizon, bolt, target, texture, noise,
            cloud_program:bridge.add_owned_program("effects/storm-clouds-s0")?, wind })
    }
    pub fn frame(&mut self, bridge: &mut Bridge, view: &mut View, time: f64) -> Option<Light> {
        let eye: [f64;3] = view.values[member("cameraPosition")..][..3].try_into().unwrap();
        bridge.r.far.input[self.sky.slot] = self.enabled as u8 as f64;
        for layer in &self.horizon {bridge.r.far.input[layer.slot] = self.enabled as u8 as f64;}
        bridge.r.far.input[self.bolt] = 0.;
        if !self.enabled { return None; }
        self.storm.update::<bfe_environment::surface_math::V8>(time,eye,true);
        let light = Light { texture:self.texture, flash:self.storm.flash, position:self.storm.position };
        for name in ["ambient", "ambientTop", "ambientBottom", "characterAmbient"] {
            let at = member(name);
            for (i, amount) in [0.10,0.13,0.18].iter().enumerate() { view.values[at+i] += light.flash * amount; }
        }
        let mut world = Matrix4::default(); world.elements.copy_from_slice(&view.values[16..32]);world.invert();
        let mut inverse = Matrix4::default();inverse.elements.copy_from_slice(&view.values[..16]);inverse.invert();
        self.sky.set(bridge,"viewRotation",&world.elements);
        self.sky.set(bridge,"inverseProjection",&inverse.elements);
        self.sky.set(bridge,"eyePosition",&eye);
        self.sky.set(bridge,"lightningPosition",&light.position);
        self.sky.set(bridge,"lightningFlash",&[light.flash]);
        for layer in &self.horizon {
            let y=layer.follow.map_or(eye[1],|f|f[0]*eye[1]+(1.-f[0])*f[1]);
            bridge.r.far.input[layer.slot+13..layer.slot+16].copy_from_slice(&[eye[0],y,eye[2]]);
            layer.set(bridge,"hazeColor",&HAZE);layer.set(bridge,"lightningPosition",&light.position);layer.set(bridge,"lightningFlash",&[light.flash]);
        }
        bridge.r.far.input[self.bolt] = (light.flash > 0.005) as u8 as f64;
        bridge.r.far.input[self.bolt+1..self.bolt+17].copy_from_slice(&Matrix4::default().elements);
        bridge.r.far.input[self.bolt+17..self.bolt+21].copy_from_slice(&[light.flash,self.storm.strikes as f64,0.,self.storm.count as f64]);
        if self.storm.dirty {
            let mut at=0;
            for values in [&self.storm.positions,&self.storm.tangents,&self.storm.edges,&self.storm.widths,&self.storm.energies] {
                bridge.r.far.vertices[at..at+values.len()].copy_from_slice(values);at+=values.len();
            }
        }
        let facts=&mut bridge.r.frames.plan.facts;
        facts[f::CLOUDS]=1.;facts[f::CLOUD_TIME]=time;facts[f::CLOUD_GENERATION]=1.;facts[f::CLOUD_EYE..f::CLOUD_EYE+3].copy_from_slice(&eye);
        let row=&mut bridge.r.frames.bindings[r::CLOUDS*b::LEN..][..b::LEN];row.fill(0.);
        row[b::TARGET]=self.target as f64;row[b::SAMPLE]=-1.;row[b::WIDTH]=SIZE[0] as f64;row[b::HEIGHT]=SIZE[1] as f64;
        row[b::SCISSOR+2]=SIZE[0] as f64;row[b::SCISSOR+3]=SIZE[1] as f64;
        let step=&mut bridge.r.post.input[..post::STEP];step.fill(0.);
        step[post::STEP_PROGRAM]=self.cloud_program as f64;step[post::STEP_TARGET]=post::TARGET_BOUND;
        step[post::STEP_WIDTH]=SIZE[0] as f64;step[post::STEP_HEIGHT]=SIZE[1] as f64;step[post::STEP_UNIFORMS]=5.;
        for (i,(kind,values)) in [(2,&self.wind[..]),(3,&eye[..]),(3,&HAZE[..]),(1,&[time][..]),(post::TYPE_SAMPLER_3D,&[self.noise as f64][..])].iter().enumerate() {
            let at=post::STEP_FIRST+i*post::UNIFORM;step[at]=i as f64;step[at+1]=*kind as f64;step[at+2..at+2+values.len()].copy_from_slice(values);
        }
        bridge.r.post.states[..post::STATE].fill(0.);
        let meta=&mut bridge.r.frames.meta[r::CLOUDS*m::LEN..][..m::LEN];meta[m::KIND]=k::SCREEN as f64;meta[m::STEPS]=1.;meta[m::BOUND]=1.;
        bridge.frame_publish_frame(r::CLOUDS as u32);
        Some(light)
    }
}
