//! Name hashes used by prepared 2005 Zero engine content.
//!
//! Animation banks key bones and clips by a 32-bit CRC (MSB first, polynomial
//! `0x04C11DB7`, init `0xFFFFFFFF`, final XOR `0xFFFFFFFF`, lowercased Latin-1
//! bytes). Sub-archive packages, properties, and localization keys use 32-bit
//! FNV-1a over each Latin-1 byte OR `0x20`.

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            if (c & 0x8000_0000) != 0 {
                c = (c << 1) ^ 0x04C1_1DB7;
            } else {
                c <<= 1;
            }
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = build_crc_table();

/// Zero engine animation bone/clip CRC-32 (lowercased Latin-1 input).
pub fn crc(name: &str) -> u32 {
    let mut r = 0xFFFF_FFFFu32;
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase() as u32;
        let byte = (lower & 0xFF) as u8;
        let idx = ((r >> 24) as u8 ^ byte) as usize;
        r = (r << 8) ^ CRC_TABLE[idx];
    }
    r ^ 0xFFFF_FFFFu32
}

/// 8-character lowercase hex formatting of [`crc`].
pub fn crc_hex(name: &str) -> String {
    format!("{:08x}", crc(name))
}

/// Zero engine 32-bit FNV-1a over each byte OR `0x20`.
///
/// Note that `b | 0x20` folds ASCII uppercase letters to lowercase while also
/// turning `_` (`0x5F`) into `0x7F`.
pub fn fnv1a(name: &str) -> u32 {
    let mut h = 2_166_136_261u32;
    for ch in name.chars() {
        let byte = ((ch as u32) & 0xFF) as u8;
        h ^= (byte | 0x20) as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// 8-character lowercase hex formatting of [`fnv1a`].
pub fn fnv1a_hex(name: &str) -> String {
    format!("{:08x}", fnv1a(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_zero_engine_bone_and_clip_hashes() {
        assert_eq!(crc_hex("human_rifle_jump"), "1e0246c2");
        assert_eq!(crc_hex("HUMAN_RIFLE_JUMP"), "1e0246c2");
        assert_eq!(crc_hex("human_rifle_diveforward"), "dc2308eb");
        assert_eq!(crc_hex("human_rifle_stand_runforward"), "0f0f4e11");
    }

    #[test]
    fn fnv1a_matches_zero_engine_package_hashes() {
        // Standard FNV-1a offset basis for empty input, plus case folding and
        // `b | 0x20` behavior on synthetic names.
        assert_eq!(fnv1a(""), 2_166_136_261u32);
        assert_eq!(fnv1a_hex(""), "811c9dc5");
        // Single lowercase ASCII byte 'a' (0x61 | 0x20 == 0x61):
        // (2166136261 ^ 0x61) * 16777619 = 0xe40c292c (published FNV-1a-32("a")).
        assert_eq!(fnv1a_hex("a"), "e40c292c");
        assert_eq!(fnv1a_hex("A"), "e40c292c");
        // Lowercase and uppercase synthetic names hash identically.
        assert_eq!(fnv1a_hex("unit_alpha_one"), fnv1a_hex("UNIT_ALPHA_ONE"));
        assert_ne!(fnv1a_hex("unit_alpha_one"), fnv1a_hex("unit_beta_two"));
        // Underscore (0x5f) folds to 0x7f under `b | 0x20`, matching Zero's hash.
        let expected_underscore = (2_166_136_261u32 ^ 0x7fu32).wrapping_mul(16_777_619);
        assert_eq!(fnv1a("_"), expected_underscore);
    }
}
