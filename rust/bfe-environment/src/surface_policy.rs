//! Water.farSpan, farInView and resizeReflection are per-view surface policy.
//! Numeric camera/settings facts cross once; no host projection, frustum,
//! temporary vector or callback scans the authored water-cell boxes.
use crate::{layout::{water_view as input,water_view_result as out},surface_math::Math};
use three::{Matrix4,Vector3,CoordinateSystem};
use serde_json::Value;
pub struct Policy {pub input:[f64;input::STRIDE],pub result:[f64;out::STRIDE],bounds:([f64;3],[f64;3]),cells:Vec<[f64;6]>}
impl Default for Policy{fn default()->Self{Self{input:[0.;input::STRIDE],result:[0.;out::STRIDE],bounds:([0.;3],[0.;3]),cells:vec![]}}}
impl Policy {
    pub fn configure(&mut self,data:&Value){let recipe=crate::content::recipe(4,data);
        self.bounds=(std::array::from_fn(|i|recipe["bounds"][0][i].as_f64().unwrap_or(f64::INFINITY)),std::array::from_fn(|i|recipe["bounds"][1][i].as_f64().unwrap_or(f64::NEG_INFINITY)));
        self.cells=recipe["cells"].as_array().into_iter().flatten().map(|v|std::array::from_fn(|i|v[i].as_f64().unwrap_or(0.))).collect();
    }
    // farSpan preserves the near fade end as its start, even when no far
    // range survives. The caller's far camera sets the distance scale.
    pub fn update<M:Math>(&mut self){let c=&self.input;let r=&mut self.result;let end=c[input::FAR];let fade=c[input::FADE];
        r[out::RANGE]=c[input::FADE+1];r[out::RANGE+1]=0.;r[out::VISIBLE]=0.;
        if end>fade{let scale=if c[input::AUTHORED_FAR]>0.{end/c[input::AUTHORED_FAR]}else{1.};r[out::RANGE+1]=if c[input::WATER_FAR]>0.{c[input::WATER_FAR]*scale}else{end};}
        // resizeReflection limits the longer render dimension. Low retains
        // the original fixed target size even though its scale is zero.
        let scale=v8math::min(c[input::SCALE],c[input::MAX_SIZE]/v8math::max(c[input::SIZE],c[input::SIZE+1]));
        r[out::DIMENSIONS]=if c[input::SCALE]!=0.{v8math::max(1.,jsrt::conv::js_round(c[input::SIZE]*scale))}else{512.};
        r[out::DIMENSIONS+1]=if c[input::SCALE]!=0.{v8math::max(1.,jsrt::conv::js_round(c[input::SIZE+1]*scale))}else{256.};
        let enhanced=c[input::SCALE]>0.;r[out::ENHANCED]=u8::from(enhanced) as f64;r[out::PLANAR]=u8::from(enhanced&&c[input::PLANAR]!=0.) as f64;
        r[out::DISTORT]=if c[input::HAS_BUMPS]!=0.{if enhanced{0.03*0.18}else{0.03}}else{0.};r[out::UNDERWATER]=u8::from(c[input::EYE_Y]<c[input::LEVEL]) as f64;
        if !(r[out::RANGE+1]>r[out::RANGE]){return;}
        // PerspectiveCamera.updateProjectionMatrix uses a fresh camera, with
        // its default zoom and no film/view offset, for this visibility query.
        let near=v8math::max(c[input::NEAR],0.1);let top=near*M::tan((std::f64::consts::PI/180.)*0.5*c[input::FOV]);let height=2.*top;let width=c[input::ASPECT]*height;let left=-0.5*width;
        let mut projection=Matrix4::default();projection.make_perspective(left,left+width,top,top-height,near,r[out::RANGE+1],CoordinateSystem::WebGL);
        let inverse=Matrix4{elements:c[input::INVERSE..input::INVERSE+16].try_into().unwrap()};let mut matrix=Matrix4::default();matrix.multiply_matrices(&projection,&inverse);
        let mut frustum=crate::common::Frustum::default();frustum.set_from_projection_matrix(&matrix);
        let v=|a:[f64;3]|Vector3::new(a[0],a[1],a[2]);if !frustum.intersects_box(&v(self.bounds.0),&v(self.bounds.1)){return;}
        r[out::VISIBLE]=u8::from(self.cells.iter().any(|b|frustum.intersects_box(&Vector3::new(b[0],b[1],b[2]),&Vector3::new(b[3],b[4],b[5])))) as f64;
    }
}
#[cfg(target_arch="wasm32")]
mod exports {
    use super::*;use std::cell::RefCell;
    thread_local!{pub(super) static POLICY:RefCell<Policy>=RefCell::new(Policy::default());}
    #[no_mangle]pub extern "C" fn water_view_input()->*mut f64{POLICY.with_borrow_mut(|p|p.input.as_mut_ptr())}
    #[no_mangle]pub extern "C" fn water_view_output()->*const f64{POLICY.with_borrow(|p|p.result.as_ptr())}
    #[no_mangle]pub extern "C" fn water_view(){POLICY.with_borrow_mut(|p|p.update::<crate::surface_math::Page>())}
}
#[cfg(target_arch="wasm32")]
pub fn configure(data:&Value){exports::POLICY.with_borrow_mut(|p|p.configure(data))}
