use super::{Hold, Original, Segment};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Weak,
    },
};

pub(super) const LIMIT: usize = 64 * 1024 * 1024;
static BYTES: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Credit(usize);

impl Credit {
    pub(super) fn claim(bytes: usize) -> Option<Self> {
        BYTES
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|&sum| sum <= LIMIT)
            })
            .ok()?;
        Some(Self(bytes))
    }
}

impl Drop for Credit {
    fn drop(&mut self) {
        BYTES.fetch_sub(self.0, Ordering::Relaxed);
    }
}

// Each complete interleave chunk belongs to one substream, in round-robin
// order. A single substream uses the entire byte range, including padding.
fn layout(s: &Segment) -> Option<(usize, usize, usize)> {
    let parts = s.substreams.max(1);
    if !(1..=12).contains(&s.channels.checked_mul(parts)?) {
        return None;
    }
    let chunk = if parts == 1 { s.length } else { s.interleave };
    Some((parts, chunk, s.length.checked_div(chunk)?))
}

pub(super) fn frames(s: &Segment) -> usize {
    layout(s).map_or(0, |(parts, chunk, chunks)| {
        (chunks.div_ceil(parts) * chunk / (36 * s.channels)).saturating_mul(64)
    })
}

fn open_range(s: &Segment) -> Option<File> {
    let mut f = File::open(&s.file).ok()?;
    if f.metadata().ok()?.len() < s.offset.checked_add(s.length as u64)? {
        return None;
    }
    f.seek(SeekFrom::Start(s.offset)).ok()?;
    Some(f)
}

fn read(s: &Segment) -> Option<(Original, Credit)> {
    let (count, chunk, chunks) = layout(s)?;
    let frames = frames(s);
    if frames == 0 {
        return None;
    }
    let credit = Credit::claim(chunk * chunks)?;
    let file = open_range(s)?;
    // Narrow interleaves share disk reads. Large chunks pass straight
    // through the reader, without a second copy of the compressed data.
    let scratch = if count > 1 && chunk < 8192 { 8192 } else { 0 };
    let _scratch = Credit::claim(scratch)?;
    let mut f = BufReader::with_capacity(scratch, file);
    // Read into the final buffers so interleaved beds never need a second
    // segment-sized allocation. The credit follows them through playback.
    let mut parts: Vec<_> = (0..count)
        .map(|i| {
            let n = chunks / count + usize::from(i < chunks % count);
            vec![0; n * chunk]
        })
        .collect();
    for i in 0..chunks {
        let offset = (i / count) * chunk;
        f.read_exact(&mut parts[i % count][offset..offset + chunk])
            .ok()?;
    }
    Some((
        Original {
            rate: s.rate,
            channels: s.channels,
            frames,
            parts,
        },
        credit,
    ))
}

pub(super) struct Work {
    segment: usize,
    playback: Weak<Hold>,
    reply: SyncSender<(Original, Credit)>,
}

pub(super) fn run(segments: Arc<Vec<Segment>>, work: Receiver<Work>) {
    for task in work {
        if task.playback.strong_count() == 0 {
            continue;
        }
        if let Some(data) = segments.get(task.segment).and_then(read) {
            let _ = task.reply.send(data);
        }
    }
}

#[derive(Default)]
pub(super) struct Standby {
    requested: bool,
    queued: Option<Work>,
    result: Option<Receiver<(Original, Credit)>>,
    original: Option<Original>,
    credit: Option<Credit>,
}

impl Standby {
    pub(super) fn start(&mut self, queue: &SyncSender<Work>, segment: usize, hold: &Arc<Hold>) {
        if !self.requested {
            self.requested = true;
            let (reply, result) = sync_channel(1);
            self.queued = Some(Work {
                segment,
                playback: Arc::downgrade(hold),
                reply,
            });
            self.result = Some(result);
        }
        if let Some(task) = self.queued.take() {
            if let Err(TrySendError::Full(task)) = queue.try_send(task) {
                self.queued = Some(task);
            }
        }
    }

    pub(super) fn prepared(&mut self) -> bool {
        if let Some(rx) = &self.result {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => return false,
                Ok((original, credit)) => {
                    self.original = Some(original);
                    self.credit = Some(credit);
                }
                Err(TryRecvError::Disconnected) => {}
            }
            self.result = None;
        }
        self.requested
    }

    pub(super) fn take(&mut self) -> Option<Original> {
        self.prepared();
        self.original.take()
    }

    pub(super) fn available(&self) -> bool {
        self.original.is_some()
    }

    pub(super) fn pending(&self) -> bool {
        !self.requested || self.result.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_substreams_have_the_same_blocks_as_a_direct_read() {
        let path =
            std::env::temp_dir().join(format!("bfe-interleaved-original-{}", std::process::id()));
        let bytes: Vec<_> = (0..2031).map(|i| (i * 13) as u8).collect();
        std::fs::write(&path, bytes).unwrap();
        for count in [1, 2, 3] {
            for interleave in [5, 37, 112] {
                let s = Segment {
                    file: path.clone(),
                    offset: 4,
                    length: 2017,
                    rate: 32000.,
                    channels: 2,
                    substreams: count,
                    interleave,
                };
                let direct = super::super::read_original(&s).unwrap().unwrap();
                let (backup, _credit) = read(&s).unwrap();
                assert_eq!(backup.frames, direct.frames);
                assert_eq!(backup.parts, direct.parts);
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}
