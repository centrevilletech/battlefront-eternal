//! Pixel-visible primitives used by FountainJets.foamPixels and water geometry.
//! Native fixtures use V8 arithmetic; Electron supplies its own Math builtins,
//! following the environment phase's scalar host-math boundary.
pub trait Math {fn pow(x:f64,y:f64)->f64;fn sin(x:f64)->f64;fn cos(x:f64)->f64;fn hypot(x:f64,y:f64)->f64;fn hypot3(x:f64,y:f64,z:f64)->f64;}
// Native fixtures use the same operation order as Electron's Math builtins.
// std::powf and the platform libm are not pixel-byte reference oracles.
pub struct V8;
impl Math for V8 {
    fn pow(x:f64,y:f64)->f64{v8math::pow(x,y)}
    fn sin(x:f64)->f64{v8math::sin(x)}
    fn cos(x:f64)->f64{v8math::cos(x)}
    fn hypot(x:f64,y:f64)->f64{v8math::hypot(x,y)}
    fn hypot3(x:f64,y:f64,z:f64)->f64{v8math::hypot3(x,y,z)}
}
// FountainJets.foamPixels and procedural water geometry use scalar imports
// only at rounding-sensitive pixels. No object or state algorithm calls back
// into JavaScript; all vertex loops and descriptor decisions remain native.
#[cfg(target_arch="wasm32")]
mod page {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace=Math,js_name=pow)] fn pow(x:f64,y:f64)->f64;
        #[wasm_bindgen(js_namespace=Math,js_name=sin)] fn sin(x:f64)->f64;
        #[wasm_bindgen(js_namespace=Math,js_name=cos)] fn cos(x:f64)->f64;
        #[wasm_bindgen(js_namespace=Math,js_name=hypot)] fn hypot2(x:f64,y:f64)->f64;
        #[wasm_bindgen(js_namespace=Math,js_name=hypot)] fn hypot3(x:f64,y:f64,z:f64)->f64;
    }
    // This is the existing environment phase host-math pattern.
    pub struct Page;
    impl super::Math for Page {
        fn pow(x:f64,y:f64)->f64{pow(x,y)}
        fn sin(x:f64)->f64{sin(x)}
        fn cos(x:f64)->f64{cos(x)}
        fn hypot(x:f64,y:f64)->f64{hypot2(x,y)}
        fn hypot3(x:f64,y:f64,z:f64)->f64{hypot3(x,y,z)}
    }
}
#[cfg(target_arch="wasm32")]
pub use page::Page;
