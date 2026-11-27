use super::{submit_requests, Batch, Prepared};
use audio_control::{
    engine::{MUSIC, NONE, SILENT},
    layout::{region, request_kind},
    streams::{Options, Request},
};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires prepared 2004 content in BFE_CONTENT_DIR"]
fn a_full_stream_worker_defers_a_healthy_bed() {
    let root = std::env::var("BFE_CONTENT_DIR").unwrap();
    let args = ["--local", "nab2", &root, "--side", "rep"].map(String::from);
    let options = crate::local::options::Options::parse(&args, None)
        .unwrap()
        .unwrap();
    let launch = crate::local::launch::Prepared::new(&options).unwrap();
    let bank = bfe_content_native::Loader::new(&root, bfe_family::GameFamily::Swbf1)
        .load("nab2", &launch.admission.query)
        .unwrap();
    let prepared = Prepared::new(&bank).unwrap();
    let assets = prepared.streams;
    let mut rt = prepared.runtime;
    let p = &mut rt.phase;
    p.engine.enhanced = false;
    let bed = p
        .director
        .regions
        .iter()
        .map(|r| r[region::SOUND] as usize)
        .find(|&s| {
            p.engine
                .sounds
                .get(s)
                .is_some_and(|s| s.kind == 4 && !s.entries.is_empty())
        })
        .unwrap();
    let mut options = Options::new(bed, MUSIC);
    options.segment = p.engine.sounds[bed].entries[0].id;
    let (short, _) = p
        .streams
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.duration > 0.)
        .min_by(|(_, a), (_, b)| a.duration.total_cmp(&b.duration))
        .unwrap();
    let filler = Request {
        voice: 0,
        generation: 0,
        segment: short,
        part: 0,
        sample: 0,
        operation: request_kind::LOAD,
        variant: NONE,
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let wait = || {
        assert!(Instant::now() < deadline, "stream worker stalled");
        std::thread::sleep(Duration::from_millis(1));
    };
    // Both bounded queues hold 128 requests, with one reply held by the worker.
    for _ in 0..257 {
        while !assets.request(filler) {
            wait();
        }
    }
    let initial = p.streams.play(&mut p.engine, options);
    assert_ne!(initial, SILENT);
    let mut refused = 0;
    submit_requests(&mut rt, &assets, &mut Batch::new(), &mut refused);
    assert_eq!(refused, 1);
    for _ in 0..257 {
        loop {
            if let Some(r) = assets.receive() {
                assert!(r.pcm.is_some());
                break;
            }
            wait();
        }
    }
    rt.clear_output();
    let p = &mut rt.phase;
    p.engine.wall_now += 4.;
    p.streams.update(&mut p.engine, &p.acoustics, 0.);
    assert_ne!(p.streams.play(&mut p.engine, options), SILENT);
    let req = *p.streams.requests.last().unwrap();
    assert!(assets.request(req));
    let pcm = loop {
        if let Some(r) = assets.receive() {
            break r.pcm.unwrap();
        }
        wait();
    };
    assert!(p.streams.ready(
        &mut p.engine,
        req.voice,
        req.generation,
        req.part,
        req.sample,
        pcm.frames as f64 / pcm.rate,
        pcm.channels
    ));
    assert!(p.streams.census().any(|(id, _)| id == bed));
}
