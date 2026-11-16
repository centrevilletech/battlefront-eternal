//! Front-end shell audio and output render-phase tests ported from
//! `tests/audio-frontend.test.js` and `tests/audio-light.test.js`.

use audio::commands::*;
use audio::frontend::Frontend;
use audio::output::Output;
use audio::FRAMES;

// Ported from `tests/audio-frontend.test.js`:
// "front-end completion events retire PCM once and allow a slot to be reused".
//
// Each active front-end slot carries a caller-supplied generation number alongside
// its reusable slot index. When a non-looping or faded source finishes inside a
// render quantum, `process` writes `(slot, generation)` into `ended` once so the
// host can release or reuse that slot without a stale completion retiring the new
// occupant.
#[test]
fn frontend_completion_events_retire_pcm_once_and_allow_a_slot_to_be_reused() {
    let rate = 48000.0;
    let mut front = Frontend::new(rate);

    // Install an 800-frame mono sample at 8000 Hz filled with 0.2 into slots 0 and 1.
    for slot in [0, 1] {
        let ptr = front.sample(slot, 800, 1, 8000.0);
        unsafe {
            std::slice::from_raw_parts_mut(ptr, 800).fill(0.2);
        }
    }

    // Start slot 0 (generation 1) at when = 0.0, pitch = 1.5, non-looping on fx bus (0),
    // and slot 1 (generation 2) at when = 0.01, pitch = 1.0, looping on fx bus (0).
    front.commands = vec![
        FRONT_START as f64,
        0.0,
        1.0,
        0.0,
        0.0,
        1.5,
        0.0,
        1.0,
        0.0,
        FRONT_START as f64,
        1.0,
        2.0,
        0.01,
        0.0,
        1.0,
        1.0,
        1.0,
        0.0,
    ];
    front.apply();

    // Advance up to 0.2 s (9600 frames = 75 quanta of 128 frames).
    // At 8000 Hz and 1.5x pitch, 800 source frames last ~0.0667 s, so slot 0 finishes
    // well before 0.2 s while the looping voice in slot 1 continues playing.
    let suspend_frame = (0.2 * rate) as u64;
    let mut completed: Vec<(u32, u32)> = Vec::new();
    let mut frame = 0u64;
    while frame < suspend_frame {
        let count = front.process(frame);
        for i in 0..count {
            let slot = front.ended[i * FRONT_END_STRIDE + FRONT_END_SLOT];
            let gen = front.ended[i * FRONT_END_STRIDE + FRONT_END_GENERATION];
            completed.push((slot, gen));
        }
        frame += FRAMES as u64;
    }

    // Only the non-looping pitched voice in slot 0 has ended by 0.2 s.
    assert_eq!(completed, vec![(0, 1)]);

    // Reuse slot 0 with generation 3 starting at current time (0.2 s), and fade/stop
    // the looping voice in slot 1 from 0.2 s to 0.28 s.
    let now = frame as f64 / rate;
    let ptr = front.sample(0, 800, 1, 8000.0);
    unsafe {
        std::slice::from_raw_parts_mut(ptr, 800).fill(0.2);
    }
    front.commands = vec![
        FRONT_START as f64,
        0.0,
        3.0,
        now,
        0.0,
        1.0,
        0.0,
        1.0,
        0.0,
        FRONT_FADE as f64,
        1.0,
        now,
        now + 0.08,
    ];
    front.apply();

    // Render until 0.5 s (24000 frames total).
    while frame < 24000 {
        let count = front.process(frame);
        for i in 0..count {
            let slot = front.ended[i * FRONT_END_STRIDE + FRONT_END_SLOT];
            let gen = front.ended[i * FRONT_END_STRIDE + FRONT_END_GENERATION];
            completed.push((slot, gen));
        }
        frame += FRAMES as u64;
    }

    // Both the faded loop (slot 1, gen 2) and the replacement voice (slot 0, gen 3)
    // must have retired exactly once.
    assert_eq!(completed.len(), 3);
    assert!(completed.contains(&(1, 2)));
    assert!(completed.contains(&(0, 3)));
}

