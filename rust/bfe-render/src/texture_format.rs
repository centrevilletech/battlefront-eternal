// KTX2Loader.getTranscoderFormat's authored format order, including the
// engine's exception for drivers which advertise BC7 but unpack it to RGBA8.
// This is a resource decision: a sampler clone cannot select another format.
// All formats are linear; the authored gamma-space values stay unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Astc, Bc7, Bc1, Bc3, Etc1, Etc2Rgb, Etc2Rgba, PvrtcRgb, PvrtcRgba, Rgba }

#[derive(Clone, Copy, Default)]
pub struct Capabilities {
    // Capability discovery happens once per context, before any transcode.
    // Decoder workers consume that choice and never probe another context.
    pub astc: bool,
    pub bc7: bool,
    pub s3tc: bool,
    pub etc2: bool,
    pub etc1: bool,
    pub pvrtc: bool,
}

impl Capabilities {
    pub fn from_bits(bits: u32) -> Self {
        Self { astc: bits & 1 != 0, bc7: bits & 2 != 0, s3tc: bits & 4 != 0, etc2: bits & 8 != 0, etc1: bits & 16 != 0, pvrtc: bits & 32 != 0 }
    }
}

// This exception is mandatory at every quality setting: choosing High must
// not turn emulated BC7 back on and multiply the texture's resident bytes.
// gpu-info.js's Mesa exception. Check ASCII word boundaries as its regexp
// does, so an unrelated renderer containing these letters cannot lose BC7.
pub fn unpacks_bc7(renderer: &str) -> bool {
    // The interface warning and uploader consume one device-policy table.
    let policy: serde_json::Value=serde_json::from_str(include_str!("gpu-policy.json")).expect("Static GPU policy JSON");
    let text = renderer.to_ascii_lowercase();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    for (at, _) in text.match_indices("mesa") {
        if at > 0 && word(text.as_bytes()[at - 1]) { continue; }
        if text.as_bytes().get(at + 4).is_some_and(|b| word(*b)) { continue; }
        let tail = &text[at + 4..];
        for part in tail.split('(').skip(1) {
            if let Some((inside, _)) = part.split_once(')') {
                let code = inside.split_once(" gt").map_or(inside, |(code, n)| if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() { code } else { inside });
                if policy["mesaCodes"].as_array().is_some_and(|names|names.iter().any(|v|v.as_str().is_some_and(|name|name.eq_ignore_ascii_case(code)))) { return true; }
            }
        }
        if let Some(tail) = tail.strip_prefix(" dri intel(r) ") {
            if policy["mesaNames"].as_array().is_some_and(|names|names.iter().any(|v|v.as_str().is_some_and(|name|tail.starts_with(&name.to_ascii_lowercase())))) { return true; }
        }
    }
    false
}

// Both ETC1S and UASTC followed UASTC's priority in KTX2Loader. Keep that
// quirk: changing the ETC1S priority would change blocks on mixed-capability
// devices even though both formats can be sampled by the same GPU.
pub fn choose(c: Capabilities, uastc: bool, alpha: bool, width: u32, height: u32, unpacked_bc7: bool) -> Format {
    // BC7 emulation is excluded before priority selection. Do not decode it
    // and then convert its RGBA output: that recreates the old memory cost.
    if uastc && c.astc { return Format::Astc; }
    if c.bc7 && !unpacked_bc7 { return Format::Bc7; }
    if c.etc2 { return if alpha { Format::Etc2Rgba } else { Format::Etc2Rgb }; }
    if c.etc1 && !alpha { return Format::Etc1; }
    if c.s3tc { return if alpha { Format::Bc3 } else { Format::Bc1 }; }
    // The original power-of-two check accepts sizes one and two, too. The
    // transcoder still allocates PVRTC's minimum 8x8 physical block footprint.
    if c.pvrtc && width.is_power_of_two() && height.is_power_of_two() {
        return if alpha { Format::PvrtcRgba } else { Format::PvrtcRgb };
    }
    Format::Rgba
}

