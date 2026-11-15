//! The front end's movies: the Bink files inside the player's own game archives, read in place and decoded
//! on a worker thread by the bink-decoder crate. Nothing is converted or copied.
pub mod archive;
pub mod library;

pub use library::{MovieLibrary, Source};

use bink_decoder::Decoder;
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

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

/// Decoder lifetime is cancelled without joining on the render thread. Dropping
/// either channel wakes a worker waiting for a free buffer or a queued frame.
pub struct Movie {
    rx: Receiver<Result<Frame, String>>,
    recycle: SyncSender<Frame>,
    cancel: Arc<AtomicBool>,
}

fn open_decoder(source: &Source) -> Result<Decoder<File>, String> {
    let mut file =
        File::open(&source.path).map_err(|e| format!("{}: {e}", source.path.display()))?;
    file.seek(SeekFrom::Start(source.offset))
        .map_err(|e| format!("{}: {e}", source.path.display()))?;
    let mut movie = Decoder::new(file).map_err(|e| e.to_string())?;
    // The front end plays its movies without sound, as the Electron build does.
    movie.disable_all_audio();
    Ok(movie)
}

impl Movie {
    /// Starts decoding `source` (None: the movie is not in the game's archives, which fails at once).
    pub fn open(source: Option<Source>, looping: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(3);
        let (recycle, free) = mpsc::sync_channel(3);
        for _ in 0..3 {
            recycle.send(Frame::default()).unwrap();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        thread::spawn(move || {
            let run = || -> Result<(), String> {
                let source = source.ok_or("Movie is not in the game's archives")?;
                let mut movie = open_decoder(&source)?;
                let duration = movie.header().duration();
                let mut base = 0.;
                let mut unused = None;
                loop {
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
                    let start = Instant::now();
                    match movie.next_frame().map_err(|e| e.to_string())? {
                        Some(decoded) => {
                            decoded.picture.copy_i420(&mut frame.planes);
                            frame.width = decoded.picture.width;
                            frame.height = decoded.picture.height;
                            frame.pts = base + decoded.time;
                            frame.duration = duration;
                            frame.decode_ms = start.elapsed().as_secs_f64() * 1000.;
                            if tx.send(Ok(frame)).is_err() {
                                return Ok(());
                            }
                        }
                        None if looping => {
                            // The only keyframe is the first: a loop's last frame runs into it.
                            movie.rewind();
                            base += duration;
                            unused = Some(frame);
                        }
                        None => return Ok(()),
                    }
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
    pub(crate) const PATTERN: &[u8] = include_bytes!("../../../tests/fixtures/video/pattern.bik");

    fn fixture() -> (std::path::PathBuf, Source) {
        let dir = std::env::temp_dir().join(format!(
            "bfe-video-movie-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("archive.bin");
        // Stored in place after unrelated bytes, as inside an archive.
        let mut bytes = vec![0xee; 300];
        bytes.extend(PATTERN);
        std::fs::write(&path, bytes).unwrap();
        (dir, Source { path, offset: 300 })
    }

    fn next(movie: &Movie) -> Frame {
        for _ in 0..2000 {
            match movie.poll() {
                Some(Ok(frame)) => return frame,
                Some(Err(e)) => panic!("{e}"),
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
        panic!("no frame");
    }

    #[test]
    fn plays_the_synthetic_movie_in_place_and_loops() {
        let (dir, source) = fixture();
        let movie = Movie::open(Some(source), true);
        let mut pointers = None;
        for n in 0..12 {
            let frame = next(&movie);
            let k = n % 5;
            assert_eq!((frame.width, frame.height), (32, 24));
            assert_eq!(frame.planes[0], vec![20 + 40 * k as u8; 32 * 24]);
            assert_eq!(frame.planes[1], vec![60 + 10 * k as u8; 16 * 12]);
            assert_eq!(frame.planes[2], vec![200 - 20 * k as u8; 16 * 12]);
            assert!(
                (frame.pts - n as f64 * 0.1).abs() < 1e-9,
                "{n}: {}",
                frame.pts
            );
            assert!((frame.duration - 0.5).abs() < 1e-9);
            let p: [*const u8; 3] = std::array::from_fn(|i| frame.planes[i].as_ptr());
            pointers.get_or_insert(Vec::new()).push(p);
            movie.recycle(frame);
        }
        // Three buffers circulate; none is reallocated after the first round.
        let seen = pointers.unwrap();
        assert!(seen[3..].iter().all(|p| seen[..3].contains(p)));
        drop(movie);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_movie_fails_at_once() {
        let movie = Movie::open(None, true);
        for _ in 0..2000 {
            if let Some(result) = movie.poll() {
                assert!(result.is_err());
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("no error");
    }

    #[test]
    fn a_non_looping_movie_ends() {
        let (dir, source) = fixture();
        let movie = Movie::open(Some(source), false);
        for _ in 0..5 {
            let frame = next(&movie);
            movie.recycle(frame);
        }
        thread::sleep(Duration::from_millis(50));
        assert!(movie.poll().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn real_movies_decode_when_the_game_is_there() {
        // Local check: BFE_GAME_DIR names a game install; every movie of its archives decodes.
        let Some(game) = std::env::var_os("BFE_GAME_DIR") else {
            return;
        };
        let library = MovieLibrary::open(None, &[game.into()]);
        assert!(library.archive_count() > 0, "no movie archive found");
        let mut count = 0;
        for source in library.sources() {
            let mut movie = open_decoder(source).unwrap();
            let frames = movie.header().frame_count;
            let mut decoded = 0;
            while let Some(frame) = movie.next_frame().unwrap() {
                decoded += 1;
                let _ = frame.picture;
            }
            assert_eq!(decoded, frames);
            count += 1;
        }
        eprintln!("{count} movies decoded");
        assert!(count >= 55);
    }
}
