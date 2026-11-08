//! Xbox IMA ADPCM and signed 16-bit PCM bank normalization tests ported from
//! `tests/audio-units.test.js` and `tests/audio-light.test.js`.
//!
//! These tests verify that `audio::adpcm::Decoder` reconstructs multi-block
//! stereo tones above 25 dB SNR while preserving exact block header samples,
//! deinterleaves multi-substream chunks in order, and normalizes signed 16-bit
//! bank samples to `f32` without bias.
//!
//! Original Battlefront sound banks store compressed samples in Xbox IMA ADPCM:
//! each channel contributes a 4-byte predictor header (16-bit signed sample +
//! 8-bit step index + reserved byte) followed by eight 4-byte code groups,
//! yielding 64 frames per 36-byte channel block. Multi-substream ambient and
//! music streams interleave fixed byte chunks of those blocks across substreams.

use audio::adpcm::Decoder;
use std::f64::consts::PI;

/// Reference IMA ADPCM step table (89 entries) used to encode synthetic test tones.
/// Indexed by `0..=88`; each step controls the quantizer delta for the next nibble.
const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Reference IMA ADPCM step-index deltas indexed by the lower 3 nibble bits.
const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// Encodes planar `f64` channels into 36-byte-per-channel Xbox ADPCM blocks
/// (64 frames per block: 1 header sample + 63 4-bit codes).
///
/// The encoder mirrors the decoder's exact integer shift-and-add arithmetic
/// (`step >> 3`, `step >> 2`, `step >> 1`, `step`) and clamps both predictor
/// and step index after each sample so block headers carry evolved state.
fn encode_xbox_adpcm(channels: &[Vec<f64>]) -> Vec<u8> {
    let ch_count = channels.len();
    let frames = channels[0].len();
    let blocks = frames / 64;
    let mut out = vec![0u8; blocks * 36 * ch_count];
    let mut state: Vec<(i32, usize)> = vec![(0, 0); ch_count];

    for b in 0..blocks {
        let base = b * 36 * ch_count;
        for (c, ch) in channels.iter().enumerate() {
            let (ref mut pred, ref mut index) = state[c];
            // Block header stores the initial clamped predictor sample and step index
            // for frame 0 of this 64-frame block.
            *pred = (ch[b * 64].round() as i32).clamp(-32768, 32767);
            let h = base + 4 * c;
            let sample_bytes = (*pred as i16).to_le_bytes();
            out[h] = sample_bytes[0];
            out[h + 1] = sample_bytes[1];
            out[h + 2] = *index as u8;

            let mut codes = [0u8; 64];
            for k in 1..64 {
                let target = ch[b * 64 + k];
                let step = STEPS[*index];
                let mut diff = target - (*pred as f64);
                let mut code = 0u8;
                if diff < 0.0 {
                    code = 8;
                    diff = -diff;
                }
                let mut vp = step >> 3;
                if diff >= step as f64 {
                    code |= 4;
                    diff -= step as f64;
                    vp += step;
                }
                if diff >= (step >> 1) as f64 {
                    code |= 2;
                    diff -= (step >> 1) as f64;
                    vp += step >> 1;
                }
                if diff >= (step >> 2) as f64 {
                    code |= 1;
                    vp += step >> 2;
                }
                *pred = if (code & 8) != 0 {
                    *pred - vp
                } else {
                    *pred + vp
                }
                .clamp(-32768, 32767);
                *index = (*index as i32 + INDEX[(code & 7) as usize]).clamp(0, 88) as usize;
                codes[k - 1] = code;
            }

            // Pack 8 groups of 8 nibbles (4 bytes per channel per group, low nibble first).
            for g in 0..8 {
                for k in 0..4 {
                    let i = g * 8 + k * 2;
                    let dst = base + 4 * ch_count + (g * ch_count + c) * 4 + k;
                    out[dst] = codes[i] | (codes[i + 1] << 4);
                }
            }
        }
    }
    out
}

