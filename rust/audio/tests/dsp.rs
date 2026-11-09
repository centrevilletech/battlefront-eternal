//! Audio DSP processor golden parity tests ported from `tests/audio-dsp.test.js`
//! and `tests/audio-dsp-parity.mjs`.
//!
//! These tests drive the spatial mixer, room reverb, front stabilizer, master
//! limiter, clarity stage, and loudness meter through their C ABI staging
//! entrypoints across 48 kHz, 44.1 kHz, and 4 kHz synthetic signals and verify
//! that every output block SHA-256 hash, sample count, gain-reduction report,
//! and loudness reading matches `tests/fixtures/audio/dsp.json` bit-for-bit.
//!
//! Why this contract matters:
//! - Internal filter and delay states are kept in `f64` while former Web Audio
//!   node boundaries round to `f32` (`BlockMut::set` / `BlockMut::add`).
//! - Transcendental math uses `v8math` (fdlibm) so coefficient calculation is
//!   identical across x86_64 and aarch64 hosts.
//! - Subnormal flushing, quiet-tail shortcuts in the reverb network, and
//!   two-channel-at-a-time clarity filter pairing are all verified by exact
//!   SHA-256 digests over hundreds of 128-frame blocks.
//!
//! Processor coverage breakdown (`tests/fixtures/audio/dsp.json`):
//! - Spatial `Mixer` (`audio::mixer::Mixer`):
//!   - `PANPOT` (0): stereo equal-power panning with LFE send.
//!   - `ALSOFT` (2): OpenAL Soft 1.20.1 2D N3D ambisonic speaker decoder
//!     matrices for stereo (1st order, 1.5x front widening), 5.1 (2nd order),
//!     and 7.1 (3rd order), where centre and LFE rows are omitted so the
//!     downstream front stabilizer derives the centre channel.
//!   - `VBAP` (1): 2D/3D vector-base amplitude panning on 5.1 and 7.1.4
//!     (including four 45-degree height speakers).
//!   - `AMBI3` (3): 16-channel 3rd-order ambisonic encoding for SADIE binaural
//!     convolution.
//!   - Mid-stream parameter updates at blocks 13 and 27, slot 1 muting from
//!     block 20 onward, and live layout reconfiguration to stereo panpot.
//! - Room `Reverb` (`audio::reverb::Reverb`):
//!   - I3DL2 / EAX room parameters (`room`, `roomHF`, `decay`, `hfRatio`,
//!     `reflections`, `reflDelay`, `reverb`, `reverbDelay`, `diffusion`,
//!     `density`, `hfReference`) with bitmask updates at block 50.
//!   - Quiet-tail shortcut that zeroes the delay network once an impulse
//!     decays below the silence floor.
//!   - Short-delay-line sample-by-sample path at 4 kHz when delay lines are
//!     shorter than the 128-frame quantum.
//! - Front `Stabilizer` (`audio::stabilizer::Stabilizer`):
//!   - Extracts coherent front-left/right signal into the centre channel on
//!     5.1 and 7.1 layouts, handles layouts without a centre speaker or with
//!     fewer input channels than output channels, and preserves subnormal
//!     behavior on `tiny` (1e-38) signals.
//! - Master `Limiter` (`audio::limiter::Limiter`):
//!   - `home`, `normal`, and `night` compression/limiting profiles with
//!     lookahead delay (1.5 ms), LFE sidechain skip on multichannel layouts,
//!     and periodic gain-reduction telemetry reports in `io[0]`.
//! - `Clarity` stage (`audio::clarity::Clarity`):
//!   - Processes non-LFE channels in pairs (with an odd trailing channel
//!     processed solo) across 2, 3, 6, and 12 channels.
//! - Loudness `Meter` (`audio::meter::Meter`):
//!   - Momentary, short-term, integrated, sample-peak, true-peak, and
//!     per-channel peak readings over 800 blocks (~2.13 s at 48 kHz) for
//!     unweighted stereo and ITU-R BS.1770 weighted 5.1 (`[1, 1, 1, 0, 1.41, 1.41]`).

use audio::FRAMES;
use std::sync::Mutex;

// The C ABI models one audio worklet's module-local staging store. Serialize
// complete test sessions, including access through returned buffer pointers;
// locking individual calls would still let sessions race on the handle tables.
static C_ABI: Mutex<()> = Mutex::new(());

/// Default device sample rate for the recorded DSP golden suite.
const RATE: f64 = 48000.0;
/// Frames processed per quantum (`audio::FRAMES`, matching Web Audio's 128-frame block).
const BLOCK: usize = FRAMES;

/// Standard FIPS 180-4 SHA-256 implementation for hashing little-endian `f32` sample streams.
/// Built into the test module because the `audio` crate intentionally depends only on `v8math`.
struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bits: u64,
}

