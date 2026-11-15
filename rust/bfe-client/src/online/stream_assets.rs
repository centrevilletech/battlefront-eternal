//! StreamAssets reads streamed segments off the window and device threads.
//! A segment stays Xbox ADPCM; the device decodes it a block at a time as it
//! plays (audio::voices::Sample::adpcm). The shared audio runtime still
//! chooses every segment, generation, channel layout and transition.
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
    pub sample: audio::voices::Sample,
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
            while let Ok(request) = jobs.recv() {
                let pcm = segments.get(request.segment).and_then(|segment| read(segment).ok().flatten());
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
fn read(segment: &Segment) -> Result<Option<Pcm>, std::io::Error> {
    // The whole authored segment, including padding to a substream chunk,
    // split into its substreams the way the browser decoder reads them.
    let mut file = std::fs::File::open(&segment.file)?;
    file.seek(SeekFrom::Start(segment.offset))?;
    let mut bytes = vec![0; segment.length];
    file.read_exact(&mut bytes)?;
    let count = segment.substreams.max(1);
    let parts = if count == 1 {
        vec![bytes]
    } else {
        if segment.interleave == 0 {
            return Ok(None);
        }
        let mut parts = vec![Vec::with_capacity(bytes.len() / count + segment.interleave); count];
        for (chunk, run) in bytes.chunks_exact(segment.interleave).enumerate() {
            parts[chunk % count].extend_from_slice(run);
        }
        parts
    };
    Ok(audio::voices::Sample::adpcm(segment.rate, segment.channels, parts)
        .map(|sample| Pcm { rate: segment.rate, channels: sample.channels, frames: sample.frames, sample }))
}
