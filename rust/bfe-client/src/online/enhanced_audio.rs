//! The enhanced mode's upscaled sound as tools/enhance_audio.py prepares it
//! (`audio/enhanced/<sub-level>/manifest.json`): a 48 kHz FLAC per bank
//! sample. They decode on a loading thread once Enhanced is first chosen;
//! until one has, its original plays (docs/audio.md, "Upscaling"). An item
//! that failed the tool's checks ("fallback") always plays its original.
use super::stream_assets::Pcm;
use std::{
    path::PathBuf,
    sync::mpsc::{sync_channel, Receiver},
};

/// One upscaled sample: the bank sample it replaces, its file, and the level
/// to bring it back to (an item stored quieter than it plays).
pub struct Item {
    pub index: usize,
    pub file: PathBuf,
    pub gain_db: f64,
}

/// A decoded upscaled sample and the render id its PCM is stored under.
pub struct Loaded {
    pub index: usize,
    pub render_id: usize,
    pub pcm: Pcm,
}

/// Decodes `items` on a thread, render ids from `base` on, in order.
pub fn load(items: Vec<Item>, base: usize) -> Receiver<Loaded> {
    let (send, receive) = sync_channel(64);
    std::thread::spawn(move || {
        let (start, total) = (std::time::Instant::now(), items.len());
        let mut decoded = 0;
        for (n, item) in items.into_iter().enumerate() {
            let Ok(bytes) = std::fs::read(&item.file) else { continue };
            match decode(&bytes, item.gain_db, 0) {
                Ok(pcm) => {
                    decoded += 1;
                    if send.send(Loaded { index: item.index, render_id: base + n, pcm }).is_err() {
                        return;
                    }
                }
                Err(err) => eprintln!("enhanced audio: {} not decoded ({err})", item.file.display()),
            }
        }
        eprintln!("enhanced audio: {decoded} of {total} upscaled samples decoded in {:.1} s", start.elapsed().as_secs_f64());
    });
    receive
}

/// Decodes a whole FLAC, Ogg Opus or WAV file to channel-major PCM, at its
/// own rate (`rate` 0) or converted to `rate`, scaled by `gain_db`.
pub fn decode(bytes: &[u8], gain_db: f64, rate: u32) -> Result<Pcm, String> {
    decode_with(bytes, gain_db, rate, false)
}

/// As `decode`, with the filter-gain compensation impulse responses need when
/// converted (the headphone decoder's filters).
pub fn decode_with(bytes: &[u8], gain_db: f64, rate: u32, compensate: bool) -> Result<Pcm, String> {
    use audio_decode::{Decoder, Step};
    let mut decoder = Decoder::new(rate, gain_db, compensate);
    decoder.push(bytes, true)?;
    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut out_rate = 0.;
    loop {
        match decoder.step(1 << 15)? {
            Step::NeedInput => return Err("truncated audio file".into()),
            Step::Metadata => {
                let mut info = decoder.info.clone().ok_or("missing audio metadata")?;
                if info.frames.is_none() {
                    // Opus: the duration comes from the last page.
                    decoder.tail(&bytes[bytes.len().saturating_sub(65536)..])?;
                    info = decoder.info.clone().ok_or("missing audio metadata")?;
                }
                if info.channels == 0 || info.channels > 16 {
                    return Err("unsupported channel count".into());
                }
                out_rate = info.rate as f64;
                planes = vec![Vec::with_capacity(info.frames.unwrap_or(0) as usize); info.channels];
            }
            Step::Output(frames) => {
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane.extend_from_slice(&decoder.output[c * frames..(c + 1) * frames]);
                }
            }
            Step::Done => break,
        }
    }
    let channels = planes.len();
    let frames = planes.first().map_or(0, Vec::len);
    if channels == 0 || frames == 0 || !(8000. ..=192000.).contains(&out_rate) {
        return Err("empty audio file".into());
    }
    let sample = audio::voices::Sample { rate: out_rate, channels, frames, data: planes.concat(), ring: None, feed: None };
    Ok(Pcm { rate: out_rate, channels, frames, sample })
}

/// The headphone decoder's filters as tools/make_binaural.py writes them
/// (`audio/hrtf/sadie2_d1_3oa_{left,right}.wav`, sixteen ambisonic channels
/// each): converted to 48 kHz with their gain kept, laid out per ambisonic
/// channel as its left-ear then its right-ear filter. None without them.
pub fn headphone_filters(left: &[u8], right: &[u8]) -> Result<(Vec<f32>, usize), String> {
    let ears = [decode_with(left, 0., 48000, true)?, decode_with(right, 0., 48000, true)?];
    if ears.iter().any(|e| e.channels != 16) || ears[0].frames != ears[1].frames {
        return Err("the headphone decoder must have sixteen channels per ear".into());
    }
    let frames = ears[0].frames;
    let mut filters = Vec::with_capacity(32 * frames);
    for c in 0..16 {
        for ear in &ears {
            filters.extend_from_slice(&ear.sample.data[c * frames..(c + 1) * frames]);
        }
    }
    Ok((filters, frames))
}
