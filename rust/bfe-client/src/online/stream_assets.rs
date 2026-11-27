//! StreamAssets reads streamed segments off the window and device threads.
//! A segment stays Xbox ADPCM; the device decodes it a block at a time as it
//! plays (audio::voices::Sample::adpcm). An upscaled (Opus) segment decodes
//! on a thread of its own, a few windows ahead of playback, so the device
//! only copies it. The shared audio runtime still chooses every segment,
//! generation, channel layout and transition.
mod lookahead;
mod reserve;
mod surround;
use audio_control::streams::Request;
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        Arc, OnceLock,
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
    end: Option<Arc<AtomicUsize>>,
}
struct DurationWatch {
    request: Request,
    frames: usize,
    rate: f64,
    end: Arc<AtomicUsize>,
}
pub struct Assets {
    durations: RefCell<Vec<DurationWatch>>,
    send: Option<SyncSender<Request>>,
    receive: Receiver<Response>,
    thread: Option<std::thread::JoinHandle<()>>,
    ahead: Option<std::thread::JoinHandle<()>>,
    reserves: Option<std::thread::JoinHandle<()>>,
}
/// An upscaled segment the enhanced mode plays in place of the original
/// (tools/enhance_audio.py's Opus file for a layout) and its playback gain.
pub struct Variant {
    pub file: PathBuf,
    pub gain_db: f64,
    pub surround: usize,
}
impl Variant {
    pub fn new(file: PathBuf, gain_db: f64, layout: &str, kind: Option<&str>) -> Self {
        let surround = match (kind, layout) {
            (Some("music"), "5.1") => 6,
            (Some("music"), "7.1") => 8,
            _ => 0,
        };
        Self { file, gain_db, surround }
    }
}
fn file_ids<K: std::hash::Hash + Eq>(keys: impl Iterator<Item = K>) -> Vec<usize> {
    let mut ids = HashMap::new();
    keys.enumerate().map(|(i, key)| *ids.entry(key).or_insert(i)).collect()
}
impl Assets {
    pub fn prepare(control: &mut audio_control::streams::Streams, segments: Vec<Segment>, variants: Vec<Variant>) -> Self {
        let original_files = file_ids(segments.iter().map(|s| (&s.file, s.offset, s.length)));
        let variant_files = file_ids(variants.iter().map(|v| &v.file));
        control.set_source_files(original_files, variant_files.clone());
        Self::with_files(segments, variants, variant_files)
    }
    #[cfg(test)]
    pub(crate) fn new(segments: Vec<Segment>, variants: Vec<Variant>) -> Self {
        let files = file_ids(variants.iter().map(|v| &v.file));
        Self::with_files(segments, variants, files)
    }
    fn with_files(segments: Vec<Segment>, variants: Vec<Variant>, files: Vec<usize>) -> Self {
        let (send, jobs) = sync_channel::<Request>(128);
        let (results, receive) = sync_channel(128);
        let (decode, decodes) = channel::<Job>();
        let segments = Arc::new(segments);
        let originals = segments.clone();
        let (reserve, work) = sync_channel(32);
        let reserves = if variants.is_empty() {
            None
        } else {
            std::thread::Builder::new().name("audio stream originals".into())
                .spawn(move || reserve::run(originals, work)).ok()
        };
        let ahead = std::thread::Builder::new().name("audio decode ahead".into()).spawn(move || decode_ahead(decodes)).ok();
        let thread = std::thread::spawn(move || {
            // Several manifest entries can name the same file. A decoder reports
            // failure to this battle's asset worker even after its voice is reused.
            let failed: Arc<[_]> = (0..files.len()).map(|_| OnceLock::<u64>::new()).collect();
            while let Ok(request) = jobs.recv() {
                let original = segments.get(request.segment);
                let mut end = None;
                let pcm = variants.get(request.variant).and_then(|variant| {
                    let file = files[request.variant];
                    if failed[file].get().is_some() { return None; }
                    let source = Source { request, failed: failed.clone(), file, reserve: reserve.clone() };
                    match read_variant(variant, &decode, Some((source, original))) {
                        Ok((pcm, watch)) => { end = watch; Some(pcm) }
                        Err(err) => {
                            if failed[file].set(request.generation).is_ok() {
                                eprintln!("enhanced audio: {} ({err}); falling back to original stream", variant.file.display());
                            }
                            None
                        }
                    }
                }).or_else(|| {
                    let pcm = read(original?).ok().flatten()?;
                    match variants.get(request.variant) {
                        Some(v) if v.surround != 0 && pcm.channels == 2 => surround_original(pcm, v.surround, &decode).ok(),
                        _ => Some(pcm),
                    }
                });
                if results.send(Response { request, pcm, end }).is_err() {
                    break;
                }
            }
        });
        Self { durations: RefCell::new(Vec::with_capacity(audio_control::streams::STREAM_VOICES * 3)),
            send: Some(send), receive, thread: Some(thread), ahead, reserves }
    }
    pub fn request(&self, request: Request) -> bool {
        self.send.as_ref().is_some_and(|sender| sender.try_send(request).is_ok())
    }
    pub fn receive(&self) -> Option<Response> {
        let mut response = self.receive.try_recv().ok()?;
        let r = response.request;
        let mut watches = self.durations.borrow_mut();
        watches.retain(|w| w.request.voice != r.voice || w.request.part != r.part);
        if r.voice < audio_control::streams::STREAM_VOICES && r.part < 3 {
            if let (Some(end), Some(pcm)) = (response.end.take(), &response.pcm) {
                watches.push(DurationWatch { request: r, rate: pcm.rate, frames: pcm.frames, end });
            }
        }
        Some(response)
    }

