//! Cold authored bolt styles and texture hue extraction. Resource identity and
//! image readiness stay with the host; none of their colour policy does.
use serde::{Deserialize, Serialize};
use v8math::{max, min, pow};

// Only authored numeric fields cross this cold boundary. Texture names and
// renderer batch identity are deliberately left with the resource host.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Content {
    #[serde(rename="base")] base: Option<String>,
    laser_texture: Option<String>,
    blur_length: Option<f64>,
    fade_out_time: Option<f64>,
    gravity: Option<f64>,
    laser_width: Option<f64>,
    laser_length: Option<f64>,
    glow_length: Option<f64>,
    light_radius: Option<f64>,
    laser_glow_color: Option<Vec<f64>>,
    light_color: Option<Vec<f64>>,
}

// The same result feeds the Classic colours and the Enhanced capsule look.
// Registering a texture hue later changes only the sheath colour.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Numbers {
    texture_hue: bool,
    pub color: [f64; 3],
    glow: f64,
    core: f64,
    length: f64,
    brightness: f64,
    light_radius: f64,
    pub light_color: [f64; 3],
    pub glow_color: [f64; 3],
    pub glow_length: f64,
}

// blaster-bolts.js sheathColor ignores authored alpha so a dim glow retains
// the texture's hue. The peak includes one, including for an all-black input.
pub fn sheath(rgba: &[f64]) -> [f64; 3] {
    let r = rgba.first().copied().unwrap_or(255.0);
    let g = rgba.get(1).copied().unwrap_or(255.0);
    let b = rgba.get(2).copied().unwrap_or(255.0);
    let peak = max(max(max(r, g), b), 1.0);
    [r / peak, g / peak, b / peak]
}

// blaster-bolts.js vivid keeps the strongest channel at one; use the same
// power implementation as the browser to retain float32 draw receipts.
pub fn vivid(rgb: &[f64], power: f64) -> [f64; 3] {
    std::array::from_fn(|i| pow(max(rgb.get(i).copied().unwrap_or(0.0), 0.0), power))
}

// PageOrdnance.color01 folds alpha into each channel before registration.
// Missing alpha is opaque; an absent colour is the original white default.
fn color01(rgba: Option<&[f64]>) -> [f64; 3] {
    let white = [255.0; 4];
    let rgba = rgba.unwrap_or(&white);
    let alpha = rgba.get(3).copied().unwrap_or(255.0) / 255.0;
    std::array::from_fn(|i| (rgba.get(i).copied().unwrap_or(255.0) / 255.0) * alpha)
}

// PageOrdnance.look, color01 and glowLength, formerly applied on the page.
// A heavy gun has no authored glow and keeps the old minimum capsule size.
pub fn style(def: Content) -> Numbers {
    // Authored zero width was treated as absent by the page's truthy default.
    // Keep that distinct from GlowLength's nullish fallback below.
    let width = super::or(def.laser_width.unwrap_or(0.0), 0.1);
    let heavy = super::or(def.glow_length.unwrap_or(0.0), 0.0) == 0.0;
    let glow = max(width, if heavy { 0.26 } else { 0.0 });
    let color = sheath(def.laser_glow_color.as_deref().or(def.light_color.as_deref()).unwrap_or(&[255.0; 4]));
    Numbers {
        texture_hue: heavy,
        color: vivid(&color, 1.6),
        glow,
        core: min(max(width * 0.24, if heavy { 0.05 } else { 0.0 }), 0.11),
        length: max(max(max(super::or(def.laser_length.unwrap_or(0.0), 1.0),
            super::or(def.glow_length.unwrap_or(0.0), 0.0) * 0.6), width * 6.0), if heavy { 2.5 } else { 0.0 }),
        brightness: if width >= 0.3 { 1.15 } else { 1.0 },
        // Every enhanced bolt carries a light, even when its original class
        // relied entirely on the textured core for visibility.
        light_radius: max(super::or(def.light_radius.unwrap_or(0.0), 0.0), 2.5 + glow * 5.0),
        light_color: color01(def.light_color.as_deref()),
        glow_color: color01(def.laser_glow_color.as_deref()),
        // Classic glow length falls back to three core lengths only when
        // the field is absent. An explicitly authored zero stays zero.
        glow_length: def.glow_length.unwrap_or_else(|| {
            let length = def.laser_length.unwrap_or(0.0);
            if length != 0.0 { length * 3.0 } else { 0.0 }
        }),
    }
}

