//! Compare complete native sound control/DSP against the recorded WASM host.
use bfe_client::{audio::{AudioDevice,Status},sound::SoundOutput};
fn run()->Result<(),String>{
    let mut args=std::env::args().skip(1);let dir=std::path::PathBuf::from(args.next().ok_or("usage: native-sound ARCHIVE [--device]")?);
    let device=args.next().as_deref()==Some("--device");let mut sound=SoundOutput::load(&dir)?;
    let checks=sound.checks;let frames=sound.frames;let mut pcm=vec![0.0f32;frames*2];sound.fill(&mut pcm,&Status::default());
    let reference=std::fs::read(dir.join("reference.f32")).map_err(|e|e.to_string())?;
    if reference.len()!=pcm.len()*4{return Err("reference PCM dimensions differ".into());}
    let mut changed=0;let mut max=0.0f32;let mut sum=0.0f64;
    for channel in 0..2 {for frame in 0..frames {let i=(channel*frames+frame)*4;let expected=f32::from_le_bytes(reference[i..i+4].try_into().unwrap());let actual=pcm[frame*2+channel];let delta=(actual-expected).abs();changed+=usize::from(actual.to_bits()!=expected.to_bits());max=max.max(delta);sum+=(delta as f64).powi(2);}}
    if device {let stream=AudioDevice::open_sound(SoundOutput::load(&dir)?)?;std::thread::sleep(std::time::Duration::from_secs(20));stream.check()?;if stream.status.blocks.load(std::sync::atomic::Ordering::Relaxed)<8{return Err("device produced no sound blocks".into());}}
    println!("{}",serde_json::json!({"controlBankChecks":checks,"frames":frames,"changedSamples":changed,"max":max,"rms":(sum/pcm.len() as f64).sqrt(),"device":device}));
    if max>0.000015848932{return Err("native PCM differs by more than -96 dBFS".into());}Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