    /// Apply confirmed end frames to the same native sample that was accepted
    /// in this part. Receiving its replacement retires the old sample's watch.
    pub fn update_durations(&self, control: &mut audio_control::streams::Streams) {
        self.durations.borrow_mut().retain(|watch| {
            let end = watch.end.load(Ordering::Acquire);
            if end == usize::MAX { return true; }
            if end < watch.frames {
                control.shorten(watch.request, (watch.frames - end) as f64 / watch.rate);
            }
            false
        });
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
        if let Some(thread) = self.reserves.take() {
            let _ = thread.join();
        }
    }
}
struct Original {
    rate: f64,
    channels: usize,
    frames: usize,
    parts: Vec<Vec<u8>>,
}
impl Original {
    fn pcm(self) -> Option<Pcm> {
        let sample = audio::voices::Sample::adpcm(self.rate, self.channels, self.parts)?;
        Some(Pcm { rate: self.rate, channels: sample.channels, frames: sample.frames, sample })
    }
}
fn read(segment: &Segment) -> Result<Option<Pcm>, std::io::Error> {
    Ok(read_original(segment)?.and_then(Original::pcm))
}
fn read_original(segment: &Segment) -> Result<Option<Original>, std::io::Error> {
    // The whole authored segment, including padding to a substream chunk,
    // split into its substreams the way the client decoder reads them.
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
    let channels = segment.channels;
    if !(1..=12).contains(&channels.saturating_mul(count)) { return Ok(None); }
    let frames = parts[0].len() / (36 * channels) * 64;
    Ok(Some(Original { rate: segment.rate, channels, frames, parts }))
}