// Ported from `tests/audio-light.test.js`:
// "Classic stereo has one required Rust source-to-output host".
//
// In the light mix (`Output::light`, Classic stereo's cheap path), voices bypass
// the Enhanced per-voice lowpass and the Master chain, feeding the two-channel
// OpenAL Soft light panner and WaveShaper table directly.
#[test]
fn light_mix_renders_panned_voice_without_host_dsp() {
    let mut out = Output::light(48000.0, false, false);
    assert!(out.light_mode && out.master.is_none());

    // Upload a 1000-frame mono buffer filled with 0.25 into sample id 1.
    out.voices
        .sample_channels(1, 1000, 1, 48000.0)
        .fill(0.25);

    // Bind voice 0 as spatial, start sample 1 at offset 0 with gain/rate 1,
    // and place it hard left (-1, 0, 0) at level 0.5.
    out.commands = vec![
        BIND as f64,
        0.0,
        1.0,
        START as f64,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        20000.0,
        0.0,
        PLACE as f64,
        0.0,
        -1.0,
        0.0,
        0.0,
        0.5,
        0.0,
    ];
    let len = out.commands.len();
    out.apply(len);

    out.process();
    let left = &out.output[..FRAMES];
    let right = &out.output[FRAMES..2 * FRAMES];
    assert!(left.iter().any(|&v| v > 0.05));
    assert!(right.iter().all(|&v| v == 0.0));
}


// Classic on stereo speakers plays the full original chain, as the surround
// layouts do: the Master (reverb, dynamics, limiter), not the light mix.
#[test]
fn classic_stereo_builds_the_full_original_chain() {
    let out = Output::new(48000.0, 2, false, false, false, false, false);
    assert!(!out.light_mode && out.master.is_some() && !out.enhanced && out.channels == 2);
}

