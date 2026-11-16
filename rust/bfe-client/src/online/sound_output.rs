//! The battle's mix configuration from the player's audio settings: SOUND
//! (Enhanced or Classic), OUTPUT, DYNAMICS, MONO and CLARITY become one
//! configured `audio::output::Output` (docs/audio.md, "Settings"). The
//! numbers are setup data for the Rust output; it does all the processing.
use audio::{mixer, output::Output};

/// The player's audio settings (`audioQuality`, `audioOutput`,
/// `audioDynamics`, `audioMono`, `audioClarity`) and master volume.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub enhanced: bool,
    pub output: String,
    pub dynamics: String,
    pub mono: bool,
    pub clarity: bool,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self { enhanced: true, output: "auto".into(), dynamics: "normal".into(), mono: false, clarity: true }
    }
}

/// An output layout: device channels in their order (L R C LFE SL SR; FL FR
/// FC LFE BL BR SL SR, then TFL TFR TBL TBR), each speaker's azimuth
/// (positive to the left) and elevation in degrees, or none for the LFE.
pub struct Layout {
    pub name: &'static str,
    pub speakers: &'static [mixer::Speaker],
    pub center: i32,
    pub lfe: i32,
}
const FRONT: [mixer::Speaker; 2] = [Some((30., 0.)), Some((-30., 0.))];
const S51: [mixer::Speaker; 6] = [Some((30., 0.)), Some((-30., 0.)), Some((0., 0.)), None, Some((110., 0.)), Some((-110., 0.))];
const S71: [mixer::Speaker; 8] =
    [Some((30., 0.)), Some((-30., 0.)), Some((0., 0.)), None, Some((150., 0.)), Some((-150., 0.)), Some((90., 0.)), Some((-90., 0.))];
const S714: [mixer::Speaker; 12] = [
    Some((30., 0.)), Some((-30., 0.)), Some((0., 0.)), None, Some((150., 0.)), Some((-150., 0.)), Some((90., 0.)), Some((-90., 0.)),
    Some((45., 45.)), Some((-45., 45.)), Some((135., 45.)), Some((-135., 45.)),
];
pub const LAYOUTS: [Layout; 5] = [
    Layout { name: "headphones", speakers: &FRONT, center: -1, lfe: -1 },
    Layout { name: "stereo", speakers: &FRONT, center: -1, lfe: -1 },
    Layout { name: "5.1", speakers: &S51, center: 2, lfe: 3 },
    Layout { name: "7.1", speakers: &S71, center: 2, lfe: 3 },
    Layout { name: "7.1.4", speakers: &S714, center: 2, lfe: 3 },
];
fn layout(name: &str) -> Option<&'static Layout> {
    LAYOUTS.iter().find(|l| l.name == name)
}
/// The layout Automatic picks for a device's channel count.
pub fn auto_layout(channels: usize) -> &'static str {
    match channels {
        12.. => "7.1.4",
        8.. => "7.1",
        6.. => "5.1",
        _ => "stereo",
    }
}
/// The layout an OUTPUT choice asks for before the device has its say:
/// Automatic follows the device's own channel count.
pub fn asked(output: &str, device_default: usize) -> &'static Layout {
    let name = if output == "auto" || layout(output).is_none() { auto_layout(device_default) } else { output };
    layout(name).unwrap_or(&LAYOUTS[1])
}
/// The layout an OUTPUT choice resolves to on a device that plays the widths
/// in `widths` (bit n: n channels): the one asked for when the device plays
/// it, else the widest narrower layout it does play, else stereo.
pub fn resolve(output: &str, device_default: usize, widths: u32) -> &'static Layout {
    let want = asked(output, device_default);
    let plays = |l: &Layout| l.speakers.len() == 2 || widths & (1 << l.speakers.len()) != 0;
    if plays(want) {
        return want;
    }
    LAYOUTS[2..].iter().rev().find(|l| l.speakers.len() < want.speakers.len() && plays(l)).unwrap_or(&LAYOUTS[1])
}