impl Sha256 {
    /// FIPS 180-4 round constants (first 32 bits of the fractional parts of the
    /// cube roots of the first 64 primes).
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    /// Initializes the 256-bit state vector with the standard FIPS 180-4
    /// initial hash values (first 32 bits of the fractional parts of the
    /// square roots of the first 8 primes).
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bits: 0,
        }
    }

    /// Compresses a single 512-bit message block into the running 256-bit state.
    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, &chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for (i, &w_i) in w.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w_i);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    /// Feeds arbitrary bytes into the 64-byte message block buffer,
    /// compressing each completed 512-bit block in big-endian word order
    /// and tracking total message bit length for final Merkle-Damgard padding.
    fn update(&mut self, data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);
        let mut offset = 0;
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            offset += take;
            if self.buf_len == 64 {
                Self::compress(&mut self.state, &self.buf);
                self.buf_len = 0;
            }
        }
        while offset + 64 <= data.len() {
            let block: &[u8; 64] = data[offset..offset + 64].try_into().unwrap();
            Self::compress(&mut self.state, block);
            offset += 64;
        }
        if offset < data.len() {
            let rem = data.len() - offset;
            self.buf[..rem].copy_from_slice(&data[offset..]);
            self.buf_len = rem;
        }
    }

    /// Appends the mandatory `0x80` bit, zero padding to 56 mod 64 bytes,
    /// and the 64-bit big-endian bit length, then formats the 256-bit digest
    /// as a 64-character lowercase hexadecimal string matching Node `crypto`.
    fn finish_hex(mut self) -> String {
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let pad_len = if self.buf_len < 56 {
            56 - self.buf_len
        } else {
            120 - self.buf_len
        };
        let total_bits = self.total_bits;
        self.update(&pad[..pad_len]);
        self.update(&total_bits.to_be_bytes());
        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }
}

/// Hashes nested blocks of `f32` channels and verifies both the SHA-256 digest
/// and total sample count against the frozen golden entry in `dsp.json`.
fn assert_golden(outputs: &[Vec<Vec<f32>>], expected_sha256: &str, expected_samples: usize) {
    let mut hasher = Sha256::new();
    let mut samples = 0usize;
    for block in outputs {
        for ch in block {
            for &sample in ch {
                let bits = if sample.is_nan() {
                    0x7fc0_0000_u32
                } else {
                    sample.to_bits()
                };
                hasher.update(&bits.to_le_bytes());
                samples += 1;
            }
        }
    }
    assert_eq!(samples, expected_samples);
    assert_eq!(hasher.finish_hex(), expected_sha256);
}

