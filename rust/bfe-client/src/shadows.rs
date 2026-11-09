//! Native sun cascades reuse the renderer's static-depth and camera owners.
use crate::{render_bridge::Bridge, resources::Resources, scene::View};
use bfe_render::{
    cameras::layout::{input as I, lens as N, light_output as L, shader_frame as SF, source as U},
    frame_plan::layout::{binding as B, facts as F, kind as K, meta as M, role as R},
    shadow as S,
};

pub const COUNT: usize = 4;
const SIZE: usize = 2048;

pub struct Shadows {
    maps: [u32; COUNT],
    caches: [u32; COUNT],
    casters: bfe_render::native_casters::Casters,
}
impl Shadows {
    pub fn new(
        bridge: &mut Bridge,
        resources: &mut Resources,
        bones: usize,
    ) -> Result<Self, String> {
        let mut maps = [0; COUNT];
        let mut caches = [0; COUNT];
        for map in maps.iter_mut().chain(caches.iter_mut()) {
            // Graphics.shadowTarget uses unsigned depth with comparison
            // filtering. Resource admission reports unsupported storage as an
            // error rather than silently removing shadows on a driver.
            let source = resources.source(&serde_json::json!({"width":SIZE,"height":SIZE,"depth":1,"target":glow::TEXTURE_2D,"format":1026,"dataType":1014,"srgb":false,"renderTarget":true}).to_string())?;
            let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":515}"#)?;
            resources.upload(sample)?;
            *map = bridge.r.core.add_texture(resources.texture(sample)?);
        }
        let mut skins = Vec::with_capacity(bfe_render::instances::MAX_RECORDS);
        let size = bfe_render::actor_depth::skin_size(bones);
        for _ in 0..bfe_render::instances::MAX_RECORDS {
            let source = resources.source(&serde_json::json!({"width":size,"height":size,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1015,"srgb":false,"renderTarget":true}).to_string())?;
            let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#)?;
            resources.upload(sample)?;
            skins.push(bridge.r.core.add_texture(resources.texture(sample)?));
        }
        let casters = bfe_render::native_casters::Casters::new(&mut bridge.r.core, &skins, bones)?;
        bridge.set_shadow_maps(&maps);
        let input = &mut bridge.r.cameras.input;
        // Graphics.applyShadows' high preset and CSM light-space range.
        input[I::CASCADE_COUNT] = COUNT as f64;
        input[I::MAP_SIZE] = SIZE as f64;
        input[I::MAX_FAR] = 300.;
        input[I::LIGHT_RANGE..I::LIGHT_RANGE + 2].copy_from_slice(&[1., 3000.]);
        input[I::MARGIN] = 400.;
        for row in input[I::SHADOW_METADATA..I::SHADOW_METADATA + COUNT * 6].chunks_exact_mut(6) {
            row.copy_from_slice(&[1., -0.015 / 2999., 0., 1., SIZE as f64, SIZE as f64]);
        }
        bridge.static_depth_finish()?;
        let draws =
            (bfe_render::instances::MAX_RECORDS * 4).max(bridge.r.shadow.statics.records.len());
        bridge
            .r
            .shadow
            .input
            .resize(S::S_FIRST + draws * S::DRAW, 0.);
        bridge.frame_shadow_reserve_frame();
        eprintln!("native shadows: cascades={COUNT} size={SIZE} comparison=LEQUAL");
        Ok(Self {
            maps,
            caches,
            casters,
        })
    }
    pub fn publish(
        &mut self,
        bridge: &mut Bridge,
        view: &mut View,
        records: usize,
    ) -> Result<(), String> {
        self.casters.prepare(&mut bridge.r.core, records)?;
        let mut world = three::Matrix4::default();
        world
            .elements
            .copy_from_slice(&view.values[SF::VIEW..SF::VIEW + 16]);
        world.invert();
        let mut rotation = three::Quaternion::default();
        rotation.set_from_rotation_matrix(&world);
        let input = &mut bridge.r.cameras.input;
        input[I::PUBLISHED] = 1.;
        // Cascades widen and recompose the main lens. Preserve the actual
        // camera orientation as well as its published world/view matrices.
        input[I::QUATERNION..I::QUATERNION + 4]
            .copy_from_slice(&[rotation.x, rotation.y, rotation.z, rotation.w]);
        input[I::POSITION..I::POSITION + 3].copy_from_slice(&view.values[SF::EYE..SF::EYE + 3]);
        input[I::MATRICES + U::WORLD..I::MATRICES + U::WORLD + 16].copy_from_slice(&world.elements);
        input[I::MATRICES + U::INVERSE..I::MATRICES + U::INVERSE + 16]
            .copy_from_slice(&view.values[SF::VIEW..SF::VIEW + 16]);
        input[I::MATRICES + U::PROJECTION..I::MATRICES + U::PROJECTION + 16]
            .copy_from_slice(&view.values[..16]);
        // The admitted native lens is a 60-degree vertical perspective.
        input[I::LENS + N::FOV] = 60.;
        input[I::LENS + N::ASPECT] = view.values[5] / view.values[0];
        input[I::LENS + N::NEAR] = 0.3;
        input[I::LENS + N::FAR] = view.values[14] / (view.values[10] + 1.);
        bridge.r.cameras.settings.output[L::DIRECTION..L::DIRECTION + 3]
            .copy_from_slice(&view.values[SF::SUN_DIRECTION..SF::SUN_DIRECTION + 3]);
        bridge.r.cameras.main();
        bridge.camera_cascades_frame();
        for cascade in 0..COUNT {
            bridge.camera_shadow_frame(cascade as u32);
            bridge.camera_static_shadow_frame(cascade as u32);
            bridge.r.shadow.static_view[bfe_render::static_layout::SHADOW_TIME] =
                view.values[SF::TIME];
            let input = &mut bridge.r.shadow.input;
            input[S::S_TARGET] = self.caches[cascade] as f64;
            input[S::S_FROM] = -1.;
            input[S::S_SIZE] = SIZE as f64;
            let mut count = 0;
            for (id, record) in bridge.r.shadow.statics.records.iter().enumerate() {
                if !record.as_ref().is_some_and(|r| r.active) {
                    continue;
                }
                let at = S::S_FIRST + count * S::DRAW;
                input[at..at + S::DRAW].fill(0.);
                input[at + S::D_PROGRAM] = -1.;
                input[at + S::D_GEOMETRY] = id as f64;
                count += 1;
            }
            input[S::S_DRAWS] = count as f64;
            let role = R::SHADOW_STATIC0 + cascade;
            let row = &mut bridge.r.frames.bindings[role * B::LEN..(role + 1) * B::LEN];
            row.fill(0.);
            row[B::TARGET] = -1.;
            row[B::SAMPLE] = -1.;
            row[B::WIDTH] = SIZE as f64;
            row[B::HEIGHT] = SIZE as f64;
            bridge.r.frames.meta[role * M::LEN + M::BOUND] = 1.;
            bridge.r.frames.meta[role * M::LEN + M::KIND] = K::SHADOW as f64;
            bridge.frame_publish_frame(role as u32);
            let at = cascade * bfe_render::cameras::layout::cascade::LEN;
            bridge.r.frames.plan.facts[F::SHADOW_DIRTY + cascade] =
                bridge.r.cameras.cascades.output[at + bfe_render::cameras::layout::cascade::DIRTY];
            bridge.r.frames.plan.facts[F::SHADOW_MOVED + cascade] =
                bridge.r.cameras.cascades.output[at + bfe_render::cameras::layout::cascade::MOVED];
            // Copy cached static depth into the live comparison map before
            // moving casters. FramePlan retains its last-cascade cadence.
            bridge.r.shadow.input[S::S_TARGET] = self.maps[cascade] as f64;
            bridge.r.shadow.input[S::S_FROM] = self.caches[cascade] as f64;
            let shadow_view = &bridge.r.shadow.static_view;
            self.casters.publish(
                &bridge.r.core,
                &mut bridge.r.shadow.input,
                records,
                &shadow_view[16..32],
                &shadow_view[..16],
            )?;
            let live_role = R::SHADOW_DYNAMIC0 + cascade;
            let binding: [f64; B::LEN] = bridge.r.frames.bindings
                [role * B::LEN..(role + 1) * B::LEN]
                .try_into()
                .unwrap();
            bridge.r.frames.bindings[live_role * B::LEN..(live_role + 1) * B::LEN]
                .copy_from_slice(&binding);
            bridge.r.frames.meta[live_role * M::LEN + M::BOUND] = 1.;
            bridge.r.frames.meta[live_role * M::LEN + M::KIND] = K::SHADOW as f64;
            bridge.frame_publish_frame(live_role as u32);
        }
        bridge.r.frames.plan.facts[F::SHADOW_COUNT] = COUNT as f64;
        bridge.camera_shadow_shader_frame();
        view.values[SF::CASCADE_NEAR] = bridge.r.cameras.cascades.range[0];
        view.values[SF::CASCADE_FAR] = bridge.r.cameras.cascades.range[1];
        Ok(())
    }
}
