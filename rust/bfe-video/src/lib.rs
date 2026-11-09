//! Muted VP9 WebM movies. Container parsing and decoding stay off the UI thread.
pub mod webm;
use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn bfe_vpx_new() -> *mut c_void;
    fn bfe_vpx_free(ctx: *mut c_void);
    fn bfe_vpx_decode(
        ctx: *mut c_void,
        data: *const u8,
        len: u32,
        planes: *mut *mut u8,
        strides: *mut i32,
        size: *mut u32,
    ) -> i32;
}

/// Packed, top-down I420 planes, recycled through a bounded three-frame pool.
#[derive(Default)]
pub struct Frame {
    pub planes: [Vec<u8>; 3],
    pub width: u32,
    pub height: u32,
    pub pts: f64,
    pub duration: f64,
    pub decode_ms: f64,
}
struct Decoder(*mut c_void);
impl Decoder {
    fn new() -> Result<Self, String> {
        let ctx = unsafe { bfe_vpx_new() };
        if ctx.is_null() {
            Err("VP9 decoder initialization failed".into())
        } else {
            Ok(Self(ctx))
        }
    }
    fn decode(&mut self, data: &[u8], frame: &mut Frame) -> Result<bool, String> {
        let mut planes = [std::ptr::null_mut(); 3];
        let mut strides = [0; 3];
        let mut size = [0; 2];
        let start = Instant::now();
        let result = unsafe {
            bfe_vpx_decode(
                self.0,
                data.as_ptr(),
                u32::try_from(data.len()).map_err(|_| "VP9 packet too large")?,
                planes.as_mut_ptr(),
                strides.as_mut_ptr(),
                size.as_mut_ptr(),
            )
        };
        if result < 0 {
            return Err("Invalid VP9 frame or unsupported pixel format".into());
        }
        if result == 0 {
            return Ok(false);
        }
        if size[0] == 0 || size[1] == 0 || size[0] > 8192 || size[1] > 8192 {
            return Err("Invalid movie dimensions".into());
        }
        frame.width = size[0];
        frame.height = size[1];
        for i in 0..3 {
            let w = if i == 0 { size[0] } else { size[0].div_ceil(2) } as usize;
            let h = if i == 0 { size[1] } else { size[1].div_ceil(2) } as usize;
            if planes[i].is_null() || strides[i] < w as i32 {
                return Err("Invalid VP9 plane stride".into());
            }
            // Vec capacity survives recycling; only a resolution change allocates.
            frame.planes[i].resize(w * h, 0);
            for row in 0..h {
                let src = unsafe {
                    std::slice::from_raw_parts(planes[i].add(row * strides[i] as usize), w)
                };
                frame.planes[i][row * w..(row + 1) * w].copy_from_slice(src);
            }
        }
        frame.decode_ms = start.elapsed().as_secs_f64() * 1000.;
        Ok(true)
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { bfe_vpx_free(self.0) }
    }
}

/// Decoder lifetime is cancelled without joining on the render thread. Dropping
/// either channel wakes a worker waiting for a free buffer or a queued frame.
pub struct Movie {
    rx: Receiver<Result<Frame, String>>,
    recycle: SyncSender<Frame>,
    cancel: Arc<AtomicBool>,
}
impl Movie {
    pub fn open(path: PathBuf, looping: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(3);
        let (recycle, free) = mpsc::sync_channel(3);
        for _ in 0..3 {
            recycle.send(Frame::default()).unwrap();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        thread::spawn(move || {
            let run = || -> Result<(), String> {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let movie = webm::Webm::parse(&bytes)?;
                let mut base = 0.;
                let mut decoder = Decoder::new()?;
                let mut unused = None;
                loop {
                    for packet in &movie.packets {
                        if stop.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let mut frame = if let Some(frame) = unused.take() {
                            frame
                        } else {
                            loop {
                                match free.recv_timeout(Duration::from_millis(50)) {
                                    Ok(frame) => break frame,
                                    Err(mpsc::RecvTimeoutError::Timeout)
                                        if !stop.load(Ordering::Relaxed) =>
                                    {
                                        continue
                                    }
                                    _ => return Ok(()),
                                }
                            }
                        };
                        if decoder.decode(&bytes[packet.range.clone()], &mut frame)? {
                            frame.pts = base + packet.pts;
                            frame.duration = movie.duration;
                            if tx.send(Ok(frame)).is_err() {
                                return Ok(());
                            }
                        } else if stop.load(Ordering::Relaxed) {
                            return Ok(());
                        } else {
                            // Non-display reference packets do not consume a
                            // presentation buffer or advance its timestamp.
                            unused = Some(frame);
                        }
                    }
                    if !looping {
                        return Ok(());
                    }
                    base += movie.duration;
                }
            };
            if let Err(error) = run() {
                let _ = tx.send(Err(error));
            }
        });
        Self {
            rx,
            recycle,
            cancel,
        }
    }
    pub fn poll(&self) -> Option<Result<Frame, String>> {
        self.rx.try_recv().ok()
    }
    pub fn recycle(&self, frame: Frame) {
        let _ = self.recycle.try_send(frame);
    }
}
impl Drop for Movie {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &[u8] = include_bytes!("../../../tests/fixtures/video/pattern.webm");
    #[test]
    fn decodes_synthetic_vp9_pattern_and_reuses_planes() {
        let webm = webm::Webm::parse(SAMPLE).unwrap();
        assert_eq!(webm.packets.len(), 5);
        assert!((webm.duration - 0.5).abs() < 0.001);
        let mut decoder = Decoder::new().unwrap();
        let mut frame = Frame::default();
        let mut pointers = [std::ptr::null(); 3];
        let mut first = 0;
        for (index, packet) in webm.packets.iter().enumerate() {
            assert!(decoder
                .decode(&SAMPLE[packet.range.clone()], &mut frame)
                .unwrap());
            assert_eq!((frame.width, frame.height), (32, 24));
            assert_eq!(
                frame.planes.iter().map(Vec::len).collect::<Vec<_>>(),
                [768, 192, 192]
            );
            let ptrs = std::array::from_fn(|i| frame.planes[i].as_ptr());
            if index == 0 {
                pointers = ptrs;
                first = frame.planes[0].iter().map(|v| *v as u64).sum::<u64>();
            } else {
                assert_eq!(pointers, ptrs);
            }
        }
        assert!(first > 10000);
        assert!(decoder.decode(&[0; 32], &mut frame).is_err());
    }
    #[test]
    fn every_truncation_is_bounded() {
        for end in 0..SAMPLE.len() {
            let _ = webm::Webm::parse(&SAMPLE[..end]);
        }
    }
    #[test]
    fn prepared_movie_corpus_decodes_when_present() {
        let Some(root) = std::env::var_os("BFE_CONTENT_DIR") else {
            return;
        };
        let movies = std::path::Path::new(&root).join("common/shell/movies");
        let mut count = 0;
        for path in std::fs::read_dir(movies)
            .unwrap()
            .flatten()
            .map(|p| p.path())
            .filter(|p| p.extension().is_some_and(|e| e == "webm"))
        {
            let bytes = std::fs::read(&path).unwrap();
            let webm = webm::Webm::parse(&bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let mut decoder = Decoder::new().unwrap();
            let mut frame = Frame::default();
            for packet in webm.packets {
                assert!(decoder
                    .decode(&bytes[packet.range], &mut frame)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display())));
            }
            count += 1;
        }
        assert!(count > 0);
    }
}