/// Generates deterministic synthetic signal blocks matching `signal()` in `tests/audio-dsp-parity.mjs`.
/// Uses the same 32-bit LCG (`s * 1103515245 + 12345`) and `v8math::sin` as the JS harness.
fn make_signal(kind: &str, channels: usize, blocks: usize, seed: u32) -> Vec<Vec<Vec<f32>>> {
    let mut s = seed;
    let mut rand = || -> f64 {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (s as f64 / 4_294_967_296.0) * 2.0 - 1.0
    };

    let mut out = Vec::with_capacity(blocks);
    // Build `blocks` quanta of `channels` planar buffers, advancing the LCG
    // in channel-major order within each block to match the JS loop order.
    for b in 0..blocks {
        let mut ch_blocks = Vec::with_capacity(channels);
        for c in 0..channels {
            let mut a = vec![0.0_f32; BLOCK];
            for (i, sample) in a.iter_mut().enumerate() {
                *sample = match kind {
                    "noise" => (rand() * 0.6) as f32,
                    "hot" => (rand() * 1.4) as f32,
                    "sine" => (0.8 * v8math::sin((b * BLOCK + i) as f64 * 0.11 + c as f64)) as f32,
                    "impulse" => {
                        if b == 0 && i == 0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    "tiny" => (rand() * 1e-38) as f32,
                    "square" => {
                        if !((b * BLOCK + i) >> 4).is_multiple_of(2) {
                            0.9
                        } else {
                            -0.9
                        }
                    }
                    _ => 0.0,
                };
            }
            // In JS `signal("nan")`, `a[3] = NaN` is followed by `if (!a[i]) a[i] = rand() * 0.3`;
            // because `!NaN` is true in JS, every zero or NaN slot draws from `rand()`.
            if kind == "nan" && b == 1 && c == 0 {
                a[3] = f32::NAN;
            }
            if kind == "nan" {
                for sample in &mut a {
                    if *sample == 0.0 || sample.is_nan() {
                        *sample = (rand() * 0.3) as f32;
                    }
                }
            }
            ch_blocks.push(a);
        }
        out.push(ch_blocks);
    }
    out
}

/// Speaker layout definition matching `src/audio/layouts.js`.
/// Speakers are `(azimuth_deg, elevation_deg)` with positive azimuth to the left,
/// or `None` for the LFE channel.
struct LayoutSpec {
    channels: u32,
    speakers: &'static [Option<(f64, f64)>],
}

// Standard 2-channel stereo speaker pair at +/-30 degrees azimuth, 0 elevation.
const LAYOUT_STEREO: LayoutSpec = LayoutSpec {
    channels: 2,
    speakers: &[Some((30.0, 0.0)), Some((-30.0, 0.0))],
};
// Standard 5.1 surround layout (FL, FR, FC, LFE, SL at +110 deg, SR at -110 deg).
// Channel 3 is the LFE and has no spatial speaker direction (`None`).
const LAYOUT_51: LayoutSpec = LayoutSpec {
    channels: 6,
    speakers: &[
        Some((30.0, 0.0)),
        Some((-30.0, 0.0)),
        Some((0.0, 0.0)),
        None,
        Some((110.0, 0.0)),
        Some((-110.0, 0.0)),
    ],
};
// Standard 7.1 surround layout adding rear speakers at +/-150 deg and sides at +/-90 deg.
const LAYOUT_71: LayoutSpec = LayoutSpec {
    channels: 8,
    speakers: &[
        Some((30.0, 0.0)),
        Some((-30.0, 0.0)),
        Some((0.0, 0.0)),
        None,
        Some((150.0, 0.0)),
        Some((-150.0, 0.0)),
        Some((90.0, 0.0)),
        Some((-90.0, 0.0)),
    ],
};
// 7.1.4 spatial layout adding four overhead height speakers at +/-45 and +/-135 deg
// azimuth with +45 deg elevation for 3D VBAP panning.
const LAYOUT_714: LayoutSpec = LayoutSpec {
    channels: 12,
    speakers: &[
        Some((30.0, 0.0)),
        Some((-30.0, 0.0)),
        Some((0.0, 0.0)),
        None,
        Some((150.0, 0.0)),
        Some((-150.0, 0.0)),
        Some((90.0, 0.0)),
        Some((-90.0, 0.0)),
        Some((45.0, 45.0)),
        Some((-45.0, 45.0)),
        Some((135.0, 45.0)),
        Some((-135.0, 45.0)),
    ],
};

/// OpenAL Soft 1.20.1 2D N3D ambisonic speaker decoder rows from `src/audio/layouts.js`.
const X51_FL: [f64; 5] = [1.8854286e-1, 1.27709292e-1, 1.66295695e-1, 7.30571517e-2, 2.10901184e-2];
const X51_SL: [f64; 5] = [3.33000782e-1, 1.89084803e-1, -2.00042375e-1, -2.12307769e-2, -1.14579885e-2];
const X71_BL: [f64; 7] = [2.04124145e-1, 1.08880247e-1, -1.8858612e-1, -1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0.0];
const X71_SL: [f64; 7] = [2.04124145e-1, 2.17760495e-1, 0.0, 0.0, -1.49071198e-1, -3.73460789e-2, 0.0];
const X71_FL: [f64; 7] = [2.04124145e-1, 1.08880247e-1, 1.8858612e-1, 1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0.0];

/// Mirrors a left-speaker ambisonic decoder row to the right speaker by negating Y, V, and Q.
fn mirror_row(row: &[f64]) -> Vec<f64> {
    row.iter()
        .enumerate()
        .map(|(k, &g)| if k == 1 || k == 3 || k == 5 { -g } else { g })
        .collect()
}

/// Configures the mixer's speaker table, optional OpenAL Soft decoder matrix, and panning law.
unsafe fn set_mixer_layout(
    h: u32,
    law: u32,
    speakers: &[Option<(f64, f64)>],
    decoder: Option<(f64, Vec<Option<Vec<f64>>>)>,
    dirty: u32,
) {
    let cfg_len = (speakers.len() * 3).max(1);
    let cfg = std::slice::from_raw_parts_mut(audio::mixer_cfg(h, cfg_len as u32), cfg_len);
    for (i, spk) in speakers.iter().enumerate() {
        let triple = match spk {
            Some((az, el)) => [*az, *el, 1.0],
            None => [0.0, 0.0, 0.0],
        };
        cfg[i * 3..i * 3 + 3].copy_from_slice(&triple);
    }
    audio::mixer_speakers(h, speakers.len() as u32, dirty);

    // When an OpenAL Soft ambisonic decoder is active, flatten `[row_count,
    // len_0..len_n, values...]` into `mixer_cfg` and place `front` widening in `io[0]`.
    if let Some((front, rows)) = decoder {
        let mut flat = Vec::new();
        flat.push(rows.len() as f64);
        for r in &rows {
            flat.push(r.as_ref().map_or(-1.0, |v| v.len() as f64));
        }
        for r in rows.into_iter().flatten() {
            flat.extend_from_slice(&r);
        }
        let d_cfg = std::slice::from_raw_parts_mut(audio::mixer_cfg(h, flat.len() as u32), flat.len());
        d_cfg.copy_from_slice(&flat);
        *audio::mixer_io(h) = front;
        audio::mixer_decoder(h, 1);
    } else {
        audio::mixer_decoder(h, 0);
    }
    audio::mixer_law(h, law);
}

// Ported from `tests/audio-dsp-parity.mjs`:
// Spatial mixer golden parity across panpot, alsoft, vbap, and ambi3 layouts,
// including mid-stream parameter updates and layout switches.
#[test]
fn mixer_matches_recorded_goldens() {
    let _abi = C_ABI.lock().unwrap();
    // OpenAL Soft 1.20.1 stereo decoder matrix: 1.5x front azimuth widening
    // and first-order 2D ambisonic weights (W, Y, X) for left and right speakers.
    let alsoft_stereo = Some((
        1.5,
        vec![
            Some(vec![0.5, 2.88675135e-1, 5.52305643e-2]),
            Some(vec![0.5, -2.88675135e-1, 5.52305643e-2]),
        ],
    ));
    // OpenAL Soft 1.20.1 5.1 decoder matrix: second-order 2D ambisonic rows
    // for FL, FR, SL, and SR; centre (index 2) and LFE (index 3) are None.
    let alsoft_51 = Some((
        0.0,
        vec![
            Some(X51_FL.to_vec()),
            Some(mirror_row(&X51_FL)),
            None,
            None,
            Some(X51_SL.to_vec()),
            Some(mirror_row(&X51_SL)),
        ],
    ));
    // OpenAL Soft 1.20.1 7.1 decoder matrix: third-order 2D ambisonic rows
    // for FL, FR, BL, BR, SL, and SR; centre (index 2) and LFE (index 3) are None.
    let alsoft_71 = Some((
        0.0,
        vec![
            Some(X71_FL.to_vec()),
            Some(mirror_row(&X71_FL)),
            None,
            None,
            Some(X71_BL.to_vec()),
            Some(mirror_row(&X71_BL)),
            Some(X71_SL.to_vec()),
            Some(mirror_row(&X71_SL)),
        ],
    ));

    // Each entry specifies:
    // (law, speaker_layout, optional_alsoft_decoder,
    //  initial_40_block_sha256, initial_sample_count,
    //  post_layout_switch_4_block_sha256, post_layout_switch_sample_count).
    let cases = [
        (0u32, Some(&LAYOUT_STEREO), None, "2e26cc39c7a76e51c9e03a49b33563d9da137bd6b0f5ebb69ea50128d7d6ee26", 15360usize, "59b5e67657e648f52c45789a58b7811ee3425231e07fb33c8e2c28a565fd2ec0", 1536usize),
        (2, Some(&LAYOUT_STEREO), alsoft_stereo, "f87df1fc0cf5d167c43930be3d006dba0eefdd4c9032c29f01d382d507d2956d", 15360, "27528e4acdec3981786cc70d550ef4cf7c416bee50d30f300d4cd6c1c7e69e96", 1536),
        (2, Some(&LAYOUT_51), alsoft_51, "c8f5585f6208d0751e689abeb074c3791afb3c7ef7aabbaac467e671d79fe591", 35840, "f10a9e68a5312faa7b35e74b8a8172491ab898a4bf7a1c9a720db61fe72967f2", 3584),
        (2, Some(&LAYOUT_71), alsoft_71, "b20594a1ac8a40ce4bb12f39773b561398ca4e5e08190c71ce554b7c94cabcd5", 46080, "ff6d35488d5f581167bdb4e99ba88913f128b6b424f0f7d53ea1a548a77451af", 4608),
        (1, Some(&LAYOUT_51), None, "fef3473ba5a2f5e83cdb8dc67d5809932afb0360e04eb4f57bce3f911ad1d215", 35840, "40c2fda9dd3d259e164a65bb13a847372ee0a266dd25fbd0d1b3e29ab596c9a8", 3584),
        (1, Some(&LAYOUT_714), None, "44a859fc5b66e4766b9a298f3e3a8e0e25ff3f51e5301a5035676a0e7c799f48", 66560, "f0db7e18ee57565479669e99de4de0d2ba431e27b993c51a09bfe22a47152bb7", 6656),
        (3, None, None, "9cf1e2685d2923efd8379cc6d8471fdfaaf15e0cdd4e31bda4f58098f4ae8816", 87040, "092fe34d2cee4f41000856de4a55e453515f66b4f329912d02b31db400260491", 8704),
    ];

    for (law, layout, decoder, sha_main, n_main, sha_after, n_after) in cases {
        let channels = layout.map_or(16, |l| l.channels);
        let speakers = layout.map_or(&[][..], |l| l.speakers);
        let slots = 10u32;
        let active = 7usize;
        let blocks = 40usize;

        unsafe {
            let h = audio::mixer_new(slots, channels);
            set_mixer_layout(h, law, speakers, decoder, 0);

            // Generate deterministic per-voice direction vectors (x, y, z),
            // spread (0..0.75), and LFE send (0.5 on every third voice) for the
            // 7 active voices out of 10 reserved mixer slots.
            let send_params = |b: usize| {
                let mut p = vec![0.0_f32; active * 6];
                for v in 0..active {
                    let a = v as f64 * 2.4 + b as f64 * 0.05;
                    let row = [
                        v as f32,
                        v8math::sin(a) as f32,
                        (0.3 * v8math::sin(v as f64 + b as f64 * 0.01)) as f32,
                        (-v8math::cos(a)) as f32,
                        (v % 4) as f32 * 0.25,
                        if v % 3 == 0 { 0.5 } else { 0.0 },
                    ];
                    p[v * 6..v * 6 + 6].copy_from_slice(&row);
                }
                std::slice::from_raw_parts_mut(audio::mixer_params_ptr(h), p.len())
                    .copy_from_slice(&p);
                audio::mixer_params(h, p.len() as u32, 0);
            };

            // Install initial voice positions before starting the 7 active mixer slots.
            // Slots 7, 8, and 9 remain inactive (`present = 0`) throughout the test
            // to verify that unconnected slots never leak into the speaker sum.
            send_params(0);
            for slot in 0..active {
                audio::mixer_start(h, slot as u32);
            }

            // Generate 40 blocks of 7-channel seeded noise (`seed = 7`).
            // From block 20 onward, slot 1 is fed explicit silence while remaining
            // marked present (`present[1] = 1`) to test active-voice decay to zero.
            let inputs = make_signal("noise", active, blocks, 7);
            let mut rs_all = Vec::with_capacity(blocks);
            for (b, block_in) in inputs.iter().enumerate() {
                // Update moving voice directions at blocks 13 and 27 to exercise
                // per-quantum linear gain interpolation inside the mixer.
                if b == 13 || b == 27 {
                    send_params(b);
                }
                let inp = std::slice::from_raw_parts_mut(audio::mixer_in(h), slots as usize * BLOCK);
                let present = std::slice::from_raw_parts_mut(audio::mixer_present(h), slots as usize);
                for s in 0..slots as usize {
                    present[s] = u8::from(s < active);
                    if s < active {
                        let dst = &mut inp[s * BLOCK..(s + 1) * BLOCK];
                        if b >= 20 && s == 1 {
                            dst.fill(0.0);
                        } else {
                            dst.copy_from_slice(&block_in[s]);
                        }
                    }
                }
                // Render one 128-frame quantum with LFE output enabled (`has_lfe = 1`),
                // producing `channels` speaker lanes followed by 1 LFE lane.
                audio::mixer_process(h, channels, 1);
                let outp = std::slice::from_raw_parts(audio::mixer_out(h), (channels as usize + 1) * BLOCK);
                rs_all.push((0..channels as usize + 1).map(|c| outp[c * BLOCK..(c + 1) * BLOCK].to_vec()).collect());
            }
            assert_golden(&rs_all, sha_main, n_main);

            // Switch to stereo panpot mid-stream (`dirty = 1`) and verify the next 4 blocks.
            // Even when reconfigured to stereo speaker angles, the mixer handle retains its
            // constructed channel stride and interpolates gains smoothly from the prior law.
            set_mixer_layout(h, 0, LAYOUT_STEREO.speakers, None, 1);
            let more = make_signal("sine", active, 4, 3);
            let mut r2 = Vec::with_capacity(4);
            for block_in in &more {
                let inp = std::slice::from_raw_parts_mut(audio::mixer_in(h), slots as usize * BLOCK);
                for (s, src) in block_in.iter().enumerate().take(active) {
                    inp[s * BLOCK..(s + 1) * BLOCK].copy_from_slice(src);
                }
                // Render one 128-frame quantum with LFE output enabled (`has_lfe = 1`),
                // producing `channels` speaker lanes followed by 1 LFE lane.
                audio::mixer_process(h, channels, 1);
                let outp = std::slice::from_raw_parts(audio::mixer_out(h), (channels as usize + 1) * BLOCK);
                r2.push((0..channels as usize + 1).map(|c| outp[c * BLOCK..(c + 1) * BLOCK].to_vec()).collect());
            }
            assert_golden(&r2, sha_after, n_after);
        }
    }
}

/// Runs a staged processor over `inputs` and verifies the output against the frozen SHA-256.
///
/// Mirrors `runPair` in `tests/audio-dsp-parity.mjs`: for each 128-frame block,
/// invokes `drive(b)` to apply any scheduled parameter change, copies the planar
/// input channels into the processor staging buffer, runs one block, and collects
/// the planar output channels for SHA-256 verification.
#[allow(clippy::too_many_arguments)]
unsafe fn run_staged(
    in_ptr: impl Fn() -> *mut f32,
    out_ptr: impl Fn() -> *mut f32,
    mut proc: impl FnMut(u32, u32),
    inputs: &[Vec<Vec<f32>>],
    in_channels: usize,
    out_channels: usize,
    mut drive: impl FnMut(usize),
    expected_sha256: &str,
    expected_samples: usize,
) {
    let mut all = Vec::with_capacity(inputs.len());
    for (b, block_in) in inputs.iter().enumerate() {
        drive(b);
        let inp = std::slice::from_raw_parts_mut(in_ptr(), in_channels * BLOCK);
        for (c, ch) in block_in.iter().enumerate() {
            inp[c * BLOCK..(c + 1) * BLOCK].copy_from_slice(ch);
        }
        proc(in_channels as u32, out_channels as u32);
        let outp = std::slice::from_raw_parts(out_ptr(), out_channels * BLOCK);
        all.push((0..out_channels).map(|c| outp[c * BLOCK..(c + 1) * BLOCK].to_vec()).collect());
    }
    assert_golden(&all, expected_sha256, expected_samples);
}

// Ported from `tests/audio-dsp-parity.mjs`:
// Reverb, stabilizer, clarity, and multi-rate golden parity.
#[test]
fn reverb_stabilizer_clarity_and_rates_match_recorded_goldens() {
    let _abi = C_ABI.lock().unwrap();
    unsafe {
        // Reverb scenarios at 48 kHz: enhanced room, 6ch wet hall, and dry small room.
        // Parameter indices 0..=10 map to room, roomHF, decay, hfRatio, reflections,
        // reflDelay, reverb, reverbDelay, diffusion, density, and hfReference;
        // `io[11]` carries the bitmask of modified parameters.
        let send_reverb = |h: u32, values: &[(usize, f64)], initial: u32| {
            let io = std::slice::from_raw_parts_mut(audio::reverb_io(h), 12);
            let mut mask = 0u32;
            for &(idx, val) in values {
                io[idx] = val;
                mask |= 1 << idx;
            }
            io[11] = mask as f64;
            audio::reverb_params(h, initial);
        };

        type ReverbCase<'a> = (&'a [(usize, f64)], usize, &'a str, usize, &'a str, usize);
        let reverb_cases: &[ReverbCase<'_>] = &[
            (&[(0, 0.08), (1, 0.8), (2, 0.8), (3, 0.6), (4, 0.2), (5, 0.02), (6, 0.7), (7, 0.03), (8, 1.0), (9, 1.0), (10, 5000.0)], 2, "noise", 300, "e573d890801716fb17d82cdd121c5409eeec088ed4a705b4c45f174e22f92659", 76800),
            (&[(0, 0.35), (1, 0.4), (2, 3.5), (3, 0.3), (4, 0.6), (5, 0.05), (6, 1.2), (7, 0.1), (8, 0.7), (9, 0.6), (10, 3000.0)], 6, "impulse", 600, "0f25e0d5a4c7d6254be3e417e8d03a9ce8a88a5c7b37a9f83dbc0f549700f155", 460800),
            (&[(0, 0.05), (1, 1.0), (2, 0.3), (3, 1.5), (4, 0.1), (5, 0.0), (6, 0.5), (7, 0.0), (8, 0.2), (9, 0.2), (10, 8000.0)], 2, "sine", 200, "c00136525b8ed0275b725313db44d821e0354e0ceff6391ab8499367788829a8", 51200),
        ];

        // Run each 48 kHz reverb scenario and trigger a live room/decay/density
        // parameter transition at block 50 (`initial = 0`) to test smooth updates.
        for &(params, channels, kind, blocks, sha, samples) in reverb_cases {
            let h = audio::reverb_new(RATE, channels as u32);
            send_reverb(h, params, 1);
            let inputs = make_signal(kind, 1, blocks, 7);
            run_staged(
                || audio::reverb_in(h),
                || audio::reverb_out(h),
                |i, o| audio::reverb_process(h, i, o),
                &inputs,
                1,
                channels,
                |b| {
                    if b == 50 {
                        send_reverb(h, &[(2, 1.8), (0, 0.2), (9, 0.9)], 0);
                    }
                },
                sha,
                samples,
            );
        }

        // Reverb quiet shortcut after impulse tail decays below threshold.
        // Once every comb and allpass delay line has decayed to zero after an impulse,
        // the reverb network takes its silent fast path and emits exact zeros.
        let h_quiet = audio::reverb_new(RATE, 2);
        send_reverb(h_quiet, &[(0, 0.2), (2, 0.5)], 1);
        run_staged(
            || audio::reverb_in(h_quiet),
            || audio::reverb_out(h_quiet),
            |i, o| audio::reverb_process(h_quiet, i, o),
            &make_signal("impulse", 1, 400, 7),
            1,
            2,
            |_| {},
            "2e9972379be335c58bdbc9464164d59ab0a3631adcb2753caf5b51a77f5e08db",
            102400,
        );

        // Front stabilizer scenarios (5.1, 7.1, partial inputs, missing centre, subnormals).
        // The stabilizer filters the front left/right pair and distributes correlated
        // centre energy into the centre speaker channel (`centre`), or leaves the
        // signal untouched when `centre` is out of range (e.g. quad 4ch with `centre = 5`).
        let stab_cases = [
            (6usize, 2u32, "noise", 6usize, "7c8291831e3964dadb773f188105078b590f53b24658d1aba43b12060fcef371", 92160usize),
            (8, 2, "noise", 8, "eb90fb1255445f15782558ce2033257b859f1d25fdf50252e4dd4e69b86b52ad", 122880),
            (6, 2, "noise", 4, "52d1e70806ab76cc4bfda44a2db5e15b257fbd17ee389332a40bc4c33b970b9e", 92160),
            (4, 5, "noise", 4, "72ef27ea21915a1e988cec81c9802620feb715957870c05bcc2c4bdc5850fea5", 61440),
            (6, 0, "noise", 6, "d62b19572de73b8988d961dd152006314ff033f0666ccf5a05cecae8e5307b4a", 92160),
            (6, 2, "tiny", 6, "b30d91d267151b7f324fa84708eb71e2a0ad295e9c9edf9bcdd121a98d47e15a", 92160),
            (6, 2, "impulse", 6, "74574803456db08f12d07d94cd0dbeeacd0e1b64450a6820ef137f4c88545ac8", 92160),
        ];

        // Run 120 blocks through each stabilizer configuration and verify the
        // planar output SHA-256 against the recorded reference in `dsp.json`.
        for (channels, centre, kind, in_ch, sha, samples) in stab_cases {
            let h = audio::stabilizer_new(RATE, channels as u32, 0, 1, centre);
            run_staged(
                || audio::stabilizer_in(h),
                || audio::stabilizer_out(h),
                |i, o| audio::stabilizer_process(h, i, o),
                &make_signal(kind, in_ch, 120, 7),
                in_ch,
                channels,
                |_| {},
                sha,
                samples,
            );
        }

        // Clarity stage scenarios across even/odd channel counts and LFE skip indices.
        // The clarity processor pairs active non-LFE channels two at a time through
        // its SIMD-style dual biquad/envelope state, handling an odd remaining channel
        // on its own and bypassing the LFE index (`skip = 3` on surround layouts).
        let clarity_cases = [
            (2usize, "noise", -1i32, 2usize, 200usize, "81ac2f4bd30095f56f369c44382e9ae7eee9d610b264bc23089b2bd8a4e30794", 51200usize),
            (6, "square", 3, 6, 200, "4f16c8c8dad9c944826be12d6341dbfa5960f0dd3e22f01afdf2fcefe9aac5b5", 153600),
            (2, "sine", -1, 2, 200, "2d0408e5c1e04f4f920df4bc920bce95fbffbca1a368b20d97cfbdaf9fbe7c70", 51200),
            (3, "noise", -1, 3, 200, "6e8e77c532004803dd925d78855bd7c0c77ebc900f6703619abdf1177d0122c9", 76800),
            (12, "noise", 3, 12, 200, "dec91a2f0a29e38ae581ec3bdc7b3a8fbe9633aa95752b98b143b21b58919361", 307200),
            (6, "noise", 3, 3, 200, "3cf48ea6d14413433d46021343144ffaa699beaae3747c2ab579ac6cfd98c5b7", 153600),
            (2, "tiny", -1, 2, 200, "9c62255bbf907786dc6588b6eaf3f3d2a5ded051e4ac09b5021b3fbc306dde41", 51200),
            (2, "impulse", -1, 2, 400, "ce04120174ef8a6b144f29809b64f779b42295205455e2063628e6353da077d3", 102400),
            (2, "nan", -1, 2, 200, "1db3a4862a3a5e44213f79c87d61e4d6ebd0a33b6eff2410b612600749295d92", 51200),
        ];

        // Run each clarity configuration over its block count (200 or 400 blocks)
        // and verify the planar output SHA-256 against `dsp.json`.
        for (channels, kind, skip, in_ch, blocks, sha, samples) in clarity_cases {
            let h = audio::clarity_new(RATE, channels as u32, skip);
            run_staged(
                || audio::clarity_in(h),
                || audio::clarity_out(h),
                |i, o| audio::clarity_process(h, i, o),
                &make_signal(kind, in_ch, blocks, 7),
                in_ch,
                channels,
                |_| {},
                sha,
                samples,
            );
        }

        // Limiter at 44.1 kHz and reverb at 4 kHz (delay lines shorter than 128 frames).
        // At 44.1 kHz, attack/release/lookahead sample counts rescale from 48 kHz;
        // at 4 kHz, comb and allpass delay lines are shorter than the 128-frame
        // render quantum, exercising the sample-by-sample fallback loop in `reverb.rs`.
        let h_lim = audio::limiter_new(44100.0, -1.0, 0.08, 0.0015);
        std::slice::from_raw_parts_mut(audio::limiter_io(h_lim), 8)
            .copy_from_slice(&[-18.0, 2.0, 8.0, 0.025, 0.2, -2.3, -1.0, 0b111_1111 as f64]);
        audio::limiter_dynamics(h_lim);
        run_staged(
            || audio::limiter_in(h_lim),
            || audio::limiter_out(h_lim),
            |i, o| {
                audio::limiter_process(h_lim, i, o);
            },
            &make_signal("hot", 2, 200, 7),
            2,
            2,
            |_| {},
            "965b2b5489c98a4e1ca7d691eaf86b6813d0a1f6fbbe2d00f5fd6c66721146c5",
            51200,
        );

        let h_rev = audio::reverb_new(4000.0, 2);
        send_reverb(h_rev, &[(0, 0.3), (2, 0.8)], 1);
        run_staged(
            || audio::reverb_in(h_rev),
            || audio::reverb_out(h_rev),
            |i, o| audio::reverb_process(h_rev, i, o),
            &make_signal("noise", 1, 120, 7),
            1,
            2,
            |_| {},
            "c57d0d672f9dd718a8df455ef0ac1118b81b64bda9d858a91f8c9f45c615cfb4",
            30720,
        );
    }
}