// The Opus decoder feeding an upscaled segment's ring as it plays, given
// its compressed bytes in reads of 64 KiB (the decoder's input is bounded).
struct Opus {
    decoder: audio_decode::Decoder,
    channels: usize,
    bytes: Arc<Vec<u8>>,
    at: usize,
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
                Err(_) => return None,
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
    playback: AtomicU8,
}
const PRIMED: u8 = 1;
const PLAYING: u8 = 2;
impl Hold {
    fn playing(&self) {
        if self.playback.load(Ordering::Relaxed) == PRIMED {
            self.playback.store(PLAYING, Ordering::Release);
        }
    }
}
struct Source {
    request: Request,
    failed: Arc<[OnceLock<u64>]>,
    file: usize,
    reserve: SyncSender<reserve::Work>,
}
impl Source {
    fn failed(&self, reason: std::fmt::Arguments<'_>) {
        let r = self.request;
        if self.failed[self.file].set(r.generation).is_ok() {
            eprintln!("enhanced audio: variant {} voice {} generation {} {reason}", r.variant, r.voice, r.generation);
        }
    }
}
// One upscaled segment decoding ahead of its playback: windows go to the
// device, and the device hands their buffers back for the next ones.
struct Job {
    opus: Option<Opus>,
    source: Option<Source>,
    original: Option<Pcm>,
    // Fields drop in order. Free the active ADPCM backing before returning
    // its credit to readers for other voices or battles.
    standby: reserve::Standby,
    covered: usize,
    lookahead: Option<lookahead::Lookahead>,
    inspection: lookahead::Inspection,
    end: Option<Arc<AtomicUsize>>,
    substitute_layout: usize,
    surround: Option<surround::Surround>,
    rate: f64,
    channels: usize,
    frames: usize,
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
        if self.position >= self.frames { return None; }
        let start = self.position;
        let mut buffer = self.spare.try_recv().unwrap_or_default();
        buffer.clear();
        if let Some(cache) = &mut self.lookahead {
            let frames = cache.take(start, &mut buffer);
            if frames > 0 {
                self.position += frames;
                return Some((buffer, frames, start));
            }
        }
        if let Some(opus) = self.opus.as_mut() {
            while let Some((pcm, frames)) = audio::streaming::Producer::produce(opus, WINDOW) {
                if frames == 0 { continue; }
                let take = frames.min(self.frames - start);
                for plane in pcm.chunks_exact(frames) { buffer.extend_from_slice(&plane[..take]); }
                self.position += take;
                return Some((buffer, take, start));
            }
            self.fail_at(start);
            if start >= self.frames { return None; }
        }
        if self.original.is_none() {
            let original = self.standby.take()?;
            let covered = (original.frames as f64 * self.rate / original.rate).round() as usize;
            self.frames = self.frames.min(covered);
            if let Some(limit) = &self.end { limit.store(self.frames.max(start), Ordering::Release); }
            if start >= self.frames { return None; }
            self.original = original.pcm();
            if self.substitute_layout == self.channels && self.original.as_ref()?.channels == 2 && matches!(self.channels, 6 | 8) {
                self.surround = Some(surround::Surround::new(self.channels));
            }
        }
        let frames = WINDOW.min(self.frames - start);
        let original = self.original.as_mut()?;
        let ratio = original.rate / self.rate;
        buffer.resize(frames * self.channels, 0.0);
        if let Some(mix) = &mut self.surround {
            mix.render(original, start, frames, &mut buffer);
            self.position += frames;
            return Some((buffer, frames, start));
        }
        for i in 0..frames {
            let at = (start + i) as f64 * ratio;
            let index = (at as usize).min(original.frames - 1);
            let next = (index + 1).min(original.frames - 1);
            if !original.sample.available(next) {
                original.sample.skip_to(index);
                original.sample.consumed(index);
                original.sample.feed();
            }
            for c in 0..self.channels.min(original.channels) {
                let a = original.sample.value(c, index);
                let b = original.sample.value(c, next);
                buffer[c * frames + i] = a + (b - a) * at.fract() as f32;
            }
        }
        self.position += frames;
        Some((buffer, frames, start))
    }
    fn fail_at(&mut self, at: usize) {
        if let Some(source) = &self.source { source.failed(format_args!("stopped at frame {at}")); }
        self.opus = None;
        self.frames = self.frames.min(at.max(self.covered));
        if let Some(end) = &self.end {
            if at >= self.covered {
                end.store(at, Ordering::Release);
            } else if self.standby.prepared() {
                if !self.standby.available() { self.frames = at; }
                end.store(self.frames, Ordering::Release);
            }
        }
    }
    // Decode one extra window only after servicing every voice's usual rings.
    // The same decoder supplies playback, including any last partial window.
    fn check_ahead(&mut self) -> bool {
        if self.end.is_none() || self.opus.is_none() || self.position >= self.frames
            || self.hold.playback.load(Ordering::Acquire) != PLAYING
            || self.hold.held.load(Ordering::Acquire) || !self.standby.prepared() {
            return false;
        }
        let checked = self.inspection.check(&self.opus.as_ref().unwrap().bytes);
        if let lookahead::Inspection::Damaged(offset) = self.inspection {
            if let Some(source) = &self.source { source.failed(format_args!("damaged Ogg page at byte {offset}")); }
            self.inspection = lookahead::Inspection::Finished;
            self.lookahead = lookahead::Lookahead::new(self.position, self.frames, self.channels);
        }
        if checked { return true; }
        let Some(cache) = &mut self.lookahead else { return false };
        let Some(at) = cache.next(self.position, self.frames) else { return false };
        let opus = self.opus.as_mut().unwrap();
        match audio::streaming::Producer::produce(opus, WINDOW) {
            Some((pcm, n)) if n > 0 => cache.append(pcm, n, n.min(self.frames - at)),
            Some(_) => {}
            None => self.fail_at(at),
        }
        true
    }
    // Decodes until the device has AHEAD windows waiting; false once the
    // segment has ended or the device let it go. Decodes nothing while the
    // device holds it, and jumps to where playback got to when let go.
    fn pump(&mut self) -> bool {
        if self.hold.playback.load(Ordering::Acquire) == PLAYING {
            if let Some(s) = &self.source { self.standby.start(&s.reserve, s.request.segment, &self.hold); }
        }
        if self.hold.held.load(Ordering::Acquire) {
            // The device's end of the hold is gone with the segment.
            return Arc::strong_count(&self.hold) > 1;
        }
        let at = self.hold.at.load(Ordering::Relaxed);
        if at > self.position + WINDOW {
            self.held = None;
            let cached = self.lookahead.as_mut().is_some_and(|cache| cache.skip(at));
            if cached {
                self.position = at;
            } else if at >= self.frames {
                self.position = self.frames;
            } else if let Some(opus) = self.opus.as_mut() {
                match opus.skip(at) {
                    Ok(start) => self.position = start,
                    Err(_) => { self.fail_at(at); self.position = at.min(self.frames); }
                }
            } else {
                self.position = at.min(self.frames);
            }
        }
        loop {
            let window = match self.held.take() {
                Some(window) => window,
                None => self.next(),
            };
            let end = window.is_none();
            // A late failure can reach the decoder before the original has
            // arrived. Other voices keep decoding while this one waits.
            if end && self.position < self.frames && self.opus.is_none() && self.source.is_some() && self.standby.pending() {
                return Arc::strong_count(&self.hold) > 1;
            }
            match self.windows.try_send(window) {
                Ok(()) if end => {
                    if let Some(limit) = &self.end { limit.store(self.position, Ordering::Release); }
                    return false;
                },
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
    let mut check = 0usize;
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
        for _ in 0..live.len() {
            check %= live.len();
            let job = &mut live[check];
            check += 1;
            if job.check_ahead() { break; }
        }
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
        self.hold.playing();
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
        self.hold.playing();
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
fn read_variant(variant: &Variant, decode: &Sender<Job>, recovery: Option<(Source, Option<&Segment>)>) -> Result<(Pcm, Option<Arc<AtomicUsize>>), String> {
    let bytes = Arc::new(std::fs::read(&variant.file).map_err(|e| e.to_string())?);
    let tail = bytes[bytes.len().saturating_sub(65536)..].to_vec();
    let mut opus = Opus { decoder: audio_decode::Decoder::new(0, variant.gain_db, false), channels: 0, bytes, at: 0 };
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
    let (mut job, mut ahead) = decoder_windows(rate, channels, frames);
    job.opus = Some(opus);
    job.substitute_layout = variant.surround;
    let ended = prime(&mut job, &mut ahead);
    if job.opus.is_none() || ahead.first.is_empty() { return Err("upscaled segment decode failed".into()); }
    if let Some((source, segment)) = recovery.filter(|_| !ended && job.position < frames) {
        job.covered = segment.map_or(0, |s| (reserve::frames(s) as f64 * rate / s.rate).round() as usize);
        if job.covered < frames {
            job.end = Some(Arc::new(AtomicUsize::new(usize::MAX)));
        }
        job.source = Some(source);
    }
    let end = job.end.clone();
    let pcm = handoff(job, ahead, ended, decode)?;
    static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
        eprintln!("enhanced audio: streaming {} ({channels} ch, {frames} frames)", variant.file.display());
    }
    Ok((pcm, end))
}

fn decoder_windows(rate: f64, channels: usize, frames: usize) -> (Job, Ahead) {
    let (windows, from_ahead) = sync_channel(AHEAD);
    let (spare, spares) = sync_channel(AHEAD + FIRST + 4);
    let hold = Arc::new(Hold::default());
    let job = Job { opus: None, standby: reserve::Standby::default(), original: None, source: None, covered: usize::MAX, lookahead: None, inspection: lookahead::Inspection::new(), end: None, substitute_layout: 0, surround: None,
        rate, channels, frames, windows, spare: spares, held: None, hold: hold.clone(), position: 0 };
    let ahead = Ahead { first: VecDeque::with_capacity(FIRST), windows: from_ahead, spare, current: None,
        ended: false, hold, wanted: 0, kept: Vec::with_capacity(FIRST + AHEAD + 4) };
    (job, ahead)
}

fn prime(job: &mut Job, ahead: &mut Ahead) -> bool {
    while ahead.first.len() < FIRST {
        let Some(window) = job.next() else { return true; };
        ahead.first.push_back(window);
    }
    false
}

fn handoff(job: Job, ahead: Ahead, ended: bool, decode: &Sender<Job>) -> Result<Pcm, String> {
    let (rate, channels, frames) = (job.rate, job.channels, job.frames);
    let playback = job.source.as_ref().map(|_| job.hold.clone());
    if ended {
        let _ = job.windows.try_send(None);
    } else if decode.send(job).is_err() {
        return Err("the decode-ahead thread is gone".into());
    }
    let sample = audio::voices::Sample::codec(rate, channels, frames, Box::new(ahead)).ok_or("unsupported stream")?;
    // The constructor fills the initial device ring. Only a later refill
    // or skip belongs to playback and can ask for a standby original.
    if let Some(hold) = playback { hold.playback.store(PRIMED, Ordering::Release); }
    Ok(Pcm { rate, channels, frames: sample.frames, sample })
}

fn surround_original(original: Pcm, channels: usize, decode: &Sender<Job>) -> Result<Pcm, String> {
    let rate = 48_000.0;
    let frames = (original.frames as f64 * rate / original.rate).round() as usize;
    let (mut job, mut ahead) = decoder_windows(rate, channels, frames);
    job.original = Some(original);
    job.surround = Some(surround::Surround::new(channels));
    let ended = prime(&mut job, &mut ahead);
    handoff(job, ahead, ended, decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    // An upscaled segment nobody hears for a while: its decoding holds, then
    // picks up from where playback got to, and plays the same samples there
    // as a decode of the whole file (past the codec's pre-roll).
    #[test]
    fn an_upscaled_segment_skipped_unheard_plays_on_from_where_it_got_to() {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/audio-codecs/opus-hybrid.opus");
        let bytes = std::fs::read(&file).unwrap();
        let mut whole = audio_decode::Decoder::new(0, 0., false);
        whole.push(&bytes, true).unwrap();
        let mut reference = Vec::new();
        loop {
            match whole.step(2048).unwrap() {
                audio_decode::Step::Output(n) => reference.extend_from_slice(&whole.output[..n]),
                audio_decode::Step::Done => break,
                _ => {}
            }
        }
        let (decode, decodes) = channel::<Job>();
        let ahead = std::thread::spawn(move || decode_ahead(decodes));
        let (mut pcm, _) = read_variant(&Variant::new(file, 0., "mono", None), &decode, None).unwrap();
        let sample = &mut pcm.sample;
        let play = |sample: &mut audio::voices::Sample, from: usize, to: usize| -> f32 {
            let mut worst = 0f32;
            let mut index = from;
            let started = std::time::Instant::now();
            while index < to {
                if sample.available(index) {
                    worst = worst.max((sample.value(0, index) - reference[index]).abs());
                    index += 1;
                } else {
                    assert!(started.elapsed().as_secs() < 20, "stalled at frame {index}");
                    sample.consumed(index);
                    sample.feed();
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            sample.consumed(index);
            worst
        };
        assert_eq!(play(sample, 0, 10_000), 0., "heard from the start: the whole decode's samples");
        // Unheard from 10000 to 60000, a quantum at a time.
        for at in (10_000..60_000).step_by(128) {
            sample.skip_to(at);
        }
        sample.skip_to(60_000);
        let worst = play(sample, 60_000, reference.len());
        assert!(worst < 1e-3, "heard again from frame 60000: {worst}");
        drop(pcm);
        drop(decode);
        ahead.join().unwrap();
    }

    #[test]
    fn missing_and_truncated_variants_fall_back_to_original_segment_and_remember_failure() {
        let unique = format!(
            "bfe_stream_fallback_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).unwrap();
        let orig_path = dir.join("orig.bin");
        // Minimal valid 36-byte mono Xbox IMA ADPCM block (64 frames).
        let mut adpcm = vec![0u8; 36];
        adpcm[0..2].copy_from_slice(&1200i16.to_le_bytes());
        adpcm[2] = 0;
        adpcm[3] = 0;
        std::fs::write(&orig_path, &adpcm).unwrap();

        let opus_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/audio-codecs/opus-hybrid.opus");
        let opus_bytes = std::fs::read(&opus_src).unwrap();
        let truncated_path = dir.join("cut.opus");
        std::fs::write(&truncated_path, &opus_bytes[..opus_bytes.len() / 2]).unwrap();
        let missing_path = dir.join("missing.opus");

        let segments = vec![Segment {
            file: orig_path,
            offset: 0,
            length: adpcm.len(),
            rate: 22050.0,
            channels: 1,
            substreams: 1,
            interleave: 0,
        }];
        let variants = vec![
            Variant::new(missing_path.clone(), 0., "mono", None),
            Variant::new(truncated_path, 0., "mono", None),
        ];
        let assets = Assets::new(segments, variants);

        let fetch = |variant: usize, generation: u64| -> Response {
            let req = Request {
                voice: 0,
                generation,
                segment: 0,
                part: 0,
                sample: 10,
                operation: 1,
                variant,
            };
            assert!(assets.request(req));
            let start = std::time::Instant::now();
            loop {
                if let Some(resp) = assets.receive() {
                    return resp;
                }
                assert!(start.elapsed().as_secs() < 10, "stream response timeout");
                std::thread::sleep(Duration::from_millis(1));
            }
        };

        // Missing Enhanced variant falls back immediately to original ADPCM segment.
        let r0 = fetch(0, 1);
        let p0 = r0.pcm.expect("missing variant must fall back to original ADPCM segment");
        assert_eq!(p0.rate, 22050.0);
        assert_eq!(p0.channels, 1);
        assert_eq!(p0.frames, 64);

        // Cut-short Enhanced variant also falls back immediately to original ADPCM segment.
        let r1 = fetch(1, 2);
        let p1 = r1.pcm.expect("cut-short variant must fall back to original ADPCM segment");
        assert_eq!(p1.rate, 22050.0);
        assert_eq!(p1.channels, 1);
        assert_eq!(p1.frames, 64);

        // Even if the missing Opus path later appears on disk, this battle remembers the failure
        // and continues using the original ADPCM segment instead of retrying the failed variant.
        std::fs::write(&missing_path, &opus_bytes).unwrap();
        let r2 = fetch(0, 3);
        let p2 = r2.pcm.expect("remembered failure must keep serving original ADPCM segment");
        assert_eq!(p2.rate, 22050.0);
        assert_eq!(p2.frames, 64);

        drop(assets);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod continuity;
