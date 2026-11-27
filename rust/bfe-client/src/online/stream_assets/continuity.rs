use super::*;

#[test]
fn an_original_shorter_than_a_block_starts_as_a_zero_frame_sample() {
    let file = std::env::temp_dir().join(format!("bfe-short-original-{}", std::process::id()));
    std::fs::write(&file, [0x57; 29]).unwrap();
    let assets = Assets::new(
        vec![Segment {
            file: file.clone(),
            offset: 3,
            length: 21,
            rate: 32000.,
            channels: 1,
            substreams: 1,
            interleave: 0,
        }],
        vec![],
    );
    assert!(assets.request(Request {
        generation: 7,
        voice: 2,
        segment: 0,
        part: 0,
        sample: 1,
        operation: audio_control::layout::request_kind::LOAD,
        variant: usize::MAX,
    }));
    let reply = assets.receive.recv_timeout(Duration::from_secs(5)).unwrap();
    std::fs::remove_file(file).unwrap();
    let pcm = reply.pcm.expect("a complete short segment can start");
    assert_eq!((pcm.frames, pcm.channels, pcm.rate), (0, 1, 32000.));
    assert_eq!(pcm.sample.frames, 0);
}

#[test]
fn a_bad_interior_page_continues_at_the_original_position() {
    for (blocks, rate) in [
        (1600, 48_000.0),
        (550, 24_000.0),
        (25, 24_000.0),
        (0, 48_000.0),
    ] {
        check_continuation(blocks, rate, OriginalFile::Present);
    }
    check_continuation(550, 24_000., OriginalFile::Missing);
}

#[test]
fn the_first_playback_refill_can_prepare_the_original() {
    check_continuation(1600, 48_000., OriginalFile::AfterReply);
}

#[test]
fn a_late_failure_ends_at_the_bad_page_when_the_original_will_not_fit() {
    check_continuation(1600, 48_000., OriginalFile::OverBudget);
}

#[derive(Debug, PartialEq)]
enum OriginalFile {
    Present,
    Missing,
    AfterReply,
    OverBudget,
}

