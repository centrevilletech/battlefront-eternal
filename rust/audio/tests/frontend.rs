//! Front-end shell audio and output render-phase tests ported from
//! `tests/audio-frontend.test.js` and `tests/audio-light.test.js`.
//!
//! These tests verify that `audio::frontend::Frontend` retires completed
//! sources once with their generation so slots can be reused cleanly, and
//! that `audio::output::Output` renders Classic stereo voices, preserves
//! scheduled streams across layout rebuilds, routes speaker-test tones after
//! a muted mono mix, and keeps surround bed lanes when a narrower segment is
//! queued.
//!
//! Both `Frontend` (menu, loading, and shell music/effects) and `Output`
//! (in-battle spatial/surround/headphone/light rendering) own their complete
//! 128-frame Web Audio / native device quanta in Rust without calling back
//! into host nodes or allocating during `process`.

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
// In Classic stereo (`light_mode`), voices bypass the Enhanced per-voice lowpass
// and surround Master limiter, feeding the two-channel OpenAL Soft light panner
// and WaveShaper table directly.
#[test]
fn classic_stereo_renders_panned_voice_without_host_dsp() {
    let mut out = Output::new(48000.0, 2, false, false, false, false, false);
    assert!(out.light_mode);

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

// Ported from `tests/audio-light.test.js`:
// "layout changes retain the host, decoded samples and scheduled sources".
//
// When the player switches speaker layouts (`output_rebuild`), the output
// stage swaps in a new mixer and master chain for the new channel count while
// moving `voices`, `streams`, and `placements` across so active beds keep
// playing from their current cursors without re-uploading PCM.
#[test]
fn layout_changes_retain_decoded_samples_and_scheduled_sources() {
    let h = audio::output_new(48000.0, 2, 0, 0, 0, 0, 0);
    unsafe {
        // Upload a 1-second stereo bed filled with 0.1 into sample 1.
        let sample_ptr = audio::output_sample(h, 1, 48000, 2, 48000.0);
        std::slice::from_raw_parts_mut(sample_ptr, 2 * 48000).fill(0.1);

        // Bind stream 0 (2 channels, kind 0, nonspatial) and start sample 1 looping.
        let cmds = [
            STREAM_BIND as f64,
            0.0,
            2.0,
            0.0,
            0.0,
            STREAM_START as f64,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            1.0,
            1.0,
        ];
        let cmd_ptr = audio::output_commands_ptr(h, cmds.len() as u32);
        std::slice::from_raw_parts_mut(cmd_ptr, cmds.len()).copy_from_slice(&cmds);
        audio::output_commands(h, cmds.len() as u32);

        // Rebuild across 5.1 (6ch), 7.1 (8ch), and stereo (2ch) without re-uploading PCM.
        // Run 5 quanta after each rebuild to clear the limiter/stabilizer lookahead delay.
        for channels in [6u32, 8, 2] {
            audio::output_rebuild(h, channels, 0, 0, 0, 0);
            for block in 0..5 {
                audio::output_process(h, (1024 + block * FRAMES) as f64);
            }
            let out_ptr = audio::output_out(h);
            let rendered =
                std::slice::from_raw_parts(out_ptr, channels as usize * FRAMES);
            assert!(
                rendered.iter().any(|&v| v != 0.0),
                "rebuilt {channels}-channel output was silent"
            );
        }
    }
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