// OpenAL Soft 1.20.1's built-in speaker decoders (alc/panning.cpp,
// LGPL-2.0-or-later; see THIRD-PARTY-NOTICES.md): the Classic mode's panning
// on speakers. 2D ambisonic rows over W, Y, X, V, U, Q, P; none feed the
// centre or LFE (the front stabilizer gives the centre its share). On stereo a
// positioned source ahead has its azimuth widened 1.5 times.
const X51_FL: [f64; 5] = [1.8854286e-1, 1.27709292e-1, 1.66295695e-1, 7.30571517e-2, 2.10901184e-2];
const X51_SL: [f64; 5] = [3.33000782e-1, 1.89084803e-1, -2.00042375e-1, -2.12307769e-2, -1.14579885e-2];
const X71_BL: [f64; 7] = [2.04124145e-1, 1.08880247e-1, -1.8858612e-1, -1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0.];
const X71_SL: [f64; 7] = [2.04124145e-1, 2.17760495e-1, 0., 0., -1.49071198e-1, -3.73460789e-2, 0.];
const X71_FL: [f64; 7] = [2.04124145e-1, 1.08880247e-1, 1.8858612e-1, 1.29099444e-1, 7.45355993e-2, 3.73460789e-2, 0.];
// The right speaker of a left one: Y, V and Q change sign.
fn mirror(row: &[f64]) -> Vec<f64> {
    row.iter().enumerate().map(|(k, g)| if k == 1 || k == 3 || k == 5 { -g } else { *g }).collect()
}
fn alsoft(name: &str) -> Option<mixer::Decoder> {
    let x71 = || vec![Some(X71_FL.to_vec()), Some(mirror(&X71_FL)), None, None, Some(X71_BL.to_vec()), Some(mirror(&X71_BL)), Some(X71_SL.to_vec()), Some(mirror(&X71_SL))];
    let (front, rows) = match name {
        "stereo" => (1.5, vec![Some(vec![0.5, 2.88675135e-1, 5.52305643e-2]), Some(vec![0.5, -2.88675135e-1, 5.52305643e-2])]),
        "5.1" => (0., vec![Some(X51_FL.to_vec()), Some(mirror(&X51_FL)), None, None, Some(X51_SL.to_vec()), Some(mirror(&X51_SL))]),
        "7.1" => (0., x71()),
        // 7.1.4 is beyond OpenAL Soft 1.20.1: Classic plays it as 7.1, the heights silent.
        "7.1.4" => (0., x71().into_iter().chain([None, None, None, None]).collect()),
        _ => return None,
    };
    Some(mixer::Decoder { front, rows })
}

/// Classic's level against Enhanced (dB): what OpenAL Soft's decoders take
/// from a source ahead, so switching modes compares the sound, not the level.
/// Stereo's is measured: three recorded battles on Home theater (no
/// compression) as loud as Enhanced, which holds Normal and Night too.
fn original_level(name: &str) -> f64 {
    match name {
        "stereo" => 3.2,
        "5.1" => 3.3,
        "7.1" | "7.1.4" => 0.9,
        _ => 0.,
    }
}
/// The dynamics profiles: threshold, ratio, knee, attack, release, makeup
/// (the limiter's `DYNAMICS` order; the LFE index is appended as `skip`).
fn dynamics(name: &str) -> [f64; 6] {
    match name {
        "home" => [0., 1., 0., 0.003, 0.25, -10.2],
        "night" => [-30., 5., 10., 0.004, 0.35, 10.6],
        _ => [-18., 2., 8., 0.025, 0.2, -2.3],
    }
}
/// The light mix (Output::light, Classic stereo's cheap path, chosen nowhere
/// now): the profile is a level ahead of its soft clip, set so a whole battle
/// played as loud as Enhanced on the same profile (three battles each, within
/// half a LU).
fn light_level(name: &str) -> f64 {
    match name {
        "home" => -8.8,
        "night" => -3.8,
        _ => -5.3,
    }
}
// The clarity stage's EQ (filter kind as master::kind numbers, Hz, dB, Q) and
// its output trim, which keeps the A/B with Classic at the same loudness.
const CLARITY_EQ: [(u32, f32, f32, f32); 3] = [(2, 220., -2., 0.9), (2, 3200., 1.5, 0.8), (3, 8500., 3., 0.7)];
const CLARITY_LEVEL_DB: f64 = -0.5;

