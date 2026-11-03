//! The FNV-1a hashes the engine and the game use, each spelled the
//! way its JavaScript spells it. Two folds exist and must not be
//! confused: whole UTF-16 code units (the random-stream name mix in
//! `src/ai/random.js`) and the 2004 engine's per-byte fold with
//! `| 0x20` (`src/audio/hash.js`, `src/seats.js`).

/// The FNV-1a offset basis and prime, as the code writes them.
pub const FNV_BASIS: u32 = 0x811c9dc5;
pub const FNV_PRIME: u32 = 0x01000193;

/// FNV-1a over whole UTF-16 code units (`h ^= name.charCodeAt(i)`,
/// `h = Math.imul(h, prime)`), starting from `seed ^ basis`. This is
/// `mix(seed, name)` in `src/ai/random.js:96-103`, which seeds every
/// named `RandomStreams` stream (`gameplay`, `player-weapons`, the
/// `network-recon:` keys, ...). The JavaScript holds `h` as a signed
/// 32-bit value between steps; the bits are these bits.
pub fn fnv1a_mix_units(seed: u32, units: &[u16]) -> u32 {
    let mut h: i32 = (FNV_BASIS ^ seed) as i32;
    for &unit in units {
        h ^= unit as i32;
        h = h.wrapping_mul(FNV_PRIME as i32);
    }
    h as u32
}

/// [`fnv1a_mix_units`] over a Rust string's code units.
pub fn fnv1a_mix(seed: u32, name: &str) -> u32 {
    let mut h: i32 = (FNV_BASIS ^ seed) as i32;
    for unit in name.encode_utf16() {
        h ^= unit as i32;
        h = h.wrapping_mul(FNV_PRIME as i32);
    }
    h as u32
}

/// [`fnv1a_mix_units`] with the match seed folded in as
/// `RandomStreams` does, named for what it produces.
pub fn stream_seed(seed: u32, name: &str) -> u32 {
    fnv1a_mix(seed, name)
}

/// The 2004 engine's name hash: FNV-1a over each code unit's low byte
/// OR 0x20 (which folds letters to lower case and also turns `_`
/// into 0x7f), the state kept unsigned at every step. This is
/// `fnv1a` in `src/audio/hash.js:5-12` and `fnv` in
/// `src/seats.js:488-495`; both spell the same fold.
pub fn fnv1a_engine_units(units: &[u16]) -> u32 {
    let mut h: u32 = FNV_BASIS;
    for &unit in units {
        h ^= (unit & 0xff) as u32 | 0x20;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// [`fnv1a_engine_units`] over a Rust string's code units.
pub fn fnv1a_engine(name: &str) -> u32 {
    let mut h: u32 = FNV_BASIS;
    for unit in name.encode_utf16() {
        h ^= (unit & 0xff) as u32 | 0x20;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// The engine hash over raw bytes, for names already in bytes (the
/// same fold: `byte | 0x20`).
pub fn fnv1a_engine_bytes(bytes: &[u8]) -> u32 {
    let mut h: u32 = FNV_BASIS;
    for &b in bytes {
        h ^= (b | 0x20) as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Plain FNV-1a over bytes with no fold, for digests that want the
/// textbook hash (content fingerprints and test harnesses; the game
/// itself uses the two folds above).
pub fn fnv1a_bytes(bytes: &[u8]) -> u32 {
    let mut h: u32 = FNV_BASIS;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_names_hash_to_the_basis() {
        assert_eq!(fnv1a_engine(""), FNV_BASIS);
        assert_eq!(fnv1a_bytes(b""), FNV_BASIS);
        assert_eq!(fnv1a_mix(0, ""), FNV_BASIS);
    }

    #[test]
    fn engine_fold_lower_cases_by_or() {
        assert_eq!(fnv1a_engine("ABC"), fnv1a_engine("abc"));
        // '_' (0x5f) OR 0x20 is 0x7f, so "a_b" and "a\x7fb" collide, as
        // they do in the original engine.
        assert_eq!(fnv1a_engine("a_b"), fnv1a_engine("a\u{7f}b"));
    }
}
