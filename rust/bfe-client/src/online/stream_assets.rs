//! StreamAssets performs byte I/O and Xbox ADPCM decoding off the window and
//! device threads. The shared audio runtime still chooses every segment,
//! generation, channel layout and scheduled transition.
use audio_control::streams::Request;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::mpsc::{sync_channel, Receiver, SyncSender},
};
pub struct Segment {
    pub file: PathBuf,
    pub offset: u64,
    pub length: usize,
    pub rate: f64,
    pub channels: usize,
    pub substreams: usize,
    pub interleave: usize,
}
pub struct Pcm {
    pub rate: f64,
    pub channels: usize,
    pub frames: usize,
    pub data: Vec<f32>,
}
pub struct Response {
    pub request: Request,
    pub pcm: Option<Pcm>,
}
pub struct Assets {
    send: Option<SyncSender<Request>>,
    receive: Receiver<Response>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Assets {
    pub fn new(segments: Vec<Segment>) -> Self {
        let (send, jobs) = sync_channel::<Request>(128);
        let (results, receive) = sync_channel(128);
        let thread = std::thread::spawn(move || {
            let mut decoder = audio::adpcm::Decoder::default();
            while let Ok(request) = jobs.recv() {
                let pcm = segments.get(request.segment).and_then(|segment| decode(segment, &mut decoder).ok());
                if results.send(Response { request, pcm }).is_err() {
                    break;
                }
            }
        });
        Self { send: Some(send), receive, thread: Some(thread) }
    }
    pub fn request(&self, request: Request) -> bool {
        self.send.as_ref().is_some_and(|sender| sender.try_send(request).is_ok())
    }
    pub fn receive(&self) -> Option<Response> {
        self.receive.try_recv().ok()
    }
}
impl Drop for Assets {
    fn drop(&mut self) {
        self.send.take();
        while self.receive.recv().is_ok() {}
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn decode(segment: &Segment, decoder: &mut audio::adpcm::Decoder) -> Result<Pcm, std::io::Error> {
    // Read the whole authored segment, including padding to a substream chunk.
    // This is the browser decoder's same interleave and channel-major output.
    let mut file = std::fs::File::open(&segment.file)?;
    file.seek(SeekFrom::Start(segment.offset))?;
    decoder.bytes.resize(segment.length, 0);
    file.read_exact(&mut decoder.bytes)?;
    let frames = decoder.decode(segment.channels, segment.substreams, segment.interleave);
    let channels = segment.channels * segment.substreams;
    let mut data = Vec::with_capacity(frames * channels);
    for channel in &decoder.output {
        data.extend_from_slice(&channel[..frames]);
    }
    Ok(Pcm { rate: segment.rate, channels, frames, data })
}
