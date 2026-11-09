//! setLighting/setFog/Graphics.setEnvironment and StormSky's flash. Map RGB
//! divides by 255; environment RGB multiplies by its reciprocal. Their final
//! f64 bits differ, so those semantic commands deliberately remain distinct.
use crate::cameras::layout::{light_input as I,light_output as O,light_command as C,light_flag as F};
use jsrt::conv::{js_min,js_max};
use three::Vector3;
pub struct LightSettings {
    pub input:Vec<f64>,pub output:Vec<f64>,base:[f64;12],flashing:bool,
}
impl Default for LightSettings {
    fn default()->Self{Self{input:vec![0.;I::LEN],output:vec![0.;O::LEN],base:[0.;12],flashing:false}}
}
impl LightSettings {
    pub fn command(&mut self,command:usize){
        let a=&self.input;let out=&mut self.output;
        match command {
            C::SEED=>{out[..O::LEN].copy_from_slice(&a[..O::LEN]);},
            C::MAP=>{
                let mut d=Vector3::new(a[I::DIRECTION],a[I::DIRECTION+1],a[I::DIRECTION+2]);d.normalize();out[O::DIRECTION..O::DIRECTION+3].copy_from_slice(&[d.x,d.y,d.z]);
                for i in 0..15{out[O::COLORS+i]=a[I::COLORS+i]/255.;}out[O::STRENGTH]=1.;
            },
            C::ENVIRONMENT=>{
                let flags=a[I::FLAGS] as usize;
                if flags&F::DIRECTION!=0 {let mut d=Vector3::new(a[I::DIRECTION],a[I::DIRECTION+1],a[I::DIRECTION+2]);if d.length_sq()>0.{d.normalize();out[O::DIRECTION..O::DIRECTION+3].copy_from_slice(&[d.x,d.y,d.z]);}}
                for i in 0..6{if flags&(1<<i)!=0 {for k in 0..3{out[O::COLORS+i*3+k]=a[I::COLORS+i*3+k]*(1./255.);}}}
                if flags&F::RANGE!=0{let r=&a[I::RANGE..I::RANGE+2];let valid=r[0]>=0.&&r[0]<r[1];out[O::RANGE]=if valid{r[0]}else{1e9};out[O::RANGE+1]=if valid{r[1]}else{2e9};}
                if flags&F::STRENGTH!=0{out[O::STRENGTH]=js_max(0.,js_min(1.,a[I::STRENGTH]));}
            },
            C::FOG=>{
                if a[I::FLAGS]!=0.{for k in 0..3{out[O::COLORS+15+k]=a[I::COLORS+15+k]/255.;}}
                let valid=a[I::FLAGS]!=0.&&a[I::RANGE]>=0.&&a[I::RANGE]<a[I::RANGE+1];out[O::RANGE]=if valid{a[I::RANGE]}else{1e9};out[O::RANGE+1]=if valid{a[I::RANGE+1]}else{2e9};
            },
            // StormSky restores before sampling the next native surface flash.
            // The saved base belongs to this owner, including paused frames.
            C::FLASH=>{
                self.base.copy_from_slice(&out[O::COLORS+3..O::COLORS+15]);
                for i in 0..4{let at=O::COLORS+3+i*3;out[at]+=a[I::FLASH]*0.10;out[at+1]+=a[I::FLASH]*0.13;out[at+2]+=a[I::FLASH]*0.18;}self.flashing=true;
            },
            C::RESTORE if self.flashing=>{out[O::COLORS+3..O::COLORS+15].copy_from_slice(&self.base);self.flashing=false;},
            _=>{},
        }
    }
}
