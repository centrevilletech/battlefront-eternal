//! The operations of `Wide` against results computed with ordinary integers outside this crate
//! (written as hexadecimal text) and against the published FNV-1a and SplitMix64 test vectors.
//! No 64-bit integer type is named in this file.

use bfe_wide::{Fnv, Wide};

struct Row {
    a: &'static str,
    b: &'static str,
    shift: u32,
    divisor: u32,
    add: &'static str,
    sub: &'static str,
    mul: &'static str,
    xor: &'static str,
    shl: &'static str,
    shr: &'static str,
    rotl: &'static str,
    rem: u32,
}

/// The value of two groups of eight hexadecimal digits, `high_low`.
fn w(text: &str) -> Wide {
    let (high, low) = text.split_once('_').unwrap();
    Wide::new(u32::from_str_radix(high, 16).unwrap(), u32::from_str_radix(low, 16).unwrap())
}

/// The same groups of a value.
fn pair(v: Wide) -> String {
    format!("{:08x}_{:08x}", v.high(), v.low())
}

const ROWS: &[Row] = &[
    Row { a: "00000000_00000000", b: "00000000_80000000", shift: 1, divisor: 1, add: "00000000_80000000", sub: "ffffffff_80000000", mul: "00000000_00000000", xor: "00000000_80000000", shl: "00000000_00000000", shr: "00000000_00000000", rotl: "00000000_00000000", rem: 0 },
    Row { a: "00000000_00000001", b: "71e273a8_7984a8ab", shift: 6, divisor: 2, add: "71e273a8_7984a8ac", sub: "8e1d8c57_867b5756", mul: "71e273a8_7984a8ab", xor: "71e273a8_7984a8aa", shl: "00000000_00000040", shr: "00000000_00000000", rotl: "00000000_00000040", rem: 1 },
    Row { a: "ffffffff_ffffffff", b: "1aa615a9_4b6a5551", shift: 11, divisor: 3, add: "1aa615a9_4b6a5550", sub: "e559ea56_b495aaae", mul: "e559ea56_b495aaaf", xor: "e559ea56_b495aaae", shl: "ffffffff_fffff800", shr: "001fffff_ffffffff", rotl: "ffffffff_ffffffff", rem: 0 },
    Row { a: "00000000_80000000", b: "00000000_00000000", shift: 16, divisor: 7, add: "00000000_80000000", sub: "00000000_80000000", mul: "00000000_00000000", xor: "00000000_80000000", shl: "00008000_00000000", shr: "00000000_00008000", rotl: "00008000_00000000", rem: 2 },
    Row { a: "00000000_ffffffff", b: "80000000_00000000", shift: 21, divisor: 60, add: "80000000_ffffffff", sub: "80000000_ffffffff", mul: "80000000_00000000", xor: "80000000_ffffffff", shl: "001fffff_ffe00000", shr: "00000000_000007ff", rotl: "001fffff_ffe00000", rem: 15 },
    Row { a: "00000001_00000000", b: "5323451b_4a95902a", shift: 26, divisor: 600, add: "5323451c_4a95902a", sub: "acdcbae5_b56a6fd6", mul: "4a95902a_00000000", xor: "5323451a_4a95902a", shl: "04000000_00000000", shr: "00000000_00000040", rotl: "04000000_00000000", rem: 496 },
    Row { a: "7fffffff_ffffffff", b: "a7213dfe_89328293", shift: 31, divisor: 65535, add: "27213dfe_89328292", sub: "d8dec201_76cd7d6c", mul: "d8dec201_76cd7d6d", xor: "d8dec201_76cd7d6c", shl: "ffffffff_80000000", shr: "00000000_ffffffff", rotl: "ffffffff_bfffffff", rem: 32767 },
    Row { a: "80000000_00000000", b: "00000000_ffffffff", shift: 36, divisor: 65536, add: "80000000_ffffffff", sub: "7fffffff_00000001", mul: "80000000_00000000", xor: "80000000_ffffffff", shl: "00000000_00000000", shr: "00000000_08000000", rotl: "00000008_00000000", rem: 0 },
    Row { a: "cbf29ce4_84222325", b: "9fb93534_081c5253", shift: 41, divisor: 4294967295, add: "6babd218_8c3e7578", sub: "2c3967b0_7c05d0d2", mul: "6bfc61c2_c65f3eff", xor: "544ba9d0_8c3e7176", shl: "44464a00_00000000", shr: "00000000_0065f94e", rotl: "44464b97_e539c908", rem: 1343537162 },
    Row { a: "9e3779b9_7f4a7c15", b: "5856c32a_687b253b", shift: 46, divisor: 1000003, add: "f68e3ce3_e7c5a150", sub: "45e0b68f_16cf56da", mul: "d901a2ca_4030a1d7", xor: "c661ba93_1731592e", shl: "9f054000_00000000", shr: "00000000_000278dd", rotl: "9f05678d_de6e5fd2", rem: 346642 },
    Row { a: "71e273a8_7984a8ab", b: "00000000_00000001", shift: 51, divisor: 1, add: "71e273a8_7984a8ac", sub: "71e273a8_7984a8aa", mul: "71e273a8_7984a8ab", xor: "71e273a8_7984a8aa", shl: "45580000_00000000", shr: "00000000_00000e3c", rotl: "455b8f13_9d43cc25", rem: 0 },
    Row { a: "9fb93534_081c5253", b: "cbf29ce4_84222325", shift: 56, divisor: 2, add: "6babd218_8c3e7578", sub: "d3c6984f_83fa2f2e", mul: "6bfc61c2_c65f3eff", xor: "544ba9d0_8c3e7176", shl: "53000000_00000000", shr: "00000000_0000009f", rotl: "539fb935_34081c52", rem: 1 },
    Row { a: "f93fe42e_abd2d9f2", b: "3994693e_a3ae6e68", shift: 61, divisor: 3, add: "32d44d6d_4f81485a", sub: "bfab7af0_08246b8a", mul: "7f7dd57b_9fca8650", xor: "c0ab8d10_087cb79a", shl: "40000000_00000000", shr: "00000000_00000007", rotl: "5f27fc85_d57a5b3e", rem: 1 },
    Row { a: "db149e47_14be09ab", b: "919e9bc7_f4a02d92", shift: 2, divisor: 7, add: "6cb33a0f_095e373d", sub: "4976027f_201ddc19", mul: "3fe4e819_42f49286", xor: "4a8a0580_e01e2439", shl: "6c52791c_52f826ac", shr: "36c52791_c52f826a", rotl: "6c52791c_52f826af", rem: 3 },
    Row { a: "5323451b_4a95902a", b: "00000001_00000000", shift: 7, divisor: 60, add: "5323451c_4a95902a", sub: "5323451a_4a95902a", mul: "4a95902a_00000000", xor: "5323451a_4a95902a", shl: "91a28da5_4ac81500", shr: "00a6468a_36952b20", rotl: "91a28da5_4ac81529", rem: 38 },
    Row { a: "3994693e_a3ae6e68", b: "f93fe42e_abd2d9f2", shift: 12, divisor: 600, add: "32d44d6d_4f81485a", sub: "4054850f_f7db9476", mul: "7f7dd57b_9fca8650", xor: "c0ab8d10_087cb79a", shl: "4693ea3a_e6e68000", shr: "00039946_93ea3ae6", rotl: "4693ea3a_e6e68399", rem: 8 },
    Row { a: "a061ae14_a1dbf721", b: "8b3a1535_3f137b70", shift: 17, divisor: 65535, add: "2b9bc349_e0ef7291", sub: "152798df_62c87bb1", mul: "8f0716c4_f66bf970", xor: "2b5bbb21_9ec88c51", shl: "5c2943b7_ee420000", shr: "00005030_d70a50ed", rotl: "5c2943b7_ee4340c3", rem: 59251 },
    Row { a: "1aa615a9_4b6a5551", b: "ffffffff_ffffffff", shift: 22, divisor: 65536, add: "1aa615a9_4b6a5550", sub: "1aa615a9_4b6a5552", mul: "e559ea56_b495aaaf", xor: "e559ea56_b495aaae", shl: "6a52da95_54400000", shr: "0000006a_9856a52d", rotl: "6a52da95_5446a985", rem: 21841 },
    Row { a: "5856c32a_687b253b", b: "9e3779b9_7f4a7c15", shift: 27, divisor: 4294967295, add: "f68e3ce3_e7c5a150", sub: "ba1f4970_e930a926", mul: "d901a2ca_4030a1d7", xor: "c661ba93_1731592e", shl: "5343d929_d8000000", shr: "0000000b_0ad8654d", rotl: "5343d929_dac2b619", rem: 3234981989 },
    Row { a: "8b3a1535_3f137b70", b: "a061ae14_a1dbf721", shift: 32, divisor: 1000003, add: "2b9bc349_e0ef7291", sub: "ead86720_9d37844f", mul: "8f0716c4_f66bf970", xor: "2b5bbb21_9ec88c51", shl: "3f137b70_00000000", shr: "00000000_8b3a1535", rotl: "3f137b70_8b3a1535", rem: 868379 },
    Row { a: "20dd5386_c9cf0800", b: "67d1191f_da80a0bd", shift: 37, divisor: 1, add: "88ae6ca6_a44fa8bd", sub: "b90c3a66_ef4e6743", mul: "52b414f8_62d8e800", xor: "470c4a99_134fa8bd", shl: "39e10000_00000000", shr: "00000000_0106ea9c", rotl: "39e10004_1baa70d9", rem: 0 },
    Row { a: "a7213dfe_89328293", b: "7fffffff_ffffffff", shift: 42, divisor: 2, add: "27213dfe_89328292", sub: "27213dfe_89328294", mul: "d8dec201_76cd7d6d", xor: "d8dec201_76cd7d6c", shl: "ca0a4c00_00000000", shr: "00000000_0029c84f", rotl: "ca0a4e9c_84f7fa24", rem: 1 },
    Row { a: "919e9bc7_f4a02d92", b: "db149e47_14be09ab", shift: 47, divisor: 3, add: "6cb33a0f_095e373d", sub: "b689fd80_dfe223e7", mul: "3fe4e819_42f49286", xor: "4a8a0580_e01e2439", shl: "16c90000_00000000", shr: "00000000_0001233d", rotl: "16c948cf_4de3fa50", rem: 1 },
    Row { a: "67d1191f_da80a0bd", b: "20dd5386_c9cf0800", shift: 52, divisor: 7, add: "88ae6ca6_a44fa8bd", sub: "46f3c599_10b198bd", mul: "52b414f8_62d8e800", xor: "470c4a99_134fa8bd", shl: "0bd00000_00000000", shr: "00000000_0000067d", rotl: "0bd67d11_91fda80a", rem: 5 },
];