// Ported from `tests/audio-dsp-parity.mjs`:
// Limiter golden output hashes, gain-reduction reports, and loudness meter readings.
#[test]
fn limiter_and_meter_match_recorded_goldens_and_readings() {
    let _abi = C_ABI.lock().unwrap();
    unsafe {
        // Expected gain-reduction telemetry sequence (in dB) for the 6-channel
        // `night` dynamics profile driven by a 0.9-amplitude square wave over
        // 400 blocks. Each report reflects the envelope decay across reporting windows.
        let night_reports: &[f64] = &[
            -12.279005896920392,
            -2.302817065737673,
            -0.5895339863926159,
            -0.16210402905627222,
            -0.045370643596587414,
            -0.012760091453362123,
            -0.0035935055058731293,
            -0.001012389065435072,
            -0.0002852482073466172,
            -0.00008037323672175114,
        ];
        type LimiterCase<'a> = ([f64; 6], usize, &'a str, usize, &'a str, usize, &'a [f64]);
        let limiter_cases: &[LimiterCase<'_>] = &[
            ([0.0, 1.0, 0.0, 0.003, 0.25, -10.2], 2, "hot", 400, "e51f5684c877e5d7456cfa3c500bb7c67cb430777f4bf17d23cfa75b29cdffb5", 102400, &[0.0; 10]),
            ([-18.0, 2.0, 8.0, 0.025, 0.2, -2.3], 2, "noise", 400, "4289bd15511119065f322ee16dbe81b3631d0d572c5c4d786bdea98f987a18e6", 102400, &[0.0; 10]),
            ([-30.0, 5.0, 10.0, 0.004, 0.35, 10.6], 6, "square", 400, "3c5a562b5d7cdb4c41b11cbe72795e0c71695664a144bc1cedfd24339a506709", 307200, night_reports),
            ([-18.0, 2.0, 8.0, 0.025, 0.2, -2.3], 2, "tiny", 80, "831804b5a0767829d68326088af76b5bf845ad5262aaf08eabafd28dc7a3a5d4", 20480, &[0.0; 2]),
        ];

        // Each limiter case tests a dynamics preset (`home`, `normal`, `night`)
        // with `-1.0 dBTP` ceiling, `80 ms` release, and `1.5 ms` lookahead,
        // verifying both the audio output SHA-256 and the gain-reduction telemetry
        // values emitted whenever `limiter_process` returns `1`.
        for &(dyn_cfg, channels, kind, blocks, sha, samples, expected_reports) in limiter_cases {
            let h = audio::limiter_new(RATE, -1.0, 0.08, 0.0015);
            let skip = if channels > 2 { 3.0 } else { -1.0 };
            std::slice::from_raw_parts_mut(audio::limiter_io(h), 8).copy_from_slice(&[
                dyn_cfg[0],
                dyn_cfg[1],
                dyn_cfg[2],
                dyn_cfg[3],
                dyn_cfg[4],
                dyn_cfg[5],
                skip,
                0b111_1111 as f64,
            ]);
            audio::limiter_dynamics(h);

            let mut reports = Vec::new();
            let inputs = make_signal(kind, channels, blocks, 7);
            run_staged(
                || audio::limiter_in(h),
                || audio::limiter_out(h),
                |i, o| {
                    if audio::limiter_process(h, i, o) != 0 {
                        reports.push(*audio::limiter_io(h));
                    }
                },
                &inputs,
                channels,
                channels,
                |_| {},
                sha,
                samples,
            );
            assert_eq!(reports.len(), expected_reports.len());
            for (&a, &b) in reports.iter().zip(expected_reports.iter()) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }

        // Limiter NaN handling against recorded golden.
        // Verifies that the limiter envelope follower and gain computer match
        // the recorded reference over the 30-block `"nan"` signal sequence.
        let h_nan = audio::limiter_new(RATE, -1.0, 0.08, 0.0015);
        std::slice::from_raw_parts_mut(audio::limiter_io(h_nan), 8)
            .copy_from_slice(&[-18.0, 2.0, 8.0, 0.025, 0.2, -2.3, -1.0, 0b111_1111 as f64]);
        audio::limiter_dynamics(h_nan);
        run_staged(
            || audio::limiter_in(h_nan),
            || audio::limiter_out(h_nan),
            |i, o| {
                audio::limiter_process(h_nan, i, o);
            },
            &make_signal("nan", 2, 30, 7),
            2,
            2,
            |_| {},
            "ce069a49245abf54b2ae70698f4840895084a4ce2a42053fa41da1a1e1cf03f1",
            7680,
        );

        // Loudness meter 2ch and 6ch ITU-R BS.1770 weighted readings.
        // Over 800 blocks of 128 frames (102,400 frames = 2.133 s at 48 kHz),
        // the meter emits 21 periodic readings (every 4800 frames = 100 ms after
        // the initial window). Integrated loudness starts at -Infinity until the
        // gating window accumulates enough blocks, then converges on the target LUFS.
        for (channels, weights, first_reading, last_reading) in [
            (
                2usize,
                None,
                [-3.106924283218599, -3.106924283218599, f64::NEG_INFINITY, -0.8182076223256434, -0.8182076223256434],
                [-3.081252678963887_f64, -3.0502241164902344, -3.044721812512174, -0.4629640071606692, -0.16436040974297156],
            ),
            (
                6,
                Some([1.0, 1.0, 1.0, 0.0, 1.41, 1.41]),
                [1.5929260171472617, 1.5929260171472617, f64::NEG_INFINITY, -0.8182076223256434, -0.8182076223256434],
                [1.6178749821644343_f64, 1.5888998116302533, 1.5867197590617934, -0.6230218986394782, -0.16436040974297156],
            ),
        ] {
            // Create a fresh loudness meter at 48 kHz and install optional per-channel
            // BS.1770 weights (unity for front L/R/C, 0.0 for LFE, 1.41 for surrounds).
            let h = audio::meter_new(RATE);
            if let Some(w) = weights {
                std::slice::from_raw_parts_mut(audio::meter_cfg(h, w.len() as u32), w.len()).copy_from_slice(&w);
                audio::meter_weights(h, w.len() as u32);
            }
            let inputs = make_signal("noise", channels, 800, 7);
            let mut readings: Vec<[f64; 5]> = Vec::new();
            for block_in in &inputs {
                let inp = std::slice::from_raw_parts_mut(audio::meter_in(h), channels * BLOCK);
                for (c, ch) in block_in.iter().enumerate() {
                    inp[c * BLOCK..(c + 1) * BLOCK].copy_from_slice(ch);
                }
                // Whenever a 100 ms measurement window completes, `meter_process`
                // returns `1` and lays out `[momentary, short_term, integrated,
                // peak, true_peak, channel_count, ch_peaks...]` in `meter_readings(h)`.
                if audio::meter_process(h, channels as u32) != 0 {
                    let ptr = audio::meter_readings(h);
                    let total = *audio::meter_io(h) as usize;
                    let all = std::slice::from_raw_parts(ptr, total);
                    let mut at = 0;
                    while at < total {
                        let n = all[at + 5] as usize;
                        readings.push([all[at], all[at + 1], all[at + 2], all[at + 3], all[at + 4]]);
                        at += 6 + n;
                    }
                }
            }
            // Verify the exact reading count and bit-exact IEEE-754 f64 representation
            // of the initial (ungated integrated = -Infinity) and final converged readings.
            assert_eq!(readings.len(), 21);
            for (&a, &b) in readings[0].iter().zip(first_reading.iter()) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            for (&a, &b) in readings[20].iter().zip(last_reading.iter()) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
    }
}