// The layout last configured, for the speaker test and AUDIO DETAILS.
static CURRENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
/// The battle's output layout in force (stereo before a battle configures one).
pub fn current_layout() -> &'static Layout {
    &LAYOUTS[CURRENT.load(std::sync::atomic::Ordering::Relaxed).min(LAYOUTS.len() - 1)]
}
/// The channel names of a layout, in device order, for AUDIO DETAILS.
pub fn channel_names(layout: &Layout) -> &'static [&'static str] {
    match layout.name {
        "5.1" => &["LEFT", "RIGHT", "CENTRE", "LFE (SUBWOOFER)", "SURROUND LEFT", "SURROUND RIGHT"],
        "7.1" => &["FRONT LEFT", "FRONT RIGHT", "CENTRE", "LFE (SUBWOOFER)", "BACK LEFT", "BACK RIGHT", "SIDE LEFT", "SIDE RIGHT"],
        "7.1.4" => &[
            "FRONT LEFT", "FRONT RIGHT", "CENTRE", "LFE (SUBWOOFER)", "BACK LEFT", "BACK RIGHT", "SIDE LEFT", "SIDE RIGHT",
            "TOP FRONT LEFT", "TOP FRONT RIGHT", "TOP BACK LEFT", "TOP BACK RIGHT",
        ],
        _ => &["LEFT", "RIGHT"],
    }
}
static RENDERING: std::sync::Mutex<Option<Rendering>> = std::sync::Mutex::new(None);
/// AUDIO DETAILS for the output in force: what it is, what renders it, its channels' names.
pub fn details() -> bfe_interface::option_pages::AudioOutputInfo {
    let rendering = RENDERING.lock().ok().and_then(|r| *r);
    let Some(rendering) = rendering else { return Default::default() };
    let (output, how) = describe(&rendering);
    bfe_interface::option_pages::AudioOutputInfo {
        output,
        rendering: how,
        channels: channel_names(current_layout()).iter().map(|n| n.to_string()).collect(),
    }
}
/// AUDIO DETAILS' description of an output.
pub fn describe(rendering: &Rendering) -> (String, String) {
    let label = |layout: &str| match layout {
        "headphones" => "HEADPHONES",
        "5.1" => "5.1 SURROUND",
        "7.1" => "7.1 SURROUND",
        "7.1.4" => "7.1.4 WITH HEIGHT",
        _ => "STEREO SPEAKERS",
    };
    let fallback = if rendering.asked != rendering.layout {
        format!(" ({} NOT AVAILABLE ON THIS DEVICE)", label(rendering.asked))
    } else {
        String::new()
    };
    let label = label(rendering.layout);
    let how = if rendering.binaural {
        "DUMMY-HEAD DECODER (SADIE II KU 100)"
    } else if rendering.hrtf {
        "HRTF (IRCAM)"
    } else if rendering.light {
        "LIGHT MIX (OPENAL SOFT STEREO PANNING)"
    } else if !rendering.enhanced && rendering.channels == 2 {
        "OPENAL SOFT STEREO DECODER, THE ORIGINAL REVERB"
    } else if !rendering.enhanced {
        "OPENAL SOFT DECODER AND FRONT STABILIZER"
    } else if rendering.channels == 2 {
        "EQUAL-POWER PANNING"
    } else {
        "PAIRWISE PANNING, LFE BELOW 120 HZ"
    };
    (
        format!("{label}, {} CHANNELS, 48000 HZ{fallback}", rendering.channels),
        format!("{}, {how}", if rendering.enhanced { "ENHANCED" } else { "CLASSIC" }),
    )
}

/// What the configured output is, for the control runtime's frame flags and
/// the device: the layout's channels and the rendering it selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rendering {
    pub layout: &'static str,
    /// The layout the settings asked for (differs when the device cannot play it).
    pub asked: &'static str,
    pub channels: usize,
    pub enhanced: bool,
    pub light: bool,
    pub hrtf: bool,
    pub binaural: bool,
    pub lfe: bool,
}

