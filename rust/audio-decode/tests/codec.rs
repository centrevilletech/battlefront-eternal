//! Audio asset decoder tests ported from `tests/audio-codec.test.js`.
//!
//! These tests verify that `audio_decode::Decoder` decodes FLAC, Ogg Opus,
//! and WAV assets into the frozen reference PCM across native and converted
//! sample rates, preserves output across arbitrary input chunk boundaries,
//! applies explicit HRIR and playback gain after rate conversion, rejects
//! truncated or corrupted containers, and reassembles Ogg packets continued
//! across page boundaries without altering end-trim durations.
//!
//! In the browser build, `decodeAudioData` implicitly resampled assets to the
//! AudioContext rate using Chromium's sinc converter and discovered Opus
//! duration from the full file buffer. The native Rust decoder (`audio-decode`)
//! replaces that host pipeline on a loading worker so render threads receive
//! only prepared planar `f32` windows and never touch a compressed container.
//!
//! Container and rate-conversion invariants verified here:
//! - FLAC 16-bit and 24-bit streams preserve exact signed integer samples
//!   divided by `2^(bits - 1)` when the requested target rate matches the
//!   stream header rate (or when `target == 0` requests native rate).
//! - Ogg Opus streams strip codec pre-skip (`OpusHead` pre-skip count at
//!   48 kHz) and trim the final packet to the last Ogg page granule position
//!   before rate conversion, matching Chromium Web Audio duration truncation.
//! - Multi-channel Ogg Opus mapping family 0 (mono/stereo), family 1 (Vorbis
//!   6ch/8ch coupled/uncoupled stream order), and family 255 (discrete
//!   application-defined channel order) all decode into channel-major planes.
//! - WAV float32 16-channel HRIR assets support optional impulse-area gain
//!   compensation (`source_rate / target_rate`) so convolved ear signals keep
//!   identical loudness across 44.1 kHz and 48 kHz output devices.
//! - Playback gain in dB (`10^(gain_db / 20)` via `v8math::pow_legacy`) is
//!   folded into the resampler window rather than a second pass over the track.
//! - Truncated containers and corrupted Ogg page CRCs fail deterministically
//!   instead of silently producing short buffers.

use audio_decode::{Decoder, Metadata, Step};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Size of the default input range transfer in bytes (`CODEC.INPUT_CHUNK`).
/// Opus tail duration discovery scans at most this trailing window for the
/// final Ogg page granule position when the initial range has not reached EOF.
const INPUT_CHUNK: usize = 65536;

/// Format tag emitted in metadata for Ogg Opus assets (`CODEC.OPUS`).
const FORMAT_OPUS: u32 = 2;

/// Options controlling how a compressed asset fixture is fed through `Decoder`.
struct DecodeOptions {
    rate: u32,
    chunk_size: usize,
    maximum: usize,
    gain_db: f64,
    compensate: bool,
}

// Default decoding options reproduce a full-rate asset load with 64 KiB
// input ranges, 2048-frame output windows, unity gain (0 dB), and no
// HRIR rate-ratio compensation.
impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            rate: 0,
            chunk_size: INPUT_CHUNK,
            maximum: 2048,
            gain_db: 0.0,
            compensate: false,
        }
    }
}

/// Decoded fixture output with per-channel planar samples and metadata.
struct DecodedAudio {
    info: Metadata,
    channels: Vec<Vec<f32>>,
}

/// Resolves a fixture path inside `tests/fixtures/audio-codecs/`.
fn fixture_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/audio-codecs")
        .join(file)
}

/// Reads raw compressed fixture bytes from `tests/fixtures/audio-codecs/`.
fn read_fixture(file: &str) -> Vec<u8> {
    fs::read(fixture_path(file)).expect("audio codec fixture must exist")
}

