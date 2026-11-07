// Graphics camera publications use fixed native storage. Scalar commands
// preserve the old phase boundaries without returning allocated arrays.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
impl BfeRenderer {
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
    pub fn camera_pack_shader_frame(&mut self,pack:u32){
        use crate::cameras::layout::{pack as K,limit as L};
        let pack=pack as usize;if pack>=L::PACKS{self.r.core.input[crate::shader::FRAME_LIGHT_COUNT]=0.;return;}
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
    pub fn camera_lights_frame(&mut self,pack:u32){self.r.cameras.lights(pack as usize);}
    pub fn camera_shadow_configure_frame(&mut self){self.r.cameras.cascades.configure(&self.r.cameras.input);}
    pub fn camera_cascades_frame(&mut self){self.r.cameras.cascades();}
    pub fn camera_shadow_frame(&mut self,index:u32){self.r.cameras.cascades.publish(index as usize);}
    pub fn camera_invalidate_frame(&mut self,index:u32){self.r.cameras.cascades.invalidate_window(index as usize);}
}