#[test]
fn every_operation_matches_the_integer_results() {
    for r in ROWS {
        let (a, b) = (w(r.a), w(r.b));
        assert_eq!(a.wrapping_add(b), w(r.add), "{} + {}", r.a, r.b);
        assert_eq!(a.wrapping_sub(b), w(r.sub), "{} - {}", r.a, r.b);
        assert_eq!(a.wrapping_mul(b), w(r.mul), "{} * {}", r.a, r.b);
        assert_eq!(a.xor(b), w(r.xor), "{} ^ {}", r.a, r.b);
        assert_eq!(a.shl(r.shift), w(r.shl), "{} << {}", r.a, r.shift);
        assert_eq!(a.shr(r.shift), w(r.shr), "{} >> {}", r.a, r.shift);
        assert_eq!(a.rotate_left(r.shift), w(r.rotl), "rotl({}, {})", r.a, r.shift);
        assert_eq!(a.rem(r.divisor), r.rem, "{} % {}", r.a, r.divisor);
        assert_eq!(pair(a), r.a);
    }
}

#[test]
fn ordering_and_bytes_follow_the_number() {
    let mut sorted = [w("ffffffff_00000000"), w("00000001_ffffffff"), w("00000001_00000000"), w("00000000_00000000"), w("00000000_ffffffff")];
    sorted.sort();
    let text: Vec<String> = sorted.iter().map(|v| pair(*v)).collect();
    assert_eq!(text, ["00000000_00000000", "00000000_ffffffff", "00000001_00000000", "00000001_ffffffff", "ffffffff_00000000"]);
    let v = w("01234567_89abcdef");
    assert_eq!(v.to_le_bytes(), [0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]);
    assert_eq!(Wide::from_le_bytes(v.to_le_bytes()), v);
    assert_eq!(Wide::from_f64_bits(1.0), w("3ff00000_00000000"));
    assert_eq!(w("3ff00000_00000000").to_f64_bits(), 1.0);
    assert_eq!(Wide::from_i32(-1), Wide::MAX);
    assert_eq!(Wide::from_i32(5), Wide::from_u32(5));
    assert_eq!(Wide::from_i32(i32::MIN), w("ffffffff_80000000"));
    assert_eq!(w("00000000_00000000").to_f64(), 0.0);
    assert_eq!(w("00000001_00000000").to_f64(), 4_294_967_296.0);
    assert_eq!(w("001fffff_ffffffff").to_f64(), 9_007_199_254_740_991.0);
    // Above 2^53 the sum rounds once, to nearest even, as the integer conversion does.
    assert_eq!(w("00200000_00000001").to_f64(), 9_007_199_254_740_992.0);
    assert_eq!(w("00200000_00000003").to_f64(), 9_007_199_254_740_996.0);
    assert_eq!(w("ffffffff_ffffffff").to_f64(), 18_446_744_073_709_551_616.0);
}

