use bfe_client::audio::{AudioDevice, MasterOutput, Status};
use audio::{master::Master, FRAMES};
use std::sync::atomic::Ordering;

#[test]
fn device_slices_equal_the_master_chain_without_losing_samples() {
    let pcm = vec![0.3, -0.15, -0.2, 0.1];
    let mut host = MasterOutput::new(48000, pcm.clone()).unwrap();
    let status = Status::default();
    let mut actual = vec![0.0f32; FRAMES * 2 * 8];
    // Odd sample counts exercise both stereo and quantum boundaries.
    let mut at = 0;
    for size in [1, 37, 256, 13, 511, 3, 1227] {
        host.fill(&mut actual[at..at + size], &status);
        at += size;
    }
    assert_eq!(at, actual.len());
    let mut master = Master::new(48000.0, 2, -1.0, 0.1, 0.005, cfg!(target_arch = "x86_64"));
    let mut reference = Vec::new();
    let mut feeds = [0.0; FRAMES * 2];
    let mut output = [0.0; FRAMES * 2];
    for block in 0..8 {
        for frame in 0..FRAMES { for channel in 0..2 {
            feeds[channel * FRAMES + frame] = pcm[(frame * 2 + channel) % pcm.len()];
        } }
        master.process(&feeds, None, &[0.0; FRAMES], None, None, block, &mut output);
        for frame in 0..FRAMES { reference.extend([output[frame], output[FRAMES + frame]]); }
    }
    assert_eq!(actual, reference);
    assert_eq!(status.blocks.load(Ordering::Relaxed), 8);
    assert!(actual.iter().any(|v| v.abs() > 0.01));
    assert!(MasterOutput::new(48000, vec![f32::NAN, 0.0]).is_err());
}

#[test]
#[ignore = "requires an explicitly configured null audio device"]
fn cpal_null_device_runs_master_pcm() {
    assert_eq!(std::env::var("BFE_AUDIO_NULL").as_deref(), Ok("1"));
    let device = AudioDevice::open(vec![0.2, -0.1, -0.2, 0.1], 48000).unwrap();
    let start = std::time::Instant::now();
    while device.status.blocks.load(Ordering::Relaxed) < 8 && start.elapsed().as_secs() < 3 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    device.check().unwrap();
    assert!(device.status.blocks.load(Ordering::Relaxed) >= 8);
    assert!(f32::from_bits(device.status.peak.load(Ordering::Relaxed)) > 0.01);
    device.pause().unwrap();
    device.play().unwrap();
}