/// Builds the configured output for `settings` on a device that plays the
/// widths in `widths` (its own count `device_default`). `room` is the
/// reverb in force, `volume` the master volume. `decoder` holds the prepared
/// headphone filters (sixteen ambisonic channels, left then right ear per
/// channel, at 48 kHz) when present. This allocates: call it off the device
/// thread and hand the result to `Output::adopt`.
#[allow(clippy::too_many_arguments)]
pub fn build(
    settings: &AudioSettings,
    device_default: usize,
    widths: u32,
    room: Option<(&[f64; audio::reverb::PARAM_COUNT], u32)>,
    volume: f32,
    decoder: Option<(&[f32], usize)>,
) -> (Output, Rendering) {
    let layout = resolve(&settings.output, device_default, widths);
    let channels = layout.speakers.len();
    let headphones = layout.name == "headphones";
    let enhanced = settings.enhanced;
    let binaural = enhanced && headphones && decoder.is_some();
    let decoder_rows = if !enhanced && !headphones { alsoft(layout.name) } else { None };
    let classic_speakers = decoder_rows.is_some();
    let rate = 48000.;
    // Classic on stereo plays the full original chain with its reverb, as
    // every other layout does; Output::light (the light mix) is the cheap
    // path, chosen nowhere now: the A8 laptop holds its audio blocks with
    // the full chain in a full battle at the Minimum preset.
    let mut output = Output::new(rate, channels, enhanced, headphones, binaural, settings.mono, cfg!(target_arch = "x86_64"));
    let rendering = Rendering {
        layout: layout.name,
        asked: asked(&settings.output, device_default).name,
        channels,
        enhanced,
        light: output.light_mode,
        hrtf: headphones && !binaural,
        binaural,
        lfe: enhanced && layout.lfe >= 0,
    };
    CURRENT.store(LAYOUTS.iter().position(|l| l.name == layout.name).unwrap_or(1), std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut r) = RENDERING.lock() {
        *r = Some(rendering);
    }
    output.reserve_controls(65536);
    let law = if binaural {
        mixer::AMBI3
    } else if classic_speakers {
        mixer::ALSOFT
    } else if channels == 2 {
        mixer::PANPOT
    } else {
        mixer::VBAP
    };
    let center = if headphones { -1 } else { layout.center };
    let original = 10f64.powf(original_level(layout.name) / 20.) as f32;
    output.layout(law, decoder_rows, layout.speakers, center, layout.lfe, original);
    let clarity = enhanced && settings.clarity;
    if let Some(master) = &mut output.master {
        if clarity {
            for (kind, frequency, gain, q) in CLARITY_EQ {
                master.add_eq(audio::master::kind(kind), frequency.min((rate * 0.45) as f32), gain, q);
            }
            master.set_clarity(layout.lfe as i64, 10f64.powf(CLARITY_LEVEL_DB / 20.) as f32);
        }
        if let Some((params, mask)) = room {
            master.room(params, mask, true);
        }
        let d = dynamics(&settings.dynamics);
        let values = [d[0], d[1], d[2], d[3], d[4], d[5], layout.lfe as f64];
        master.dynamics(&values, (1 << audio::limiter::DYNAMICS_COUNT) - 1);
    }
    output.light_level(light_level(&settings.dynamics));
    output.volume(volume, 0., 0., true);
    if let (true, Some((filters, frames))) = (binaural, decoder) {
        output.decoder(filters, frames);
    }
    (output, rendering)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_choices_resolve_on_the_device() {
        let w = |list: &[u32]| list.iter().fold(0u32, |m, c| m | 1 << c);
        assert_eq!(resolve("auto", 2, w(&[2])).name, "stereo");
        assert_eq!(resolve("auto", 8, w(&[2, 8])).name, "7.1");
        assert_eq!(resolve("headphones", 2, w(&[2])).name, "headphones");
        // A layout the device cannot play falls back to the widest it does.
        assert_eq!(resolve("7.1.4", 2, w(&[2, 6, 8])).name, "7.1");
        assert_eq!(resolve("5.1", 2, w(&[2])).name, "stereo");
        assert_eq!(resolve("bogus", 6, w(&[2, 6])).name, "5.1");
        // A device that plays 2 and 8 channels but not 6: 5.1 is stereo, not a 6-wide stream cut to two.
        assert_eq!(resolve("5.1", 2, w(&[2, 8])).name, "stereo");
        assert_eq!(resolve("7.1", 2, w(&[2, 8])).name, "7.1");
        let fallback = Rendering { layout: "stereo", asked: "5.1", channels: 2, enhanced: true, light: false, hrtf: false, binaural: false, lfe: false };
        assert!(describe(&fallback).0.contains("5.1 SURROUND NOT AVAILABLE"));
    }

    const ALL: u32 = 1 << 2 | 1 << 6 | 1 << 8 | 1 << 12;
    #[test]
    fn each_setting_selects_its_rendering() {
        let s = |enhanced: bool, output: &str| AudioSettings { enhanced, output: output.into(), ..Default::default() };
        let (o, r) = build(&s(false, "stereo"), 2, ALL, None, 1., None);
        // Classic stereo plays the full original chain (master: reverb, dynamics, limiter).
        assert!(!r.light && !o.light_mode && o.master.is_some() && o.channels == 2);
        assert!(describe(&r).1.contains("THE ORIGINAL REVERB"));
        let (o, r) = build(&s(true, "stereo"), 2, ALL, None, 1., None);
        assert!(!r.light && o.enhanced && o.master.is_some() && o.channels == 2);
        let (o, r) = build(&s(true, "headphones"), 2, ALL, None, 1., None);
        assert!(r.hrtf && !r.binaural && o.headphones);
        let (o, r) = build(&s(true, "5.1"), 2, ALL, None, 1., None);
        assert!(r.lfe && o.channels == 6);
        let (o, r) = build(&s(false, "7.1.4"), 2, ALL, None, 1., None);
        assert!(!r.lfe && !r.light && o.channels == 12);
    }
}