fn check_continuation(blocks: usize, rate: f64, file: OriginalFile) {
    let dir = std::env::temp_dir().join(format!(
        "bfe-stream-continuity-{}-{file:?}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/audio-codecs/opus-hybrid.opus");
    let clean = std::fs::read(source).unwrap();
    let mut decoder = audio_decode::Decoder::new(0, 0., false);
    decoder.push(&clean, true).unwrap();
    let mut reference = Vec::new();
    loop {
        match decoder.step(2048).unwrap() {
            audio_decode::Step::Output(n) => reference.extend_from_slice(&decoder.output[..n]),
            audio_decode::Step::Done => break,
            _ => {}
        }
    }
    let mut broken = clean.clone();
    let pages: Vec<_> = clean
        .windows(4)
        .enumerate()
        .filter_map(|(i, w)| (w == b"OggS").then_some(i))
        .collect();
    broken[pages[4] - 1] ^= 1;
    let mut damaged = audio_decode::Decoder::new(0, 0., false);
    damaged.push(&broken, true).unwrap();
    let mut prefix = 0;
    loop {
        match damaged.step(2048) {
            Ok(audio_decode::Step::Output(n)) => prefix += n,
            Ok(audio_decode::Step::Done) => panic!("the interior page should fail"),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(prefix > 0 && prefix < reference.len());
    let opus = dir.join("track.opus");
    let bin = dir.join("original.bin");
    let pcm = |block: usize| (block as i16).wrapping_mul(7);
    let mut bytes = vec![0; 36 * blocks];
    for (index, block) in bytes.as_chunks_mut::<36>().0.iter_mut().enumerate() {
        block[..2].copy_from_slice(&pcm(index).to_le_bytes());
    }
    let mut length = bytes.len();
    if matches!(file, OriginalFile::Present | OriginalFile::OverBudget) {
        std::fs::write(&bin, &bytes).unwrap();
    }
    if file == OriginalFile::OverBudget {
        length = reserve::LIMIT + 36;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&bin)
            .unwrap()
            .set_len(length as u64)
            .unwrap();
    }
    std::fs::write(&opus, broken).unwrap();
    let assets = Assets::new(
        vec![Segment {
            file: bin.clone(),
            offset: 0,
            length,
            rate,
            channels: 1,
            substreams: 1,
            interleave: 0,
        }],
        vec![
            Variant::new(opus.clone(), 0.0, "mono", None),
            Variant::new(opus.clone(), 0.0, "mono", None),
        ],
    );
    let fetch = |generation, variant| {
        assert!(assets.request(Request {
            generation,
            voice: 1,
            segment: 0,
            part: 0,
            sample: 3,
            operation: 1,
            variant
        }));
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(r) = assets.receive() {
                break r.pcm;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let mut first = fetch(11, 0).expect("the playable Enhanced prefix starts");
    assert_eq!(
        first.frames,
        reference.len(),
        "the reply uses the Enhanced duration"
    );
    if file == OriginalFile::AfterReply {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(&bin, &bytes).unwrap();
    }
    let original_frames = (blocks as f64 * 64. * 48_000. / rate) as usize;
    assert!(matches!(
        first.sample.feed.as_deref(),
        Some(audio::streaming::Feed::Codec(_))
    ));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let checked = if blocks == 25 {
        for (at, &expected) in reference[..4096].iter().enumerate() {
            assert!(first.sample.available(at));
            assert_eq!(first.sample.value(0, at), expected);
        }
        // A single device refill starts checking. Keep the device here until
        // the decoder finds the bad page well beyond the ordinary ring.
        first.sample.consumed(4096);
        first.sample.feed();
        let watches = assets.durations.borrow();
        let end = &watches[0].end;
        while end.load(Ordering::Acquire) == usize::MAX {
            assert!(
                std::time::Instant::now() < deadline,
                "the unchecked tail was never reached"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(end.load(Ordering::Acquire), prefix);
        assert!(!first.sample.available(prefix - 1));
        4096
    } else {
        0
    };
    let playable = if matches!(file, OriginalFile::Missing | OriginalFile::OverBudget) {
        prefix
    } else {
        reference.len().min(prefix.max(original_frames))
    };
    for (at, &enhanced) in reference[..playable].iter().enumerate().skip(checked) {
        if at % 1024 == 0 {
            first.sample.consumed(at);
            first.sample.feed();
        }
        while !first.sample.available(at) {
            assert!(
                at < first.sample.frames,
                "decoder ended at {at} of {} promised frames",
                first.frames
            );
            assert!(std::time::Instant::now() < deadline);
            first.sample.consumed(at);
            first.sample.feed();
            std::thread::sleep(Duration::from_millis(1));
        }
        let expected = if at < prefix {
            enhanced
        } else {
            let frame = at as f64 * (rate / 48_000.);
            let i = frame as usize;
            let a = f32::from(pcm(i / 64)) / 32768.0;
            let b = f32::from(pcm((i + 1).min(blocks * 64 - 1) / 64)) / 32768.0;
            a + (b - a) * frame.fract() as f32
        };
        assert_eq!(first.sample.value(0, at), expected, "frame {at}");
    }
    if playable < first.frames {
        while first.sample.frames != playable {
            assert!(
                std::time::Instant::now() < deadline,
                "the bad stream must end"
            );
            first.sample.consumed(playable);
            first.sample.feed();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!first.sample.available(playable));
        assert!(first.sample.ring.as_ref().unwrap().eof);
    }
    if matches!(file, OriginalFile::OverBudget | OriginalFile::Missing) {
        std::fs::remove_dir_all(dir).unwrap();
        return;
    }
    std::fs::write(&opus, clean).unwrap();
    let again = fetch(12, 1).unwrap();
    if blocks == 0 {
        assert_eq!(again.frames, 0);
        assert!(again.sample.feed.is_none());
    } else {
        assert!(matches!(
            again.sample.feed.as_deref(),
            Some(audio::streaming::Feed::Adpcm(_))
        ));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn checking_a_healthy_unequal_track_preserves_the_playback_decoder_for_seeks() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/audio-codecs/opus-hybrid.opus"
    ));
    let mut opus = Opus {
        decoder: audio_decode::Decoder::new(0, 0., false),
        channels: 1,
        bytes: Arc::new(bytes.to_vec()),
        at: 0,
    };
    assert!(matches!(
        opus.step(WINDOW),
        Ok(audio_decode::Step::Metadata)
    ));
    opus.decoder.tail(bytes).unwrap();
    let frames = opus.decoder.info.as_ref().unwrap().frames.unwrap() as usize;
    let (mut job, mut ahead) = decoder_windows(48000., 1, frames);
    job.opus = Some(opus);
    assert!(!prime(&mut job, &mut ahead));
    let position = job.opus.as_ref().unwrap().decoder.position();
    job.end = Some(Arc::new(AtomicUsize::new(usize::MAX)));
    job.hold.playback.store(PLAYING, Ordering::Release);
    let (queue, receiver) = sync_channel(1);
    drop(receiver);
    job.standby.start(&queue, 0, &job.hold);
    while job.check_ahead() {}
    assert_eq!(job.opus.as_ref().unwrap().decoder.position(), position);
    assert!(job.lookahead.is_none());
}