#[test]
fn bits_are_counted_and_found() {
    let v = Wide::bit(0).or(Wide::bit(31)).or(Wide::bit(32)).or(Wide::bit(63));
    assert_eq!(v, w("80000001_80000001"));
    assert_eq!(v.count_ones(), 4);
    assert_eq!(v.trailing_zeros(), 0);
    assert_eq!(Wide::bit(40).trailing_zeros(), 40);
    assert_eq!(Wide::ZERO.trailing_zeros(), 64);
    assert!(v.has(63) && v.has(32) && v.has(31) && v.has(0) && !v.has(1) && !v.has(62) && !v.has(33));
    assert_eq!(v.not().and(v), Wide::ZERO);
    assert_eq!(Wide::MAX.shl(64), Wide::ZERO);
    assert_eq!(Wide::MAX.shr(64), Wide::ZERO);
}

#[test]
fn fnv_matches_the_published_vectors_and_the_general_multiply() {
    let hash = |bytes: &[u8]| {
        let mut f = Fnv::new();
        f.bytes(bytes);
        f.finish()
    };
    assert_eq!(hash(b""), w("cbf29ce4_84222325"));
    assert_eq!(hash(b"a"), w("af63dc4c_8601ec8c"));
    assert_eq!(hash(b"foobar"), w("85944171_f73967e8"));
    // The prime applied as one multiplication by 0x100000001b3 agrees with the specialised step for every byte value.
    let prime = Wide::new(0x100, 0x1b3);
    let mut state = Fnv::new().finish();
    let mut fast = Fnv::new();
    for byte in 0..=255u8 {
        state = state.xor(Wide::from_u32(u32::from(byte))).wrapping_mul(prime);
        fast.byte(byte);
        assert_eq!(fast.finish(), state, "byte {byte}");
    }
    // A word is its eight little-endian bytes; a small number is zero-extended to the same eight.
    let mut a = Fnv::new();
    a.word(Wide::new(0, 0xdead_beef));
    let mut b = Fnv::new();
    b.small(0xdead_beef);
    assert_eq!(a.finish(), b.finish());
    let mut c = Fnv::from_hash(w("01234567_89abcdef"));
    c.bytes(&[1, 2, 3]);
    let mut d = Fnv::from_hash(w("01234567_89abcdef"));
    d.byte(1);
    d.byte(2);
    d.byte(3);
    assert_eq!(c.finish(), d.finish());
}