/// Decompresses a `.f32.gz` reference PCM fixture into little-endian `f32` samples.
/// The reference files store concatenated channel planes recorded from the frozen
/// Chromium 153 host decoder.
fn read_reference_f32(scene_name: &str) -> Vec<f32> {
    let gz_path = fixture_path(&format!("{scene_name}.f32.gz"));
    let output = Command::new("gzip")
        .arg("-dc")
        .arg(&gz_path)
        .output()
        .expect("gzip must be available to decompress reference PCM");
    assert!(
        output.status.success(),
        "gzip failed on {}",
        gz_path.display()
    );
    assert_eq!(
        output.stdout.len() % 4,
        0,
        "reference PCM byte length must be a multiple of 4"
    );
    output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes)
        .collect()
}

/// Runs `audio_decode::Decoder` with the same streaming step protocol as
/// `decodeFixture` in `tests/audio-codec-host.js`.
///
/// Feeding bytes is intentionally separated from `step(maximum)` so the loading
/// worker can yield between decoded windows and let urgent voice starts run
/// ahead of background bank preloads.
fn decode_fixture(bytes: &[u8], opts: DecodeOptions) -> Result<DecodedAudio, String> {
    let mut decoder = Decoder::new(opts.rate, opts.gain_db, opts.compensate);
    let mut offset = 0usize;
    let mut reached_eof = false;
    let mut info: Option<Metadata> = None;
    let mut planes: Vec<Vec<f32>> = Vec::new();

    loop {
        match decoder.step(opts.maximum)? {
            Step::NeedInput => {
                // Reject a decoder that keeps asking for bytes after EOF was signaled;
                // otherwise a truncated container would spin forever on the worker.
                if reached_eof {
                    return Err("decoder requested bytes after EOF".into());
                }
                let end = (offset + opts.chunk_size).min(bytes.len());
                let next = &bytes[offset..end];
                offset = end;
                let eof = offset == bytes.len();
                decoder.push(next, eof)?;
                if eof {
                    reached_eof = true;
                }
            }
            Step::Metadata => {
                let mut meta = decoder.info.clone().ok_or("missing decoder metadata")?;
                // Opus headers do not carry a total sample count; duration comes from
                // the final Ogg page's granule minus pre-skip, converted to target rate
                // with the same f64 truncation used by AudioBuffer.
                if meta.frames.is_none() && meta.format == FORMAT_OPUS {
                    let tail_start = bytes.len().saturating_sub(INPUT_CHUNK);
                    decoder.tail(&bytes[tail_start..])?;
                    meta = decoder.info.clone().ok_or("missing updated metadata")?;
                }
                planes = vec![Vec::new(); meta.channels];
                info = Some(meta);
            }
            Step::Output(frames) => {
                // Each output window is laid out channel-major with stride `frames`.
                let meta = info.as_ref().ok_or("output before metadata")?;
                for (c, plane) in planes.iter_mut().enumerate().take(meta.channels) {
                    let start = c * frames;
                    plane.extend_from_slice(&decoder.output[start..start + frames]);
                }
            }
            Step::Done => break,
        }
    }

    Ok(DecodedAudio {
        info: info.ok_or("decoder finished without metadata")?,
        channels: planes,
    })
}