// Ported from `tests/audio-units.test.js`:
// "Xbox ADPCM decodes a stereo test tone back to the tone".
// Encodes 40 blocks (2560 frames) of a 440 Hz left sine and phase-shifted
// 660 Hz right sine at 48 kHz, decodes them with `audio::adpcm::Decoder`,
// and verifies both the >25 dB reconstruction SNR and exact block header
// predictor restoration at frame 64.
#[test]
fn xbox_adpcm_decodes_a_stereo_test_tone_back_to_the_tone() {
    let n = 64 * 40;
    let left: Vec<f64> = (0..n)
        .map(|i| 12000.0 * ((2.0 * PI * 440.0 * i as f64) / 48000.0).sin())
        .collect();
    let right: Vec<f64> = (0..n)
        .map(|i| 8000.0 * ((2.0 * PI * 660.0 * i as f64) / 48000.0 + 1.0).sin())
        .collect();

    let mut decoder = Decoder {
        bytes: encode_xbox_adpcm(&[left.clone(), right.clone()]),
        output: Vec::new(),
    };
    let frames = decoder.decode(2, 1, 0);
    assert_eq!(frames, n);

    let dl = &decoder.output[0];
    let dr = &decoder.output[1];
    assert_eq!(dl.len(), n);

    // Measure signal-to-noise ratio after the first 64-frame warmup block.
    let mut err = 0.0_f64;
    let mut pow = 0.0_f64;
    for i in 64..n {
        let el = dl[i] as f64 * 32768.0 - left[i];
        let er = dr[i] as f64 * 32768.0 - right[i];
        err += el * el + er * er;
        pow += left[i] * left[i] + right[i] * right[i];
    }

    // 4-bit ADPCM on a clean stereo sine pair must exceed 25 dB SNR.
    let snr = 10.0 * (pow / err).log10();
    assert!(snr > 25.0, "SNR {snr} dB did not exceed 25 dB");

    // The header sample of block 1 is frame 64 verbatim.
    assert_eq!((dl[64] as f64 * 32768.0).round(), left[64].round());
}

// Ported from `tests/audio-units.test.js`:
// "substreams interleave in fixed chunks and come back in order".
// Quad ambient beds interleave two stereo substreams in fixed byte chunks;
// deinterleaving must restore each substream's channel pair in order.
#[test]
fn substreams_interleave_in_fixed_chunks_and_come_back_in_order() {
    let n = 64 * 32;
    let make = |freq: f64| -> Vec<f64> {
        (0..n)
            .map(|i| 9000.0 * ((2.0 * PI * freq * i as f64) / 44100.0).sin())
            .collect()
    };

    let front = encode_xbox_adpcm(&[make(300.0), make(310.0)]);
    let rear = encode_xbox_adpcm(&[make(500.0), make(510.0)]);
    // 8 stereo blocks per chunk (72 bytes per stereo block).
    let chunk = 72 * 8;
    let mut mixed = Vec::with_capacity(front.len() + rear.len());
    for (f_chunk, r_chunk) in front.chunks(chunk).zip(rear.chunks(chunk)) {
        mixed.extend_from_slice(f_chunk);
        mixed.extend_from_slice(r_chunk);
    }

    let mut interleaved = Decoder {
        bytes: mixed,
        output: Vec::new(),
    };
    assert_eq!(interleaved.decode(2, 2, chunk), n);
    assert_eq!(interleaved.output.len(), 4);

    let mut reference = Decoder {
        bytes: rear,
        output: Vec::new(),
    };
    assert_eq!(reference.decode(2, 1, 0), n);

    for i in (0..n).step_by(97) {
        assert_eq!(interleaved.output[2][i], reference.output[0][i]);
    }
}

// Ported from `tests/audio-light.test.js`:
// "Rust bank preparation preserves signed PCM".
// Uncompressed 16-bit PCM bank entries divide by 32768.0 in f64 before rounding
// to f32 so negative full-scale (-32768) maps to exact -1.0 and 0 maps to 0.0.
#[test]
fn rust_bank_preparation_preserves_signed_pcm() {
    let source: [i16; 7] = [-32768, -16384, -1, 0, 1, 16384, 32767];
    let dst_ptr = audio::asset_i16(source.len() as u32);
    // Copy signed 16-bit samples into the staging buffer and normalize in place.
    unsafe {
        std::slice::from_raw_parts_mut(dst_ptr, source.len()).copy_from_slice(&source);
        let out_ptr = audio::asset_normalize();
        let actual = std::slice::from_raw_parts(out_ptr, source.len());
        let expected: Vec<f32> = source.iter().map(|&v| (v as f64 / 32768.0) as f32).collect();
        assert_eq!(actual, expected.as_slice());
    }
}
