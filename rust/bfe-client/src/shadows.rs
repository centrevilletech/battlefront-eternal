//! Native sun cascades reuse the renderer's static-depth and camera owners.
use crate::{render_bridge::Bridge, resources::Resources, scene::View};
use bfe_render::{
    cameras::layout::{input as I, lens as N, light_output as L, shader_frame as SF, source as U},
    frame_plan::layout::{binding as B, facts as F, kind as K, meta as M, role as R},
    shadow as S,
};

pub fn quality(choice:&str)->(usize,usize){match choice{"off"=>(0,0),"low"=>(2,1024),"medium"=>(3,2048),"ultra"=>(4,4096),_=>(4,2048)}}

pub struct Shadows {
    maps: Vec<u32>,
    samples: Vec<(u32,u32)>,
    size: usize,
    frame_ms: f64,
    slow: bool,
    caches: Vec<u32>,
    caster_bounds: [Option<[f64; 4]>; 4],
    casters: bfe_render::native_casters::Casters,
}
impl Shadows {
    pub async fn new(
        bridge: &mut Bridge,
        resources: &mut Resources,
        bones: usize,
        count: usize,
        size: usize,
    ) -> Result<Self, String> {
        let mut maps = vec![0; count];
        let mut caches = vec![0; count];
        let mut samples = Vec::new();
        for map in maps.iter_mut().chain(caches.iter_mut()) {
            crate::loading_work::frame().await;
            bridge.r.core.state.reset();
            // Graphics.shadowTarget uses unsigned depth with comparison
            // filtering. Resource admission reports unsupported storage as an
            // error rather than silently removing shadows on a driver.
            let source = resources.source(&serde_json::json!({"width":size,"height":size,"depth":1,"target":glow::TEXTURE_2D,"format":1026,"dataType":1014,"srgb":false,"renderTarget":true}).to_string())?;
            let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":515}"#)?;
            resources.upload(sample)?;
            *map = bridge.r.core.add_texture(resources.texture(sample)?);
            samples.push((*map,sample));
        }
        let mut skins = Vec::with_capacity(bfe_render::instances::MAX_RECORDS);
        let skin_size = bfe_render::actor_depth::skin_size(bones);
        for index in 0..bfe_render::instances::MAX_RECORDS {
            if index % 32 == 0 {
                crate::loading_work::frame().await;
                bridge.r.core.state.reset();
            }
            let source = resources.source(&serde_json::json!({"width":skin_size,"height":skin_size,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1015,"srgb":false,"renderTarget":true}).to_string())?;
            let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#)?;
            resources.upload(sample)?;
            let texture=bridge.r.core.add_texture(resources.texture(sample)?);
            samples.push((texture,sample));
            skins.push(texture);
        }
        let casters = bfe_render::native_casters::Casters::new(&mut bridge.r.core, &skins, bones)?;
        bridge.set_shadow_maps(&maps);
        let input = &mut bridge.r.cameras.input;
        // Graphics.applyShadows' high preset and CSM light-space range.
        input[I::CASCADE_COUNT] = count as f64;
        input[I::MAP_SIZE] = size as f64;
        input[I::MAX_FAR] = 300.;
        input[I::LIGHT_RANGE..I::LIGHT_RANGE + 2].copy_from_slice(&[1., 3000.]);
        input[I::MARGIN] = 400.;
        for row in input[I::SHADOW_METADATA..I::SHADOW_METADATA + count * 6].chunks_exact_mut(6) {
            row.copy_from_slice(&[1., -0.015 / 2999., 0., 1., size as f64, size as f64]);
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
        eprintln!("native shadows: cascades={count} size={size} comparison=LEQUAL");
        Ok(Self {
            maps, samples, size, frame_ms: 0., slow: false,
            caches, caster_bounds: [None; 4],
            casters,
        })
    }
    pub fn dispose(self,bridge:&mut Bridge,resources:&mut Resources)->Result<(),String>{
        bridge.set_shadow_maps(&[]);
        for(texture,sample)in self.samples{bridge.r.core.forget_texture(texture);resources.dispose_sample(sample)?;}
        bridge.r.cameras.input[I::CASCADE_COUNT]=0.;Ok(())
    }
    pub fn frame_time(&mut self, seconds: f64) {
        if !seconds.is_finite() || seconds <= 0. || seconds > 0.25 { return; }
        let ms = seconds * 1000.;
        self.frame_ms += 0.05 * (ms - self.frame_ms);
        if self.frame_ms > 1000. / 30. { self.slow = true; }
        else if self.frame_ms < 1000. / 40. { self.slow = false; }
    }
    pub fn publish(
        &mut self,
        bridge: &mut Bridge,
        view: &mut View,
        records: usize,
    ) -> Result<(), String> {
        let count=self.maps.len();let size=self.size;
        let skin_timer=bridge.r.core.profile.clock();
        self.casters.prepare(&mut bridge.r.core, records)?;
        bridge.r.core.profile.cpu("shadow skin upload",skin_timer);
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
        // Cascades use the displayed lens, including mounted seats and zoom.
        input[I::LENS + N::FOV] =
            2. * bfe_pose::rig::math::atan(1. / view.values[5]).to_degrees();
        input[I::LENS + N::ASPECT] = view.values[5] / view.values[0];
        input[I::LENS + N::NEAR] = view.values[14] / (view.values[10] - 1.);
        input[I::LENS + N::FAR] = view.values[14] / (view.values[10] + 1.);
        bridge.r.cameras.settings.output[L::DIRECTION..L::DIRECTION + 3]
            .copy_from_slice(&view.values[SF::SUN_DIRECTION..SF::SUN_DIRECTION + 3]);
        bridge.r.cameras.main();
        bridge.camera_cascades_frame();
        bridge.r.frames.plan.shadow_periods = if self.slow { [1, 2, 4, 4] } else { [1; 4] };
        bridge.r.frames.plan.facts[F::SHADOW_COUNT] = count as f64;
        for cascade in 0..count {
            let at = cascade * bfe_render::cameras::layout::cascade::LEN;
            bridge.r.frames.plan.facts[F::SHADOW_DIRTY + cascade] = bridge.r.cameras.cascades.output[at + bfe_render::cameras::layout::cascade::DIRTY];
            bridge.r.frames.plan.facts[F::SHADOW_MOVED + cascade] = bridge.r.cameras.cascades.output[at + bfe_render::cameras::layout::cascade::MOVED];
            if !bridge.r.frames.plan.shadow_due(cascade) { continue; }
            bridge.camera_shadow_frame(cascade as u32);
            bridge.camera_static_shadow_frame(cascade as u32);
            bridge.r.shadow.static_view[bfe_render::static_layout::SHADOW_TIME] =
                view.values[SF::TIME];
            let input = &mut bridge.r.shadow.input;
            input[S::S_TARGET] = self.caches[cascade] as f64;
            input[S::S_FROM] = -1.;
            input[S::S_SIZE] = size as f64;
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
            row[B::WIDTH] = size as f64;
            row[B::HEIGHT] = size as f64;
            bridge.r.frames.meta[role * M::LEN + M::BOUND] = 1.;
            bridge.r.frames.meta[role * M::LEN + M::KIND] = K::SHADOW as f64;
            bridge.frame_publish_frame(role as u32);
            // Copy cached static depth into the live comparison map before
            // moving casters. FramePlan retains its last-cascade cadence.
            bridge.r.shadow.input[S::S_TARGET] = self.maps[cascade] as f64;
            bridge.r.shadow.input[S::S_FROM] = self.caches[cascade] as f64;
            let caster_timer=bridge.r.core.profile.clock();
            let shadow_view = &bridge.r.shadow.static_view;
            let bounds = self.casters.publish(
                &bridge.r.core,
                &mut bridge.r.shadow.input,
                records,
                &shadow_view[16..32],
                &shadow_view[..16],
            )?;
            bridge.r.core.profile.cpu("shadow caster lists",caster_timer);
            let live_role = R::SHADOW_DYNAMIC0 + cascade;
            let binding: [f64; B::LEN] = bridge.r.frames.bindings
                [role * B::LEN..(role + 1) * B::LEN]
                .try_into()
                .unwrap();
            bridge.r.frames.bindings[live_role * B::LEN..(live_role + 1) * B::LEN]
                .copy_from_slice(&binding);
            let invalid = bridge.r.frames.plan.facts[F::SHADOW_DIRTY + cascade] != 0.
                || bridge.r.frames.plan.facts[F::SHADOW_MOVED + cascade] != 0.;
            let rect = damage_rect(self.caster_bounds[cascade], bounds, size, invalid);
            self.caster_bounds[cascade] = bounds;
            let live = &mut bridge.r.frames.bindings[live_role * B::LEN..(live_role + 1) * B::LEN];
            live[B::CLIPPED] = 1.;
            live[B::SCISSOR..B::SCISSOR+4].copy_from_slice(&rect);
            bridge.r.frames.meta[live_role * M::LEN + M::BOUND] = 1.;
            bridge.r.frames.meta[live_role * M::LEN + M::KIND] = K::SHADOW as f64;
            bridge.frame_publish_frame(live_role as u32);
        }
        bridge.r.frames.plan.facts[F::SHADOW_COUNT] = count as f64;
        bridge.camera_shadow_shader_frame();
        view.values[SF::CASCADE_NEAR] = bridge.r.cameras.cascades.range[0];
        view.values[SF::CASCADE_FAR] = bridge.r.cameras.cascades.range[1];
        Ok(())
    }
}

// Apply the camera-distance limit to freshly published props and vehicles.
// Colour visibility and static world casters keep their own rules.
pub(crate) fn limit_distance(input: &mut [f64], first: usize, end: usize, far: f64) {
    use bfe_render::instances as I;
    let eye: [f64; 3] = input[SF::EYE..SF::EYE + 3].try_into().unwrap();
    for row in input[I::IN_RECORDS + first * I::REC_STRIDE..I::IN_RECORDS + end * I::REC_STRIDE]
        .chunks_exact_mut(I::REC_STRIDE)
    {
        let distance: f64 = (0..3).map(|i| (row[I::REC_WORLD + 12 + i] - eye[i]).powi(2)).sum();
        if !(distance < far * far) {
            row[I::REC_FLAGS] = (row[I::REC_FLAGS] as u32 | I::NO_SHADOW) as f64;
        }
    }
}

// Restore both the previous and current caster footprints from the static
// cache, erasing departed casters as well as preparing new ones. Unchanged
// static pixels elsewhere already contain exactly the same depth.
fn damage_rect(previous: Option<[f64; 4]>, current: Option<[f64; 4]>, size: usize, invalid: bool) -> [f64; 4] {
    let size = size as f64;
    if invalid { return [0., 0., size, size]; }
    let bounds = match (previous, current) {
        (None, None) => return [0.; 4],
        (Some(a), None) | (None, Some(a)) => a,
        (Some(a), Some(b)) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
    };
    let x0 = ((bounds[0] * 0.5 + 0.5) * size - 2.).floor().clamp(0., size);
    let y0 = ((bounds[1] * 0.5 + 0.5) * size - 2.).floor().clamp(0., size);
    let x1 = ((bounds[2] * 0.5 + 0.5) * size + 2.).ceil().clamp(x0, size);
    let y1 = ((bounds[3] * 0.5 + 0.5) * size + 2.).ceil().clamp(y0, size);
    [x0, y0, x1-x0, y1-y0]
}

#[cfg(test)]
mod tests {
    use super::{damage_rect, limit_distance};
    #[test]
    fn distant_props_stop_casting_without_losing_their_colour_records() {
        use bfe_render::instances as I;
        use bfe_render::cameras::layout::shader_frame as SF;
        let mut input = vec![0.; I::IN_RECORDS + 5 * I::REC_STRIDE];
        input[SF::EYE..SF::EYE + 3].copy_from_slice(&[100., 20., 30.]);
        for (i, position) in [[100., 20., 30.], [169.9, 20., 30.], [100., 90., 30.], [170.1, 20., 30.], [300., 20., 30.]].iter().enumerate() {
            let at = I::IN_RECORDS + i * I::REC_STRIDE;
            input[at + I::REC_WORLD + 12..at + I::REC_WORLD + 15].copy_from_slice(position);
            input[at + I::REC_FLAGS] = 14.;
            input[at + I::REC_KIND] = 1.;
        }
        let before = input.clone();
        limit_distance(&mut input, 1, 4, 70.);
        for i in 0..5 {
            let at = I::IN_RECORDS + i * I::REC_STRIDE;
            let expected = if i == 2 || i == 3 {14 | I::NO_SHADOW} else {14};
            assert_eq!(input[at + I::REC_FLAGS], expected as f64);
            input[at + I::REC_FLAGS] = 14.;
        }
        assert_eq!(input, before);
    }

    #[test]
    fn shadow_copy_erases_departed_casters_and_invalid_windows() {
        let old = Some([-0.5, -0.5, 0., 0.]);
        let new = Some([0., 0., 0.5, 0.5]);
        assert_eq!(damage_rect(old, new, 100, false), [23., 23., 54., 54.]);
        assert_eq!(damage_rect(old, None, 100, false), [23., 23., 29., 29.]);
        assert_eq!(damage_rect(None, None, 100, false), [0.; 4]);
        assert_eq!(damage_rect(old, new, 100, true), [0., 0., 100., 100.]);
    }
}