#[test]
fn splitmix_and_xorshift_built_from_wide_follow_the_reference_streams() {
    let mut state = Wide::ZERO;
    let mut out = Vec::new();
    for _ in 0..4 {
        state = state.wrapping_add(w("9e3779b9_7f4a7c15"));
        let mut z = state;
        z = z.xor(z.shr(30)).wrapping_mul(w("bf58476d_1ce4e5b9"));
        z = z.xor(z.shr(27)).wrapping_mul(w("94d049bb_133111eb"));
        out.push(pair(z.xor(z.shr(31))));
    }
    assert_eq!(out, ["e220a839_7b1dcdaf", "6e789e6a_a1b965f4", "06c45d18_8009454f", "f88bb8a8_724c81ec"]);
    let mut x = w("0139408d_cbbf7a44");
    let mut out = Vec::new();
    for _ in 0..3 {
        x = x.xor(x.shr(12));
        x = x.xor(x.shl(25));
        x = x.xor(x.shr(27));
        out.push(pair(x.wrapping_mul(w("2545f491_4f6cdd1d"))));
    }
    assert_eq!(out, ["e6a7a1ea_317bcf2b", "a742a7a1_b0d7cef5", "9eed8b48_662608d4"]);
}

#[test]
fn decimal_printing_matches_the_integer_display() {
    for (hex, decimal) in [
        ("00000000_00000000", "0"),
        ("00000000_00000001", "1"),
        ("00000000_00000009", "9"),
        ("00000000_0000000a", "10"),
        ("00000000_3b9ac9ff", "999999999"),
        ("00000000_3b9aca00", "1000000000"),
        ("00000000_ffffffff", "4294967295"),
        ("00000001_00000000", "4294967296"),
        ("0de0b6b3_a7640000", "1000000000000000000"),
        ("8ac72304_89e7ffff", "9999999999999999999"),
        ("ffffffff_ffffffff", "18446744073709551615"),
        ("80000000_00000000", "9223372036854775808"),
        ("00200000_00000001", concat!("90071992", "54740993")),
        ("f2a74de4_52e6b438", "17485029721327973432"),
        ("6513270e_269e0d37", "7283207964119141687"),
        ("0c5c7fd0_a6a3a450", "890727360438182992"),
        ("d23f0824_128b2f33", "15149836622520594227"),
        ("1818e811_892f902b", "1736392818365009963"),
        ("9531985d_5d9dc9f8", "10750541312280087032"),
    ] {
        let v = w(hex);
        assert_eq!(v.to_string(), decimal, "{hex}");
        let (quotient, rest) = v.div_rem(1000);
        assert!(rest < 1000);
        assert_eq!(quotient.wrapping_mul(Wide::from_u32(1000)).wrapping_add(Wide::from_u32(rest)), v, "{hex}");
    }
    assert_eq!(format!("{:>6}", w("00000000_0000002a")), "    42");
    assert_eq!(format!("{:06}", w("00000000_0000002a")), "000042");
}

#[test]
fn printing_follows_the_integer_rules() {
    assert_eq!(format!("{:x}", w("00000000_000000ff")), "ff");
    assert_eq!(format!("{:x}", Wide::ZERO), "0");
    assert_eq!(format!("{:x}", w("00000001_0000002a")), "10000002a");
    assert_eq!(format!("{:X}", w("00000000_abcdef01")), "ABCDEF01");
    assert_eq!(format!("{:08x}", w("00000000_0000002a")), "0000002a");
    assert_eq!(format!("{:08x}", w("0000000f_ffffffff")), "fffffffff");
    assert_eq!(format!("{:#x}", w("00000000_000000ff")), "0xff");
    assert_eq!(format!("{:#018x}", w("00000000_000000ff")), "0x00000000000000ff");
    assert_eq!(format!("{:>6x}", w("00000000_000000ff")), "    ff");
    assert_eq!(format!("{} {:?}", w("00000000_000000ff"), w("00000001_ffffffff")), "255 8589934591");
    assert_eq!(std::mem::size_of::<Wide>(), 8);
    assert_eq!(std::mem::align_of::<Wide>(), 8);
}