// Ported from `tests/audio-light.test.js`:
// "speaker test reaches its named channel after the muted mono game mix".
//
// Stream kinds `>= 3` address physical speaker lanes (`kind - 3`) after master
// volume, mono fold, and limiter processing so the audio panel's speaker test
// still identifies individual speakers even when game audio is muted or folded.
#[test]
fn speaker_test_reaches_its_named_channel_after_the_muted_mono_game_mix() {
    // Enhanced 5.1 (6 channels) with mono fold enabled and master volume muted to 0.
    let mut out = Output::new(48000.0, 6, true, false, false, true, false);
    out.volume(0.0, 0.0, 0.0, true);

    out.voices
        .sample_channels(1, 128, 1, 48000.0)
        .fill(0.25);

    // Bind stream 0 with kind = 6 (speaker test channel 6 - 3 = 3, the LFE lane).
    out.commands = vec![
        STREAM_BIND as f64,
        0.0,
        1.0,
        6.0,
        0.0,
        STREAM_START as f64,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    let len = out.commands.len();
    out.apply(len);

    out.process();
    for c in 0..6 {
        let lane = &out.output[c * FRAMES..(c + 1) * FRAMES];
        if c == 3 {
            assert!(lane.iter().all(|&v| v == 0.25));
        } else {
            assert!(lane.iter().all(|&v| v == 0.0));
        }
    }
}

// Ported from `tests/audio-light.test.js`:
// "a narrower queued segment preserves the first segment's surround lanes".
//
// When a 6-channel stream has a narrower 2-channel segment queued on part 1,
// overlapping or subsequent rendering must not zero out the higher surround
// lanes contributed by part 0.
#[test]
fn a_narrower_queued_segment_preserves_the_first_segments_surround_lanes() {
    let mut out = Output::new(48000.0, 6, true, false, false, false, false);

    // First sample: 6 channels, 48000 frames, with channel 3 (LFE) set to 0.25.
    {
        let first = out.voices.sample_channels(1, 48000, 6, 48000.0);
        first[3 * 48000..4 * 48000].fill(0.25);
    }
    // Second sample: narrower 2-channel segment queued at 0.03 s on part 1.
    {
        let next = out.voices.sample_channels(2, 48000, 2, 48000.0);
        next[..48000].fill(0.1);
    }

    out.commands = vec![
        STREAM_BIND as f64,
        0.0,
        6.0,
        2.0,
        0.0,
        STREAM_START as f64,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        1.0,
        STREAM_START as f64,
        0.0,
        1.0,
        2.0,
        0.03,
        0.0,
        1.0,
        1.0,
    ];
    let len = out.commands.len();
    out.apply(len);

    // Render up to frame 2048 and verify channel 3 retains signal after frame 512.
    for block in 0..16 {
        out.process();
        if block >= 4 {
            let ch3 = &out.output[3 * FRAMES..4 * FRAMES];
            assert!(
                ch3.iter().all(|&v| v > 0.05),
                "channel 3 lost signal at block {block}"
            );
        }
    }
}

// A 5.1 or 7.1 bed still playing when the output narrows, and a 7.1 bed on
// 7.1.4: every lane lands on a speaker of the new layout, by its role.
fn bed_levels(output_channels: usize, bed_channels: usize, lane: usize) -> Vec<f32> {
    let mut out = Output::new(48000.0, output_channels, true, false, false, false, false);
    {
        let pcm = out.voices.sample_channels(1, 48000, bed_channels, 48000.0);
        pcm[lane * 48000..(lane + 1) * 48000].fill(0.1);
    }
    out.commands = vec![
        STREAM_BIND as f64, 0.0, bed_channels as f64, 2.0, 0.0,
        STREAM_GAIN as f64, 0.0, 1.0, 0.0, 0.0, 1.0,
        STREAM_START as f64, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0,
    ];
    let len = out.commands.len();
    out.apply(len);
    let mut energy = vec![0f32; output_channels];
    for block in 0..40 {
        out.process();
        if block >= 20 {
            for (c, e) in energy.iter_mut().enumerate() {
                *e += out.output[c * FRAMES..(c + 1) * FRAMES].iter().map(|v| v * v).sum::<f32>();
            }
        }
    }
    energy
}
#[test]
fn a_surround_bed_on_a_narrower_output_keeps_every_lane_by_its_role() {
    // 5.1 on stereo: the centre in both, equally; the LFE nowhere; a surround on its side.
    let centre = bed_levels(2, 6, 2);
    assert!(centre[0] > 1e-3 && (centre[0] / centre[1] - 1.0).abs() < 0.01, "{centre:?}");
    let lfe = bed_levels(2, 6, 3);
    assert!(lfe.iter().all(|&e| e < 1e-9), "{lfe:?}");
    let surround = bed_levels(2, 6, 5);
    assert!(surround[1] > 1e-3 && surround[0] < 1e-9, "{surround:?}");
    // 7.1 on 5.1: the side left joins the surround left.
    let side = bed_levels(6, 8, 6);
    assert!(side[4] > 1e-3 && side.iter().enumerate().all(|(c, &e)| c == 4 || e < 1e-9), "{side:?}");
    // 7.1 on 7.1.4: each lane on its own speaker.
    for lane in [2, 4, 7] {
        let e = bed_levels(12, 8, lane);
        assert!(e[lane] > 1e-3 && e.iter().enumerate().all(|(c, &v)| c == lane || v < 1e-9), "lane {lane}: {e:?}");
    }
}

// With the master volume at 0 nothing is heard, so nothing is mixed: the
// output is silence, and a voice moves on through the silence, so when the
// volume comes back it plays from where an output that was heard all along
// plays it.
#[test]
fn a_silent_output_mixes_nothing_and_a_voice_carries_on_through_the_silence() {
    let start = |volume: f32| {
        let mut out = Output::new(48000.0, 2, false, false, false, false, false);
        out.layout(audio::mixer::PANPOT, None, &[Some((30., 0.)), Some((-30., 0.))], -1, -1, 1.0);
        let data = out.voices.sample_channels(1, 48000, 1, 48000.0);
        for (i, x) in data.iter_mut().enumerate() {
            *x = 0.2 * ((i as f32) * 0.05).sin();
        }
        out.commands = vec![BIND as f64, 0.0, 0.0, START as f64, 0.0, 1.0, 0.0, 1.0, 1.0, 20000.0, 0.0, GAIN_SET as f64, 0.0, 1.0, 0.0];
        let len = out.commands.len();
        out.apply(len);
        out.volume(volume, 0.0, 0.0, true);
        out
    };
    let (mut heard, mut silent) = (start(1.0), start(0.0));
    for _ in 0..200 {
        heard.process();
        silent.process();
        assert!(silent.output.iter().all(|&x| x == 0.0));
    }
    let now = silent.frame as f64 / 48000.0;
    silent.volume(1.0, 0.0, now, true);
    // Past the limiter's lookahead the two outputs are the same.
    for _ in 0..4 {
        heard.process();
        silent.process();
    }
    assert!(heard.output.iter().any(|&x| x.abs() > 0.01));
    let worst = heard.output.iter().zip(&silent.output).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    assert!(worst < 1e-6, "the voice plays from the same place: {worst}");
}
