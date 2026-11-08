// Graphics camera publications use fixed native storage. Scalar commands
// preserve the old phase boundaries without returning allocated arrays.
use super::BfeRenderer;
mod cpu;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
    pub fn camera_scene_lights_frame(&mut self,root:u32,generation:u32,role:u32,source:u32,source_generation:u32,view:bool,now:f64,handle:f64){let Some(pack)=self.r.cameras.pack_leases.touch(handle)else{return;};self.r.scene.point_lights_frame(&mut self.r.cameras,root as usize,generation as f64,role as usize,source as usize,source_generation as f64,view,now,pack);}
    // Scene selection reads the same camera publication as shader draws.
    // Query receipts compare native f64 words without a camera facade copy.
    pub fn camera_scene_view_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as C,role as R,lens as N};
        use crate::scene::layout::scene_view as V;
        let role=role as usize;if role>=R::COUNT{return;}
        let a=&self.r.cameras.output[role*C::LEN..(role+1)*C::LEN];let out=&mut self.r.scene.view;
        out[V::PROJECTION..V::PROJECTION+16].copy_from_slice(&a[C::PROJECTION..C::PROJECTION+16]);
        out[V::VIEW..V::VIEW+16].copy_from_slice(&a[C::INVERSE..C::INVERSE+16]);
        out[V::EYE..V::EYE+3].copy_from_slice(&a[C::WORLD+12..C::WORLD+15]);out[V::ZOOM]=a[C::LENS+N::ZOOM];
    }
    pub fn camera_post_facts_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as C,role as R};use crate::post_chain::layout::facts as F;
        let role=role as usize;if role>=R::COUNT{return;}
        let a=&self.r.cameras.output[role*C::LEN..(role+1)*C::LEN];let out=&mut self.r.post_chain.input;
        for (to,from) in [(F::PROJECTION,C::PROJECTION),(F::VIEW,C::INVERSE),(F::WORLD,C::WORLD)]{out[to..to+16].copy_from_slice(&a[from..from+16]);}
    }
    pub fn scene_mirrored_frame(&self,id:u32,generation:u32)->bool{
        if self.r.scene.pose_ref(id as usize,generation as f64).is_none(){return false;}
        self.r.scene.world(id as usize).is_some_and(|world|crate::math::determinant_mirrors(&world))
    }
    pub fn camera_canopy_view_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as C,role as R};use crate::scene::layout::scene_canopy_view as V;
        let role=role as usize;if role>=R::COUNT{return;}
        let a=&self.r.cameras.output[role*C::LEN..(role+1)*C::LEN];let out=&mut self.r.scene.canopies.view;
        for(to,from)in[(V::PROJECTION,C::PROJECTION),(V::VIEW,C::INVERSE),(V::WORLD,C::WORLD)]{out[to..to+16].copy_from_slice(&a[from..from+16]);}
        out[V::EYE..V::EYE+3].copy_from_slice(&a[C::POSITION..C::POSITION+3]);
    }
    pub fn camera_foliage_view_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as C,role as R};use crate::foliage as F;
        let role=role as usize;if role>=R::COUNT{return;}
        let a=&self.r.cameras.output[role*C::LEN..(role+1)*C::LEN];let out=&mut self.r.foliage.input;
        for(to,from)in[(F::IN_PROJECTION,C::PROJECTION),(F::IN_VIEW,C::INVERSE),(F::IN_WORLD,C::WORLD)]{out[to..to+16].copy_from_slice(&a[from..from+16]);}
        out[F::IN_EYE..F::IN_EYE+3].copy_from_slice(&a[C::POSITION..C::POSITION+3]);
    }
    pub fn camera_sky_nodes_frame(&mut self,nodes:&[u32],single:u32,generation:u32){self.r.cameras.sky_nodes(nodes,single,generation);}
    pub fn camera_scene_sky_frame(&mut self,role:u32,follow:bool){self.r.cameras.scene_sky(&mut self.r.scene,role as usize,follow);}
    pub fn camera_scene_sky_save_frame(&mut self){self.r.cameras.save_sky(&self.r.scene);}
    pub fn camera_scene_sky_restore_frame(&mut self){self.r.cameras.restore_sky(&mut self.r.scene);}
    pub fn camera_unit_view_ptr(&mut self)->u32{self.r.cameras.unit_view.as_mut_ptr() as u32}
    pub fn camera_unit_view_len(&self)->u32{self.r.cameras.unit_view.len() as u32}
    pub fn camera_unit_view_frame(&mut self,role:u32){
        self.camera_projection_view_frame(role);let frustum=crate::math::Frustum::from_matrix(&self.r.cameras.projection_view);
        for (i,plane) in frustum.planes.iter().enumerate(){self.r.cameras.unit_view[i*4..i*4+3].copy_from_slice(&plane.n);self.r.cameras.unit_view[i*4+3]=plane.c;}
    }
    pub fn camera_depth_key_ptr(&mut self)->u32{self.r.cameras.depth.key.as_mut_ptr() as u32}
    pub fn camera_depth_key_len(&self)->u32{self.r.cameras.depth.key.len() as u32}
    pub fn camera_depth_marker_ptr(&mut self)->u32{self.r.cameras.depth.output.as_mut_ptr() as u32}
    pub fn camera_depth_marker_len(&self)->u32{self.r.cameras.depth.output.len() as u32}
    pub fn camera_depth_begin_frame(&mut self,role:u32){self.camera_projection_view_frame(role);self.r.cameras.depth.begin(&self.r.cameras.projection_view);}
    pub fn camera_depth_request_frame(&mut self)->bool{self.r.cameras.depth.request()}
    pub fn camera_depth_finish_frame(&mut self){self.r.cameras.depth.finish();}
    pub fn camera_projection_view_ptr(&mut self)->u32{self.r.cameras.projection_view.as_mut_ptr() as u32}
    pub fn camera_projection_view_len(&self)->u32{self.r.cameras.projection_view.len() as u32}
    pub fn camera_projection_view_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as c,role as r};let role=role as usize;if role>=r::COUNT{return;}
        let source=&self.r.cameras.output[role*c::LEN..(role+1)*c::LEN];let projection=source[c::PROJECTION..c::PROJECTION+16].try_into().unwrap_or([0.;16]);let view=source[c::INVERSE..c::INVERSE+16].try_into().unwrap_or([0.;16]);self.r.cameras.projection_view=crate::math::mul(&projection,&view);
    }
    pub fn camera_scene_uniform_frame(&mut self,id:u32,generation:u32,role:u32){
        use crate::cameras::layout::{uniform_input as i,camera as c,role as r};let role=role as usize;if role>=r::COUNT{return;}
        let Some(world)=self.r.scene.pose_ref(id as usize,generation as f64).and_then(|_|self.r.scene.world(id as usize))else{return;};
        let camera=&self.r.cameras.output[role*c::LEN..(role+1)*c::LEN];let a=&mut self.r.cameras.uniforms.input;
        a[i::WORLD..i::WORLD+16].copy_from_slice(&world);a[i::VIEW..i::VIEW+16].copy_from_slice(&camera[c::INVERSE..c::INVERSE+16]);a[i::PROJECTION..i::PROJECTION+16].copy_from_slice(&camera[c::PROJECTION..c::PROJECTION+16]);a[i::EYE..i::EYE+3].copy_from_slice(&camera[c::WORLD+12..c::WORLD+15]);self.r.cameras.uniforms.frame(false);
    }
    pub fn camera_uniform_input_ptr(&mut self)->u32{self.r.cameras.uniforms.input.as_mut_ptr() as u32}
    pub fn camera_uniform_input_len(&self)->u32{self.r.cameras.uniforms.input.len() as u32}
    pub fn camera_uniform_output_ptr(&mut self)->u32{self.r.cameras.uniforms.output.as_mut_ptr() as u32}
    pub fn camera_uniform_output_len(&self)->u32{self.r.cameras.uniforms.output.len() as u32}
    pub fn camera_uniform_frame(&mut self,uv:bool){self.r.cameras.uniforms.frame(uv);}
    pub fn camera_mirrored_frame(&self)->bool{
        use crate::cameras::layout::uniform_input as I;
        let world=self.r.cameras.uniforms.input[I::WORLD..I::WORLD+16].try_into().unwrap_or([0.;16]);
        crate::math::determinant_mirrors(&world)
    }
    pub fn camera_mirror_bounds_frame(&mut self,bounds:&[f64]){self.r.cameras.mirror_bounds(bounds);}
    pub fn camera_mirror_visibility_frame(&mut self){self.r.cameras.mirror_visibility();}
    pub fn camera_placement_ptr(&mut self)->u32 {self.r.cameras.placement.as_mut_ptr() as u32}
    pub fn camera_placement_len(&self)->u32 {self.r.cameras.placement.len() as u32}
    pub fn camera_placement_frame(&mut self,count:u32){self.r.cameras.placement(count as usize);}
    pub fn camera_input_ptr(&mut self)->u32 {self.r.cameras.input.as_mut_ptr() as u32}
    pub fn camera_input_len(&self)->u32 {self.r.cameras.input.len() as u32}
    pub fn camera_output_ptr(&mut self)->u32 {self.r.cameras.output.as_mut_ptr() as u32}
    pub fn camera_output_len(&self)->u32 {self.r.cameras.output.len() as u32}
    pub fn camera_windows_ptr(&mut self)->u32 {self.r.cameras.cascades.output.as_mut_ptr() as u32}
    pub fn camera_windows_len(&self)->u32 {self.r.cameras.cascades.output.len() as u32}
    pub fn camera_sky_ptr(&mut self)->u32 {self.r.cameras.sky.as_mut_ptr() as u32}
    pub fn camera_sky_len(&self)->u32 {self.r.cameras.sky.len() as u32}
    pub fn camera_lights_ptr(&mut self)->u32 {self.r.cameras.light_input.as_mut_ptr() as u32}
    pub fn camera_lights_len(&self)->u32 {self.r.cameras.light_input.len() as u32}
    pub fn camera_packs_ptr(&mut self)->u32 {self.r.cameras.packs.as_mut_ptr() as u32}
    pub fn camera_packs_len(&self)->u32 {self.r.cameras.packs.len() as u32}
    pub fn camera_pack_handles_ptr(&mut self)->u32 {self.r.cameras.pack_leases.handles.as_mut_ptr() as u32}
    pub fn camera_pack_handles_len(&self)->u32 {crate::cameras::layout::limit::PACKS as u32}
    pub fn camera_pack_acquire_frame(&mut self)->f64 {self.r.cameras.pack_leases.acquire()}
    pub fn camera_pack_release_frame(&mut self,handle:f64)->bool {self.r.cameras.pack_leases.release(handle)}
    pub fn camera_extra_handles_ptr(&mut self)->u32 {self.r.cameras.extra_leases.handles.as_mut_ptr() as u32}
    pub fn camera_extra_handles_len(&self)->u32 {crate::cameras::layout::limit::EXTRA_CAMERAS as u32}
    pub fn camera_acquire_frame(&mut self)->f64 {self.r.cameras.extra_leases.acquire()}
    pub fn camera_release_frame(&mut self,handle:f64)->bool {self.r.cameras.extra_leases.release(handle)}
    pub fn camera_import_frame(&mut self,handle:f64)->u32 {self.r.cameras.import(handle).map(|role|role as u32).unwrap_or(u32::MAX)}
    // Diagnostics read lifetime capacity counters without copying camera banks.
    pub fn camera_clipped(&self)->bool {self.r.cameras.clipped}
    pub fn camera_evicted(&self)->u32 {self.r.cameras.extra_leases.evicted}
    pub fn camera_pack_evicted(&self)->u32 {self.r.cameras.pack_leases.evicted}
    pub fn camera_pack_rejected(&self)->u32 {self.r.cameras.rejected_packs}
    pub fn camera_mirror_ptr(&mut self)->u32 {self.r.cameras.mirror.as_mut_ptr() as u32}
    pub fn camera_mirror_len(&self)->u32 {self.r.cameras.mirror.len() as u32}
    // Shader blocks consume the native matrices and light pack directly;
    // readback shells serve existing non-draw and graph consumers only.
    pub fn camera_shader_frame(&mut self,role:u32){
        use crate::cameras::layout::{camera as C,role as R,shader_frame as F};
        let role=role as usize;if role>=R::COUNT{return;}
        let row=&self.r.cameras.output[role*C::LEN..(role+1)*C::LEN];
        self.r.core.input[F::PROJECTION..F::PROJECTION+16].copy_from_slice(&row[C::PROJECTION..C::PROJECTION+16]);
        self.r.core.input[F::VIEW..F::VIEW+16].copy_from_slice(&row[C::INVERSE..C::INVERSE+16]);
        self.r.core.input[F::EYE..F::EYE+3].copy_from_slice(&row[C::WORLD+12..C::WORLD+15]);
    }
    pub fn camera_pack_shader_frame(&mut self,handle:f64){
        use crate::cameras::layout::pack as K;
        let Some(pack)=self.r.cameras.pack_leases.touch(handle)else{self.r.core.input[crate::shader::FRAME_LIGHT_COUNT]=0.;self.r.cameras.rejected_packs=self.r.cameras.rejected_packs.saturating_add(1);return;};
        let row=&self.r.cameras.packs[pack*K::LEN..(pack+1)*K::LEN];let count=(row[K::COUNT] as usize).min(16);
        self.r.core.input[crate::shader::FRAME_LIGHT_COUNT]=count as f64;self.r.core.input[crate::shader::FRAME_LIGHTS..crate::shader::FRAME_LIGHTS+count*8].copy_from_slice(&row[K::VALUES..K::VALUES+count*8]);
    }
    pub fn camera_shadow_shader_frame(&mut self){
        use crate::cameras::layout::input as I;
        self.r.cameras.cascades.write_shadow(&mut self.r.core.shadow_input,&self.r.cameras.input[I::SHADOW_METADATA..I::SHADOW_METADATA+24]);
    }
    pub fn camera_static_shadow_frame(&mut self,index:u32){
        use crate::cameras::layout::{camera as C,role as R,limit as L};
        use crate::static_layout as S;
        let index=index as usize;if index>=L::CASCADES{return;}
        let row=&self.r.cameras.output[(R::SHADOW0+index)*C::LEN..(R::SHADOW0+index+1)*C::LEN];
        self.r.shadow.static_view[S::SHADOW_PROJECTION..S::SHADOW_PROJECTION+16].copy_from_slice(&row[C::PROJECTION..C::PROJECTION+16]);
        self.r.shadow.static_view[S::SHADOW_VIEW..S::SHADOW_VIEW+16].copy_from_slice(&row[C::INVERSE..C::INVERSE+16]);
    }
    pub fn camera_project_frame(&mut self){self.r.cameras.project_main();}
    pub fn camera_light_input_ptr(&mut self)->u32{self.r.cameras.settings.input.as_mut_ptr() as u32}
    pub fn camera_light_input_len(&self)->u32{self.r.cameras.settings.input.len() as u32}
    pub fn camera_light_output_ptr(&mut self)->u32{self.r.cameras.settings.output.as_mut_ptr() as u32}
    pub fn camera_light_output_len(&self)->u32{self.r.cameras.settings.output.len() as u32}
    pub fn camera_light_command_frame(&mut self,command:u32){self.r.cameras.settings.command(command as usize);}
    pub fn camera_global_shader_frame(&mut self){
        use crate::cameras::layout::{light_output as O,shader_frame as F};
        let a=&self.r.cameras.settings.output;let out=&mut self.r.core.input;
        out[F::SUN_DIRECTION..F::SUN_DIRECTION+3].copy_from_slice(&a[O::DIRECTION..O::DIRECTION+3]);out[F::SHADOW_STRENGTH]=a[O::STRENGTH];
        for(i,at)in [F::SUN_COLOR,F::AMBIENT,F::AMBIENT_TOP,F::AMBIENT_BOTTOM,F::CHARACTER_AMBIENT,F::FOG_COLOR].into_iter().enumerate(){out[at..at+3].copy_from_slice(&a[O::COLORS+i*3..O::COLORS+i*3+3]);}
        out[F::CASCADE_NEAR]=self.r.cameras.cascades.range[0];out[F::CASCADE_FAR]=self.r.cameras.cascades.range[1];
        out[F::FOG_RANGE..F::FOG_RANGE+2].copy_from_slice(&a[O::RANGE..O::RANGE+2]);
    }
    pub fn camera_main_frame(&mut self)->bool{self.r.cameras.main()}
    pub fn camera_derived_frame(&mut self,role:u32){self.r.cameras.derived(role as usize);}
    pub fn camera_copy_frame(&mut self,from:u32,to:u32){self.r.cameras.copy(from as usize,to as usize);}
    pub fn camera_shift_frame(&mut self,role:u32){self.r.cameras.shift(role as usize);}
    pub fn camera_restore_frame(&mut self){self.r.cameras.restore();}
    pub fn camera_reflect_frame(&mut self){self.r.cameras.reflect();}
    pub fn camera_probe_frame(&mut self,slot:u32){self.r.cameras.probe(slot as usize);}
    pub fn camera_sky_frame(&mut self,role:u32,follow:bool){self.r.cameras.sky(role as usize,follow);}
    pub fn camera_lights_frame(&mut self,handle:f64){self.r.cameras.pack_lights(handle);}
    pub fn camera_shadow_configure_frame(&mut self){self.r.cameras.cascades.configure(&self.r.cameras.input);}
    pub fn camera_cascades_frame(&mut self){self.r.cameras.cascades();}
    pub fn camera_shadow_frame(&mut self,index:u32){self.r.cameras.cascades.publish(index as usize);}
    pub fn camera_invalidate_frame(&mut self,index:u32){self.r.cameras.cascades.invalidate_window(index as usize);}
}
