//! Shared prepared image and target descriptors. Both device hosts use the
//! same enums and sized formats; admission never generates a second policy.

use serde::{Deserialize, Serialize};

fn error(text: impl AsRef<str>) -> String {
    text.as_ref().into()
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageDesc {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) depth: i32,
    pub(crate) target: u32,
    pub(crate) format: u32,
    pub(crate) data_type: u32,
    pub(crate) internal: Option<String>,
    pub(crate) srgb: bool,
    #[serde(default)]
    pub(crate) render_target: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sampling {
    pub(crate) wrap_s: u32,
    pub(crate) wrap_t: u32,
    pub(crate) wrap_r: u32,
    pub(crate) min: u32,
    pub(crate) mag: u32,
    pub(crate) anisotropy: f32,
    pub(crate) flip: bool,
    pub(crate) premultiply: bool,
    pub(crate) alignment: i32,
    pub(crate) mipmaps: bool,
    pub(crate) compare: Option<u32>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Mip {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetDesc {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) color: u32,
    pub(crate) depth: Option<u32>,
    pub(crate) depth_buffer: bool,
    pub(crate) stencil: bool,
    pub(crate) samples: i32,
    pub(crate) resolve_depth: bool,
    pub(crate) resolve_stencil: bool,
    pub(crate) cube: bool,
}

pub(crate) fn wrap(value: u32) -> i32 {
    (match value {
        1000 => glow::REPEAT,
        1002 => glow::MIRRORED_REPEAT,
        _ => glow::CLAMP_TO_EDGE,
    }) as i32
}

pub(crate) fn filter(value: u32) -> i32 {
    (match value {
        1003 => glow::NEAREST,
        1004 => glow::NEAREST_MIPMAP_NEAREST,
        1005 => glow::NEAREST_MIPMAP_LINEAR,
        1007 => glow::LINEAR_MIPMAP_NEAREST,
        1008 => glow::LINEAR_MIPMAP_LINEAR,
        _ => glow::LINEAR,
    }) as i32
}

// Port of getInternalDepthFormat. An MSAA renderbuffer follows the target's
// stencil flag and the depth scalar type, not the depth texture's format.
pub(crate) fn depth_format(stencil: bool, data_type: Option<u32>) -> u32 {
    match (stencil, data_type) {
        (true, Some(1015)) => glow::DEPTH32F_STENCIL8,
        (true, _) => glow::DEPTH24_STENCIL8,
        (false, Some(1015)) => glow::DEPTH_COMPONENT32F,
        (false, Some(1012)) => glow::DEPTH_COMPONENT16,
        _ => glow::DEPTH_COMPONENT24,
    }
}

// These numeric descriptor enums are Three's public constants. Format and type
// translation live here, including packed depth and float attachment storage.
pub(crate) fn format(d: &ImageDesc) -> Result<(u32, u32, u32), String> {
    let ty = if d.format == 1027 && d.data_type == 1015 {
        glow::FLOAT_32_UNSIGNED_INT_24_8_REV
    } else {
        match d.data_type {
            1009 => glow::UNSIGNED_BYTE,
            1010 => glow::BYTE,
            1011 => glow::SHORT,
            1012 => glow::UNSIGNED_SHORT,
            1013 => glow::INT,
            1014 => glow::UNSIGNED_INT,
            1015 => glow::FLOAT,
            1016 => glow::HALF_FLOAT,
            1017 => glow::UNSIGNED_SHORT_4_4_4_4,
            1018 => glow::UNSIGNED_SHORT_5_5_5_1,
            1019 => glow::UNSIGNED_SHORT_5_6_5,
            1020 => glow::UNSIGNED_INT_24_8,
            _ => return Err(error("Unsupported texture scalar type")),
        }
    };
    let (base, mut sized) = match (d.format, ty) {
        (1023, glow::FLOAT) => (glow::RGBA, glow::RGBA32F),
        (1023, glow::HALF_FLOAT) => (glow::RGBA, glow::RGBA16F),
        (1023, glow::UNSIGNED_SHORT_4_4_4_4) => (glow::RGBA, glow::RGBA4),
        (1023, glow::UNSIGNED_SHORT_5_5_5_1) => (glow::RGBA, glow::RGB5_A1),
        (1023, _) => (
            glow::RGBA,
            if d.srgb {
                glow::SRGB8_ALPHA8
            } else {
                glow::RGBA8
            },
        ),
        (1022, glow::FLOAT) => (glow::RGB, glow::RGB32F),
        (1022, glow::HALF_FLOAT) => (glow::RGB, glow::RGB16F),
        (1022, glow::UNSIGNED_SHORT_5_6_5) => (glow::RGB, glow::RGB565),
        (1022, _) => (glow::RGB, if d.srgb { glow::SRGB8 } else { glow::RGB8 }),
        (1028, glow::FLOAT) => (glow::RED, glow::R32F),
        (1028, glow::HALF_FLOAT) => (glow::RED, glow::R16F),
        (1028, _) => (glow::RED, glow::R8),
        (1030, glow::FLOAT) => (glow::RG, glow::RG32F),
        (1030, glow::HALF_FLOAT) => (glow::RG, glow::RG16F),
        (1030, _) => (glow::RG, glow::RG8),
        (1026, _) => (
            glow::DEPTH_COMPONENT,
            depth_format(false, Some(d.data_type)),
        ),
        (1027, _) => (glow::DEPTH_STENCIL, depth_format(true, Some(d.data_type))),
        (compressed, _) if compressed > 30000 => (compressed, compressed),
        _ => return Err(error("Unsupported texture channel format")),
    };
    if let Some(name) = &d.internal {
        sized = match name.as_str() {
            "RGBA8" => glow::RGBA8,
            "RGBA16F" => glow::RGBA16F,
            "RGBA32F" => glow::RGBA32F,
            "R8" => glow::R8,
            "R16F" => glow::R16F,
            "R32F" => glow::R32F,
            "RG16F" => glow::RG16F,
            "RG32F" => glow::RG32F,
            "DEPTH_COMPONENT24" => glow::DEPTH_COMPONENT24,
            "DEPTH_COMPONENT32F" => glow::DEPTH_COMPONENT32F,
            _ => return Err(error(format!("Unsupported texture internal format {name}"))),
        };
    }
    Ok((base, ty, sized))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_hosts_resolve_the_same_sized_and_packed_formats() {
        let mut image: ImageDesc=serde_json::from_str(r#"{"width":8,"height":8,"depth":1,"target":3553,"format":1023,"dataType":1009,"internal":null,"srgb":true}"#).unwrap();
        assert_eq!(
            format(&image).unwrap(),
            (glow::RGBA, glow::UNSIGNED_BYTE, glow::SRGB8_ALPHA8)
        );
        assert!(!image.render_target);
        image.format = 1027;
        image.data_type = 1015;
        assert_eq!(
            format(&image).unwrap(),
            (
                glow::DEPTH_STENCIL,
                glow::FLOAT_32_UNSIGNED_INT_24_8_REV,
                glow::DEPTH32F_STENCIL8
            )
        );
        assert_eq!(depth_format(true, Some(1015)), glow::DEPTH32F_STENCIL8);
        image.data_type = 0;
        assert!(format(&image).is_err());
        image.data_type = 1009;
        image.internal = Some("invalid".into());
        assert!(format(&image).is_err());
    }
}
