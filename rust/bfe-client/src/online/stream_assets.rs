//! StreamAssets reads streamed segments off the window and device threads.
//! A segment stays Xbox ADPCM; the device decodes it a block at a time as it
//! plays (audio::voices::Sample::adpcm). An upscaled (Opus) segment decodes
//! on a thread of its own, a few windows ahead of playback, so the device
//! only copies it. The shared audio runtime still chooses every segment,
//! generation, channel layout and transition.
use audio_control::streams::Request;
use std::{
    collections::VecDeque,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    sync::mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError},
    time::Duration,
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
    ahead: Option<std::thread::JoinHandle<()>>,
}
/// An upscaled segment the enhanced mode plays in place of the original
/// (tools/enhance_audio.py's Opus file for a layout) and its playback gain.
pub struct Variant {
    pub file: PathBuf,
    pub gain_db: f64,
}
impl Assets {
    pub fn new(segments: Vec<Segment>, variants: Vec<Variant>) -> Self {
        let (send, jobs) = sync_channel::<Request>(128);
        let (results, receive) = sync_channel(128);
        let (decode, decodes) = channel::<Job>();
        let ahead = std::thread::Builder::new().name("audio decode ahead".into()).spawn(move || decode_ahead(decodes)).ok();
        let thread = std::thread::spawn(move || {
            while let Ok(request) = jobs.recv() {
                let pcm = match variants.get(request.variant) {
                    Some(variant) => read_variant(variant, &decode).map_err(|err| eprintln!("enhanced audio: {} ({err})", variant.file.display())).ok(),
                    None => segments.get(request.segment).and_then(|segment| read(segment).ok().flatten()),
                };
                if results.send(Response { request, pcm }).is_err() {
                    break;
                }
            }
        });
        Self { send: Some(send), receive, thread: Some(thread), ahead }
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
        // The loading thread held the only way to send it work: it ends now.
        if let Some(thread) = self.ahead.take() {
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

// The Opus decoder feeding an upscaled segment's ring as it plays, given
// its compressed bytes in reads of 64 KiB (the decoder's input is bounded).
struct Opus {
    decoder: audio_decode::Decoder,
    channels: usize,
    bytes: Vec<u8>,
    at: usize,
    failed: bool,
}
impl Opus {
    // Moves on toward output frame `frame` without decoding what it passes;
    // the frame the next window starts at.
    fn skip(&mut self, frame: usize) -> Result<usize, String> {
        loop {
            match self.decoder.skip(frame as u64)? {
                Some(at) => return Ok(at as usize),
                None => {
                    if self.at >= self.bytes.len() {
                        return Err("truncated upscaled segment".into());
                    }
                    let end = (self.at + 65536).min(self.bytes.len());
                    self.decoder.push(&self.bytes[self.at..end], end == self.bytes.len())?;
                    self.at = end;
                }
            }
        }
    }
    // The next step that is not a request for input.
    fn step(&mut self, frames: usize) -> Result<audio_decode::Step, String> {
        loop {
            match self.decoder.step(frames)? {
                audio_decode::Step::NeedInput => {
                    if self.at >= self.bytes.len() {
                        return Err("truncated upscaled segment".into());
                    }
                    let end = (self.at + 65536).min(self.bytes.len());
                    self.decoder.push(&self.bytes[self.at..end], end == self.bytes.len())?;
                    self.at = end;
                }
                step => return Ok(step),
            }
        }
    }
}
impl audio::streaming::Producer for Opus {
    fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)> {
        loop {
            match self.step(frames) {
                Ok(audio_decode::Step::Output(n)) => return Some((&self.decoder.output[..n * self.channels], n)),
                Ok(audio_decode::Step::Metadata) => continue,
                Ok(_) => return None,
                Err(err) => {
                    if !self.failed {
                        self.failed = true;
                        eprintln!("enhanced audio: an upscaled segment stopped ({err})");
                    }
                    return None;
                }
            }
        }
    }
}
// Frames per decoded window, the windows that may wait for the device (with
// the device's own 16384-frame ring, about 0.7 s decoded ahead at 48 kHz),
// and the windows decoded before a segment is handed over, which fill that
// ring so playback can start at once.
const WINDOW: usize = 2048;
const AHEAD: usize = 8;
const FIRST: usize = 8;
// A window: its PCM, its frames, and the segment frame it starts at.
type Window = Option<(Vec<f32>, usize, usize)>;
// While nobody hears a segment the device holds its decoding and says where
// playback has got to; decoding then picks up from there.
#[derive(Default)]
struct Hold {
    held: AtomicBool,
    at: AtomicUsize,
}
// One upscaled segment decoding ahead of its playback: windows go to the
// device, and the device hands their buffers back for the next ones.
struct Job {
    opus: Opus,
    windows: SyncSender<Window>,
    spare: Receiver<Vec<f32>>,
    held: Option<Window>,
    hold: Arc<Hold>,
    // The segment frame the next window starts at.
    position: usize,
}
impl Job {
    // The next window (None at the end), in a buffer the device gave back
    // when there is one.
    fn next(&mut self) -> Window {
        let Job { opus, spare, .. } = self;
        loop {
            let (pcm, frames) = audio::streaming::Producer::produce(opus, WINDOW)?;
            if frames == 0 {
                continue;
            }
            let mut buffer = spare.try_recv().unwrap_or_default();
            buffer.clear();
            buffer.extend_from_slice(pcm);
            let start = self.position;
            self.position += frames;
            return Some((buffer, frames, start));
        }
    }
    // Decodes until the device has AHEAD windows waiting; false once the
    // segment has ended or the device let it go. Decodes nothing while the
    // device holds it, and jumps to where playback got to when let go.
    fn pump(&mut self) -> bool {
        if self.hold.held.load(Ordering::Acquire) {
            // The device's end of the hold is gone with the segment.
            return Arc::strong_count(&self.hold) > 1;
        }
        let at = self.hold.at.load(Ordering::Relaxed);
        if at > self.position + WINDOW {
            match self.opus.skip(at) {
                Ok(start) => {
                    self.position = start;
                    self.held = None;
                }
                Err(err) => {
                    eprintln!("enhanced audio: an upscaled segment could not skip ahead ({err})");
                    return false;
                }
            }
        }
        loop {
            let window = match self.held.take() {
                Some(window) => window,
                None => self.next(),
            };
            let end = window.is_none();
            match self.windows.try_send(window) {
                Ok(()) if end => return false,
                Ok(()) => {}
                Err(TrySendError::Full(window)) => {
                    self.held = Some(window);
                    return true;
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
    }
}
// The decode-ahead thread: every upscaled segment playing, kept AHEAD windows
// ahead of the device. It looks again every 2 ms (a window lasts 43 ms), every
// 20 ms while the device holds every segment (nobody hears them).
fn decode_ahead(jobs: Receiver<Job>) {
    let mut live: Vec<Job> = Vec::new();
    loop {
        let held = live.iter().all(|job| job.hold.held.load(Ordering::Relaxed));
        match jobs.recv_timeout(Duration::from_millis(if live.is_empty() { 500 } else if held { 20 } else { 2 })) {
            Ok(job) => live.push(job),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(job) = jobs.try_recv() {
            live.push(job);
        }
        live.retain_mut(Job::pump);
    }
}
// An upscaled segment as the device sees it: windows decoded before it was
// handed over, then those the decode-ahead thread sends. Taking one only
// moves a buffer; the one before goes back to be decoded into again.
struct Ahead {
    first: VecDeque<(Vec<f32>, usize, usize)>,
    windows: Receiver<Window>,
    spare: SyncSender<Vec<f32>>,
    current: Option<Vec<f32>>,
    ended: bool,
    hold: Arc<Hold>,
    // The segment frame the ring wants next.
    wanted: usize,
    // Buffers the decode-ahead thread no longer takes back (its part has
    // ended): kept, in room reserved beforehand, and freed with the segment
    // off the device thread.
    kept: Vec<Vec<f32>>,
}
impl Ahead {
    // A buffer done with goes back to be decoded into again (or is kept, to be
    // freed with the segment off the device thread).
    fn give_back(&mut self, done: Vec<f32>) {
        if let Err(TrySendError::Full(done) | TrySendError::Disconnected(done)) = self.spare.try_send(done) {
            if self.kept.len() < self.kept.capacity() {
                self.kept.push(done);
            }
        }
    }
}
impl audio::streaming::Producer for Ahead {
    fn produce(&mut self, _frames: usize) -> Option<(&[f32], usize)> {
        if let Some(done) = self.current.take() {
            self.give_back(done);
        }
        if self.ended {
            return None;
        }
        self.hold.held.store(false, Ordering::Release);
        loop {
            let (mut buffer, frames, start) = match self.first.pop_front() {
                Some(window) => window,
                None => match self.windows.try_recv() {
                    Ok(Some(window)) => window,
                    Err(TryRecvError::Empty) => return Some((&[], 0)),
                    Ok(None) | Err(TryRecvError::Disconnected) => {
                        self.ended = true;
                        return None;
                    }
                },
            };
            // Windows from before a skip are dropped, and the one holding the
            // frame wanted loses its earlier frames (channel by channel, in place).
            if start + frames <= self.wanted {
                self.give_back(buffer);
                continue;
            }
            let cut = self.wanted.saturating_sub(start);
            let left = frames - cut;
            if cut > 0 {
                let channels = buffer.len() / frames;
                for c in 0..channels {
                    buffer.copy_within(c * frames + cut..(c + 1) * frames, c * left);
                }
                buffer.truncate(channels * left);
            }
            self.wanted = start + frames;
            return Some((self.current.insert(buffer).as_slice(), left));
        }
    }
    fn skip(&mut self, frame: usize) -> bool {
        self.hold.at.store(frame, Ordering::Relaxed);
        self.hold.held.store(true, Ordering::Release);
        self.wanted = self.wanted.max(frame);
        while let Some((buffer, ..)) = self.first.pop_front() {
            self.give_back(buffer);
        }
        while let Ok(Some((buffer, ..))) = self.windows.try_recv() {
            self.give_back(buffer);
        }
        true
    }
}
/// An upscaled segment, kept compressed and decoded ahead of playback on the
/// decode-ahead thread.
fn read_variant(variant: &Variant, decode: &Sender<Job>) -> Result<Pcm, String> {
    let bytes = std::fs::read(&variant.file).map_err(|e| e.to_string())?;
    let tail = bytes[bytes.len().saturating_sub(65536)..].to_vec();
    let mut opus = Opus { decoder: audio_decode::Decoder::new(0, variant.gain_db, false), channels: 0, bytes, at: 0, failed: false };
    match opus.step(2048)? {
        audio_decode::Step::Metadata => {}
        _ => return Err("no audio metadata".into()),
    }
    if opus.decoder.info.as_ref().is_some_and(|info| info.frames.is_none()) {
        opus.decoder.tail(&tail)?;
    }
    let info = opus.decoder.info.clone().ok_or("no audio metadata")?;
    let (rate, channels, frames) = (info.rate as f64, info.channels, info.frames.unwrap_or(0) as usize);
    opus.channels = channels;
    let (windows, from_ahead) = sync_channel(AHEAD);
    let (spare, spares) = sync_channel(AHEAD + FIRST + 4);
    let hold = Arc::new(Hold::default());
    let mut job = Job { opus, windows, spare: spares, held: None, hold: hold.clone(), position: 0 };
    let mut first = VecDeque::with_capacity(FIRST);
    let mut ended = false;
    while first.len() < FIRST {
        match job.next() {
            Some(window) => first.push_back(window),
            None => {
                ended = true;
                break;
            }
        }
    }
    let ahead = Ahead { first, windows: from_ahead, spare, current: None, ended: false, hold, wanted: 0, kept: Vec::with_capacity(FIRST + AHEAD + 4) };
    if ended {
        // Short enough to be decoded whole already: the end follows the windows.
        let _ = job.windows.try_send(None);
    } else if decode.send(job).is_err() {
        return Err("the decode-ahead thread is gone".into());
    }
    let sample = audio::voices::Sample::codec(rate, channels, frames, Box::new(ahead)).ok_or("unsupported upscaled segment")?;
    static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
        eprintln!("enhanced audio: streaming {} ({channels} ch, {frames} frames)", variant.file.display());
    }
    Ok(Pcm { rate, channels, frames: sample.frames, sample })
}
