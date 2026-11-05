// Native correctness driver. It exercises the same bounded feed/step API
// as the browser worker; no host decoder or audio device is involved.
use audio_decode::{Decoder, Step};
use std::{
    env, fs,
    io::{Read, Write},
};
fn main() {
    let args: Vec<String> = env::args().collect();
    let rate = args[2].parse().unwrap();
    let mut input = fs::File::open(&args[1]).unwrap();
    let mut decoder = Decoder::new(rate, 0.0, false);
    let mut eof = false;
    let mut buffer = [0u8; 65536];
    let mut planes: Vec<Vec<f32>> = Vec::new();
    loop {
        match decoder.step(2048).unwrap() {
            Step::NeedInput => {
                assert!(!eof, "decoder requested bytes after EOF");
                let n = input.read(&mut buffer).unwrap();
                eof = n == 0;
                decoder.push(&buffer[..n], eof).unwrap();
            }
            Step::Metadata => {
                let info = decoder.info.as_ref().unwrap();
                println!("{:?}", info);
                planes = vec![Vec::new(); info.channels];
            }
            Step::Output(frames) => {
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane.extend_from_slice(&decoder.output[c * frames..(c + 1) * frames]);
                }
            }
            Step::Done => break,
        }
    }
    let mut out = fs::File::create(&args[3]).unwrap();
    for plane in &planes {
        for value in plane {
            out.write_all(&value.to_le_bytes()).unwrap();
        }
    }
    println!("{} frames", planes[0].len());
}
