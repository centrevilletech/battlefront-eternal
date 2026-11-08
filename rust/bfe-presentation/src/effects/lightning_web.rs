//! Existing collision memory remains a foreign service of presentation. The
//! browser answers prepared rays; all ray construction and hit policy is Rust.
use wasm_bindgen::prelude::*;
use three::Vector3;
#[wasm_bindgen]
extern "C" {
    pub type Host;
    #[wasm_bindgen(method, structural)]
    fn cast(this: &Host, x:f64,y:f64,z:f64,dx:f64,dy:f64,dz:f64,length:f64)->f64;
    #[wasm_bindgen(method, structural)]
    fn normal(this: &Host,axis:usize)->f64;
}
pub struct BrowserCollision<'a>(pub &'a Host);
impl super::lightning::Collision for BrowserCollision<'_>{
    fn cast(&mut self,from:Vector3,dir:Vector3,length:f64)->Option<(f64,Vector3)>{
        let distance=self.0.cast(from.x,from.y,from.z,dir.x,dir.y,dir.z,length);
        if distance<0.{None}else{Some((distance,Vector3::new(self.0.normal(0),self.0.normal(1),self.0.normal(2))))}
    }
}