// blaster-bolts.js textureHue weights saturated texels; white cores and dark
// fringes must not wash out the sheath. An empty result means a grey texture.
pub fn texture_hue(data: &[u8]) -> Vec<f64> {
    let mut sum = [0.0; 3];
    let mut weight = 0.0;
    // Ignore an incomplete pixel rather than indexing past a malformed host
    // image. Real decoded textures always supply complete RGBA rows.
    for p in data.as_chunks::<4>().0 {
        let peak = max(max(p[0] as f64, p[1] as f64), p[2] as f64);
        if peak < 48.0 || p[3] < 32 { continue; }
        let saturation = 1.0 - min(min(p[0] as f64, p[1] as f64), p[2] as f64) / peak;
        let w = saturation * saturation * peak;
        // Accumulation order is the texture's byte order and the original
        // channel order, so a later f32 style has the same colour bits.
        for i in 0..3 { sum[i] += (p[i] as f64 / peak) * w; }
        weight += w;
    }
    if weight > 1.0 { sheath(&sum).to_vec() } else { Vec::new() }
}





// BlasterBatch's per-view pixel scale uses the actual drawing target height,
// which can differ from the browser window under dynamic resolution.
pub fn bolt_view(fov: f64, zoom: f64, height: f64) -> f64 {
    (2.0*v8math::tan((fov*(std::f64::consts::PI/180.0))/2.0))/max(height,1.0)/super::or(zoom,1.0)
}
// Scripted kills can carry infinite damage; debris already saturates at -50.
pub fn visual_damage(amount: f64) -> f64 { max(-50.0,amount) }


// PageOrdnance.send: class defaults, flight classification and every style
// scalar are compiled here. Batch handles remain opaque resource identities.
pub fn row(content:&Content,look:bool)->[f64;crate::ordnance_layout::ordnance_style::STRIDE]{
    use crate::ordnance_layout::{ordnance_style as s,flight};
    let mut row=[0.0;s::STRIDE];let n=style(content.clone());
    row[s::CORE_BATCH] = -1.0;row[s::GLOW_BATCH] = -1.0;row[s::BEAM_BATCH] = -1.0;
    row[s::LASER_WIDTH]=content.laser_width.unwrap_or(0.0);row[s::LASER_LENGTH]=content.laser_length.unwrap_or(0.0);
    row[s::GLOW_LENGTH]=n.glow_length;row[s::GLOW_RAW]=content.glow_length.unwrap_or(0.0);
    row[s::BLUR_LENGTH]=content.blur_length.unwrap_or(0.0);row[s::FADE_OUT_TIME]=content.fade_out_time.unwrap_or(0.0);
    row[s::LIGHT_RADIUS]=content.light_radius.unwrap_or(0.0);row[s::GRAVITY]=content.gravity.unwrap_or(1.0);
    row[s::FLIGHT]=match content.base.as_deref(){Some("bullet"|"fatray")=>flight::STRAIGHT,Some("shell")=>flight::BALLISTIC,_=>flight::NONE} as f64;
    row[s::LIGHT_COLOR..s::LIGHT_COLOR+3].copy_from_slice(&n.light_color);row[s::GLOW_COLOR..s::GLOW_COLOR+3].copy_from_slice(&n.glow_color);
    if look {
        row[s::LOOK_COLOR..s::LOOK_COLOR+3].copy_from_slice(&n.color);row[s::LOOK_GLOW]=n.glow;
        row[s::LOOK_CORE]=n.core;row[s::LOOK_LENGTH]=n.length;row[s::LOOK_BRIGHTNESS]=n.brightness;row[s::LOOK_LIGHT_RADIUS]=n.light_radius;
    }row
}
pub fn resources(content:&Content,role:u32,enhanced:bool)->serde_json::Value{
    if enhanced {return serde_json::json!({"hue":if content.glow_length.unwrap_or(0.0)==0.0{content.laser_texture.as_deref()}else{None}});}
    if role==2 {serde_json::json!({"beam":content.laser_texture.as_deref().filter(|n|!n.is_empty()).unwrap_or("lightglow")})}
    else {serde_json::json!({"core":content.laser_texture,"glow":"lightglow"})}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_styles_keep_heavy_minima_and_alpha() {
        let s = style(serde_json::from_str(r#"{"LaserWidth":0.1,"LightColor":[128,64,32,128]}"#).unwrap());
        assert_eq!((s.glow, s.core, s.length, s.light_radius), (0.26, 0.05, 2.5, 3.8));
        assert_eq!(s.light_color[0], (128.0 / 255.0) * (128.0 / 255.0));
        assert_eq!(s.color[0], 1.0);
        let s = style(serde_json::from_str(r#"{"LaserWidth":0.4,"LaserLength":2,"GlowLength":6}"#).unwrap());
        assert_eq!((s.core, s.brightness, s.glow_length), (0.096, 1.15, 6.0));
    }
    #[test]
    fn hue_skips_white_dark_and_transparent_texels() {
        assert_eq!(texture_hue(&[255,255,255,255, 4,0,0,255, 0,255,0,0, 200,50,0,255]), vec![1.0,0.25,0.0]);
        assert!(texture_hue(&[255,255,255,255, 1]).is_empty());
    }
}