impl Format {
    // Three's remaining CPU descriptors still expose these numeric constants.
    // Format choice and translation are Rust's; the host only stores the result.
    pub fn descriptor(self) -> u32 {
        match self { Self::Astc => 37808, Self::Bc7 => 36492, Self::Bc1 => 33777, Self::Bc3 => 33779,
            Self::Etc1 => 36196, Self::Etc2Rgb => 37492, Self::Etc2Rgba => 37496,
            Self::PvrtcRgb => 35840, Self::PvrtcRgba => 35842, Self::Rgba => 1023 }
    }
    // Basis Universal's current C++ enum. Its old BC7 modes 6 and 7 now
    // name the same transcoder output, with identical BC7 mode selection.
    pub fn transcode(self) -> u32 {
        match self {
            Self::Etc1 | Self::Etc2Rgb => 0, Self::Etc2Rgba => 1,
            Self::Bc1 => 2, Self::Bc3 => 3, Self::Bc7 => 6,
            Self::PvrtcRgb => 8, Self::PvrtcRgba => 9,
            Self::Astc => 10, Self::Rgba => 13,
        }
    }

    // Exact uploaded storage, including terminal mips smaller than a block.
    // The resource ledger counts these bytes rather than decoded RGBA size.
    pub fn bytes(self, width: u32, height: u32) -> usize {
        let w = width as usize;
        let h = height as usize;
        match self {
            Self::Rgba => w * h * 4,
            Self::PvrtcRgb | Self::PvrtcRgba => w.max(8) * h.max(8) / 2,
            Self::Bc1 | Self::Etc1 | Self::Etc2Rgb => w.div_ceil(4) * h.div_ceil(4) * 8,
            _ => w.div_ceil(4) * h.div_ceil(4) * 16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Multiple advertised families exercise priority, not just support of an
    // isolated enum. Sandy Bridge must never pay for unpacked BC7 storage.
    #[test]
    fn old_desktop_uses_bc1_and_bc3() {
        let c = Capabilities { bc7: true, s3tc: true, ..Default::default() };
        assert_eq!(choose(c, true, false, 256, 128, true), Format::Bc1);
        assert_eq!(choose(c, true, true, 256, 128, true), Format::Bc3);
        assert_eq!(choose(c, true, true, 256, 128, false), Format::Bc7);
        assert_eq!(Format::Bc1.bytes(256, 128), 16384);
        assert_eq!(Format::Bc3.bytes(256, 128), 32768);
    }

    #[test]
    fn mobile_priority_alpha_and_non_power_of_two() {
        // ETC1 has no alpha representation. A device supporting it and S3TC
        // must continue down the priority list when the source carries alpha.
        let mut c = Capabilities { astc: true, bc7: true, etc2: true, etc1: true, s3tc: true, pvrtc: true };
        assert_eq!(choose(c, true, true, 15, 9, false), Format::Astc);
        assert_eq!(choose(c, false, false, 15, 9, false), Format::Bc7);
        c.astc = false; c.bc7 = false;
        assert_eq!(choose(c, true, false, 15, 9, false), Format::Etc2Rgb);
        assert_eq!(choose(c, true, true, 15, 9, false), Format::Etc2Rgba);
        c.etc2 = false;
        assert_eq!(choose(c, true, false, 15, 9, false), Format::Etc1);
        assert_eq!(choose(c, true, true, 15, 9, false), Format::Bc3);
        c.etc1 = false; c.s3tc = false;
        assert_eq!(choose(c, true, true, 15, 9, false), Format::Rgba);
        assert_eq!(choose(c, true, true, 2, 1, false), Format::PvrtcRgba);
        assert_eq!(Format::PvrtcRgba.bytes(2, 1), 32);
        assert_eq!(Format::Bc3.bytes(1, 1), 16);
    }
    // Advertising BC7 does not imply native sampling on these old Mesa GPUs.
    // Word boundaries keep unrelated device names from selecting the exception.
    #[test]
    fn mesa_emulated_bc7_identification_matches_device_names() {
        assert!(unpacks_bc7("ANGLE (Intel, Mesa Intel(R) HD Graphics 2000 (SNB GT1), OpenGL)"));
        assert!(unpacks_bc7("Mesa DRI Intel(R) Sandybridge Mobile"));
        assert!(!unpacks_bc7("Mesa Intel(R) HD Graphics 4000 (IVB GT2)"));
        assert!(!unpacks_bc7("OtherMesa Intel (SNB GT1)"));
    }

}