/// Tuple describing one frozen host reference scene:
/// `(name, file, source_rate, channels, codec, frames, rate)`.
type SceneRow = (&'static str, &'static str, u32, usize, &'static str, u64, u32);

/// All 35 frozen host reference scenes from `tests/fixtures/audio-codecs/host.json`.
/// Covers 16-bit and 24-bit FLAC (mono/stereo, 22.05/44.1/48 kHz, standard and
/// libsndfile), 1/2/4/6/8-channel Ogg Opus (mapping families 0, 1, and 255, plus
/// SILK and hybrid modes), and 16-channel float32 HRIR WAV files.
// Scene table layout:
// 1. `flac16-all`: full 65536-sample 16-bit ramp at 44.1 kHz and resampled 48 kHz.
// 2. `flac16-stereo`: prime-length (8191 frames) stereo 16-bit at 44.1 and 48 kHz.
// 3. `flac16-22050`: half-rate 22.05 kHz mono asset at native, 44.1, and 48 kHz.
// 4. `flac24-mono` and `flac24-stereo`: 24-bit lossless assets at 48 and 44.1 kHz.
// 5. `opus-1ch`..`opus-8ch`: CELT/hybrid Opus across 1, 2, 4, 6, and 8 channels.
// 6. `opus-vorbis-6ch` and `opus-vorbis-8ch`: mapping family 1 surround coupled streams.
// 7. `hrir-float16` and `hrir-libsndfile16`: 16-channel 3OA SADIE HRIR WAV filters.
// 8. `flac24-libsndfile`, `opus-silk`, `opus-hybrid`: encoder-specific modes.
const SCENES: &[SceneRow] = &[
    ("flac16-all-44100", "flac16-all.flac", 44100, 1, "flac", 65536, 44100),
    ("flac16-all-48000", "flac16-all.flac", 44100, 1, "flac", 71331, 48000),
    ("flac16-stereo-44100", "flac16-stereo.flac", 44100, 2, "flac", 8191, 44100),
    ("flac16-stereo-48000", "flac16-stereo.flac", 44100, 2, "flac", 8915, 48000),
    ("flac16-22050-22050", "flac16-22050.flac", 22050, 1, "flac", 8191, 22050),
    ("flac16-22050-44100", "flac16-22050.flac", 22050, 1, "flac", 16382, 44100),
    ("flac16-22050-48000", "flac16-22050.flac", 22050, 1, "flac", 17830, 48000),
    ("flac24-mono-48000", "flac24-mono.flac", 48000, 1, "flac", 8191, 48000),
    ("flac24-mono-44100", "flac24-mono.flac", 48000, 1, "flac", 7525, 44100),
    ("flac24-stereo-48000", "flac24-stereo.flac", 48000, 2, "flac", 8191, 48000),
    ("flac24-stereo-44100", "flac24-stereo.flac", 48000, 2, "flac", 7525, 44100),
    ("opus-1ch-48000", "opus-1ch.opus", 48000, 1, "opus", 25441, 48000),
    ("opus-1ch-44100", "opus-1ch.opus", 48000, 1, "opus", 23373, 44100),
    ("opus-2ch-48000", "opus-2ch.opus", 48000, 2, "opus", 25441, 48000),
    ("opus-2ch-44100", "opus-2ch.opus", 48000, 2, "opus", 23373, 44100),
    ("opus-4ch-48000", "opus-4ch.opus", 48000, 4, "opus", 25441, 48000),
    ("opus-4ch-44100", "opus-4ch.opus", 48000, 4, "opus", 23373, 44100),
    ("opus-6ch-48000", "opus-6ch.opus", 48000, 6, "opus", 25441, 48000),
    ("opus-6ch-44100", "opus-6ch.opus", 48000, 6, "opus", 23373, 44100),
    ("opus-8ch-48000", "opus-8ch.opus", 48000, 8, "opus", 25441, 48000),
    ("opus-8ch-44100", "opus-8ch.opus", 48000, 8, "opus", 23373, 44100),
    ("opus-vorbis-6ch-48000", "opus-vorbis-6ch.opus", 48000, 6, "opus", 25441, 48000),
    ("opus-vorbis-6ch-44100", "opus-vorbis-6ch.opus", 48000, 6, "opus", 23373, 44100),
    ("opus-vorbis-8ch-48000", "opus-vorbis-8ch.opus", 48000, 8, "opus", 25441, 48000),
    ("opus-vorbis-8ch-44100", "opus-vorbis-8ch.opus", 48000, 8, "opus", 23373, 44100),
    ("hrir-float16-48000", "hrir-float16.wav", 48000, 16, "wav", 256, 48000),
    ("hrir-float16-44100", "hrir-float16.wav", 48000, 16, "wav", 235, 44100),
    ("flac24-libsndfile-48000", "flac24-libsndfile.flac", 48000, 2, "flac", 8191, 48000),
    ("flac24-libsndfile-44100", "flac24-libsndfile.flac", 48000, 2, "flac", 7525, 44100),
    ("opus-silk-48000", "opus-silk.opus", 48000, 1, "opus", 96339, 48000),
    ("opus-silk-44100", "opus-silk.opus", 48000, 1, "opus", 88511, 44100),
    ("opus-hybrid-48000", "opus-hybrid.opus", 48000, 1, "opus", 96113, 48000),
    ("opus-hybrid-44100", "opus-hybrid.opus", 48000, 1, "opus", 88303, 44100),
    ("hrir-libsndfile16-48000", "hrir-libsndfile16.wav", 48000, 16, "wav", 256, 48000),
    ("hrir-libsndfile16-44100", "hrir-libsndfile16.wav", 48000, 16, "wav", 235, 44100),
];

// Ported from `tests/audio-codec.test.js`:
// "Rust codec matches frozen host PCM: <scene.name>".
#[test]
fn rust_codec_matches_frozen_host_pcm() {
    let tolerance = 10.0_f32.powf(-96.0 / 20.0);
    for &(name, file, source_rate, channels, codec, frames, rate) in SCENES {
        let bytes = read_fixture(file);
        let decoded = decode_fixture(
            &bytes,
            DecodeOptions {
                rate,
                ..DecodeOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("failed to decode {name}: {e}"));

        assert_eq!(decoded.info.rate, rate, "{name}");
        assert_eq!(decoded.info.source_rate, source_rate, "{name}");
        assert_eq!(decoded.channels.len(), channels, "{name}");
        assert_eq!(decoded.info.frames, Some(frames), "{name}");

        let actual: Vec<f32> = decoded.channels.into_iter().flatten().collect();
        let expected = read_reference_f32(name);
        assert_eq!(actual.len(), expected.len(), "{name}");

        // Lossless containers decoded at their native rate must match bit-for-bit;
        // Opus and sinc rate-converted scenes must stay within -96 dBFS peak error.
        if codec != "opus" && rate == source_rate {
            for (idx, (&a, &b)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(a.to_bits(), b.to_bits(), "{name} sample {idx} differed");
            }
        } else {
            let mut max_err = 0.0_f32;
            for (&a, &b) in actual.iter().zip(expected.iter()) {
                max_err = max_err.max((a - b).abs());
            }
            assert!(
                max_err < tolerance,
                "{name} peak difference {} exceeds -96 dBFS",
                20.0 * max_err.log10()
            );
        }
    }
}

// Ported from `tests/audio-codec.test.js`:
// "codec input chunks do not change PCM: <file>".
// Verifies that arbitrary HTTP range or file read boundaries (7, 257, 4093 bytes)
// and small output transfer caps (127 frames) produce identical planar samples.
#[test]
fn codec_input_chunks_do_not_change_pcm() {
    for file in ["flac24-stereo.flac", "opus-8ch.opus", "hrir-float16.wav"] {
        let bytes = read_fixture(file);
        let reference = decode_fixture(
            &bytes,
            DecodeOptions {
                rate: 44100,
                ..DecodeOptions::default()
            },
        )
        .expect("reference decode must succeed");

        for chunk_size in [7, 257, 4093] {
            let actual = decode_fixture(
                &bytes,
                DecodeOptions {
                    rate: 44100,
                    chunk_size,
                    maximum: 127,
                    ..DecodeOptions::default()
                },
            )
            .expect("chunked decode must succeed");
            assert_eq!(
                actual.channels, reference.channels,
                "chunk size {chunk_size} altered PCM for {file}"
            );
        }
    }
}

// Ported from `tests/audio-codec.test.js`:
// "Rust preserves source rate unless conversion is explicitly requested".
// Passing `rate: 0` to `Decoder::new` leaves the asset at its native codec
// sample rate (22,050 Hz here) and avoids running the sinc resampler,
// preserving the exact source frame count (8191 frames).
#[test]
fn rust_preserves_source_rate_unless_conversion_is_explicitly_requested() {
    let bytes = read_fixture("flac16-22050.flac");
    let pcm = decode_fixture(&bytes, DecodeOptions::default()).expect("native rate decode");
    assert_eq!(pcm.info.rate, 22050);
    assert_eq!(pcm.info.frames, Some(8191));
}

// Ported from `tests/audio-codec.test.js`:
// "HRIR gain compensation uses the source and requested rates".
// Impulse responses must be scaled by `source_rate / target_rate` when resampled
// so convolution energy remains invariant across 44.1 kHz and 48 kHz devices.
#[test]
fn hrir_gain_compensation_uses_the_source_and_requested_rates() {
    let bytes = read_fixture("hrir-float16.wav");
    let plain = decode_fixture(
        &bytes,
        DecodeOptions {
            rate: 44100,
            ..DecodeOptions::default()
        },
    )
    .expect("plain HRIR decode");
    let scaled = decode_fixture(
        &bytes,
        DecodeOptions {
            rate: 44100,
            compensate: true,
            ..DecodeOptions::default()
        },
    )
    .expect("compensated HRIR decode");

    let factor = 48000.0_f64 / 44100.0_f64;
    for (plain_ch, scaled_ch) in plain.channels.iter().zip(scaled.channels.iter()) {
        let expected: Vec<f32> = plain_ch
            .iter()
            .map(|&v| (v as f64 * factor) as f32)
            .collect();
        assert_eq!(scaled_ch, &expected);
    }
}

// Ported from `tests/audio-codec.test.js`:
// "asset playback gain is applied after explicit rate conversion".
// Asset gain in dB is converted to a linear multiplier using `v8math::pow_legacy`
// (`10^(gain_db / 20)`) and applied in `f64` inside the sinc resampler before
// rounding each output sample to `f32`.
#[test]
fn asset_playback_gain_is_applied_after_explicit_rate_conversion() {
    let bytes = read_fixture("flac24-stereo.flac");
    let gain_db = 6.0;
    let plain = decode_fixture(
        &bytes,
        DecodeOptions {
            rate: 44100,
            ..DecodeOptions::default()
        },
    )
    .expect("plain FLAC decode");
    let scaled = decode_fixture(
        &bytes,
        DecodeOptions {
            rate: 44100,
            gain_db,
            ..DecodeOptions::default()
        },
    )
    .expect("gain-scaled FLAC decode");

    let linear = v8math::pow_legacy(10.0, gain_db / 20.0);
    for (plain_ch, scaled_ch) in plain.channels.iter().zip(scaled.channels.iter()) {
        let expected: Vec<f32> = plain_ch
            .iter()
            .map(|&v| (v as f64 * linear) as f32)
            .collect();
        assert_eq!(scaled_ch, &expected);
    }
}

// Ported from `tests/audio-codec.test.js`:
// "corrupt and truncated containers fail without a host decoder".
// A truncated stream or a flipped Ogg CRC bit must return a deterministic error
// rather than emitting a shorter asset that would desynchronize loop points.
#[test]
fn corrupt_and_truncated_containers_fail_without_a_host_decoder() {
    for file in ["flac24-stereo.flac", "opus-8ch.opus", "hrir-float16.wav"] {
        let bytes = read_fixture(file);
        let truncated = &bytes[..bytes.len() - 80];
        assert!(
            decode_fixture(truncated, DecodeOptions::default()).is_err(),
            "truncated {file} should fail"
        );
    }

    let mut ogg = read_fixture("opus-6ch.opus");
    let bad_index = ogg.len() - 10;
    ogg[bad_index] ^= 1;
    let err = decode_fixture(&ogg, DecodeOptions::default())
        .err()
        .expect("corrupted Ogg page must fail");
    assert!(
        err.contains("checksum") || err.contains("final page"),
        "unexpected error for corrupted Ogg: {err}"
    );
}

/// Builds an Ogg page with the standard direct CRC-32 polynomial `0x04c11db7`.
/// When `partial` is true, writes a single 255-byte lacing value so the next
/// page must continue the unfinished packet.
fn build_ogg_page(
    data: &[u8],
    flags: u8,
    granule: u64,
    serial: u32,
    sequence: u32,
    partial: bool,
) -> Vec<u8> {
    let table: Vec<u8> = if partial {
        vec![255]
    } else {
        let mut t = vec![255u8; data.len() / 255];
        t.push((data.len() % 255) as u8);
        t
    };

    let mut result = Vec::with_capacity(27 + table.len() + data.len());
    result.extend_from_slice(b"OggS");
    result.push(0);
    result.push(flags);
    result.extend_from_slice(&granule.to_le_bytes());
    result.extend_from_slice(&serial.to_le_bytes());
    result.extend_from_slice(&sequence.to_le_bytes());
    result.extend_from_slice(&0u32.to_le_bytes());
    result.push(table.len() as u8);
    result.extend_from_slice(&table);
    result.extend_from_slice(data);

    let mut crc = 0u32;
    for &byte in &result {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = (crc << 1) ^ if (crc & 0x8000_0000) != 0 { 0x04c1_1db7 } else { 0 };
        }
    }
    result[22..26].copy_from_slice(&crc.to_le_bytes());
    result
}

// Ported from `tests/audio-codec.test.js`:
// "Ogg packets continued across pages preserve PCM and end trim".
// When an Opus packet is split across two Ogg pages (the first page ending
// with a 255-byte lacing value and granule position -1, and the second page
// setting header flag 0x01 for continuation), the Ogg parser must reassemble
// the packet before calling `opus-pure` and preserve the final granule trim.
#[test]
fn ogg_packets_continued_across_pages_preserve_pcm_and_end_trim() {
    let bytes = read_fixture("opus-2ch.opus");
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut end_granule = 0u64;
    let mut serial = 0u32;
    let mut at = 0usize;

    // Extract original Ogg packets and stream metadata from the fixture.
    // Each Ogg page header is 27 bytes followed by `count` segment lengths;
    // a segment of 255 bytes continues into the next segment, while `< 255`
    // terminates the packet.
    while at < bytes.len() {
        let count = bytes[at + 26] as usize;
        let table = &bytes[at + 27..at + 27 + count];
        serial = u32::from_le_bytes(bytes[at + 14..at + 18].try_into().unwrap());
        end_granule = u64::from_le_bytes(bytes[at + 6..at + 14].try_into().unwrap());
        let mut offset = at + 27 + count;
        for &seg_len in table {
            let n = seg_len as usize;
            pending.extend_from_slice(&bytes[offset..offset + n]);
            offset += n;
            if n < 255 {
                packets.push(std::mem::take(&mut pending));
            }
        }
        at = offset;
    }

    // Re-wrap the packets so packet 2 spans two pages via a 255-byte lacing segment
    // with granule `u64::MAX` (-1) on the first page and flag `0x01` (continued) on the second.
    let mut sequence = 0u32;
    let mut pages = Vec::new();
    for (i, packet) in packets.iter().enumerate() {
        let is_end = i == packets.len() - 1;
        let granule = if is_end {
            end_granule
        } else {
            (i.saturating_sub(1) as u64) * 960
        };
        if i == 2 {
            assert!(packet.len() > 255);
            pages.extend_from_slice(&build_ogg_page(
                &packet[..255],
                0,
                u64::MAX,
                serial,
                sequence,
                true,
            ));
            sequence += 1;
            pages.extend_from_slice(&build_ogg_page(
                &packet[255..],
                1,
                granule,
                serial,
                sequence,
                false,
            ));
            sequence += 1;
        } else {
            let flags = (if i == 0 { 2 } else { 0 }) | (if is_end { 4 } else { 0 });
            let page_granule = if i < 2 { 0 } else { granule };
            pages.extend_from_slice(&build_ogg_page(
                packet,
                flags,
                page_granule,
                serial,
                sequence,
                false,
            ));
            sequence += 1;
        }
    }

    for rate in [44100, 48000] {
        let actual = decode_fixture(
            &pages,
            DecodeOptions {
                rate,
                chunk_size: 257,
                ..DecodeOptions::default()
            },
        )
        .expect("continued Ogg decode");
        let expected = decode_fixture(
            &bytes,
            DecodeOptions {
                rate,
                ..DecodeOptions::default()
            },
        )
        .expect("reference Ogg decode");
        assert_eq!(actual.channels, expected.channels);
    }
}
