//! Unequal tracks can reveal their actual end ahead of the device. Keep
//! the decoder's PCM until playback consumes it, within the reserve budget.
use super::{reserve::Credit, WINDOW};

pub(super) enum Inspection {
    Page { offset: usize, sequence: u32 },
    Damaged(usize),
    Finished,
}

impl Inspection {
    pub(super) fn new() -> Self {
        Self::Page {
            offset: 0,
            sequence: 0,
        }
    }

    /// Only a confirmed bad page warrants decoding past the normal rings.
    /// Healthy playback keeps its ordinary Opus state for a later seek.
    pub(super) fn check(&mut self, bytes: &[u8]) -> bool {
        let Self::Page { offset, sequence } = *self else {
            return false;
        };
        if !bytes.starts_with(b"OggS") {
            *self = Self::Finished;
            return false;
        }
        let Some(page) = audio_decode::verified_ogg_page(&bytes[offset..]) else {
            *self = Self::Damaged(offset);
            return true;
        };
        *self = if page[14..18] != bytes[14..18] || page[18..22] != sequence.to_le_bytes() {
            Self::Damaged(offset)
        } else if page[5] & 4 != 0 {
            Self::Finished
        } else {
            Self::Page {
                offset: offset + page.len(),
                sequence: sequence.wrapping_add(1),
            }
        };
        true
    }
}

pub(super) struct Lookahead {
    data: Vec<f32>,
    _credit: Credit,
    channels: usize,
    capacity: usize,
    begin: usize,
    end: usize,
}

impl Lookahead {
    pub(super) fn new(at: usize, frames: usize, channels: usize) -> Option<Self> {
        // At most eight MiB per voice. A long track is checked progressively.
        let bytes_per_frame = channels * size_of::<f32>();
        let capacity = (frames - at).min((8 << 20) / bytes_per_frame);
        let credit = Credit::claim(capacity * bytes_per_frame)?;
        Some(Self {
            data: vec![0.; capacity * channels],
            _credit: credit,
            channels,
            capacity,
            begin: at,
            end: at,
        })
    }

    pub(super) fn next(&mut self, position: usize, frames: usize) -> Option<usize> {
        if self.begin == self.end {
            self.begin = position;
            self.end = position;
        }
        let count = WINDOW.min(frames.saturating_sub(self.end));
        (count > 0 && count <= self.capacity - (self.end - self.begin)).then_some(self.end)
    }

    pub(super) fn append(&mut self, pcm: &[f32], stride: usize, count: usize) {
        let offset = self.end % self.capacity;
        let first = count.min(self.capacity - offset);
        for (src, dst) in pcm
            .chunks_exact(stride)
            .zip(self.data.chunks_exact_mut(self.capacity))
        {
            dst[offset..offset + first].copy_from_slice(&src[..first]);
            dst[..count - first].copy_from_slice(&src[first..count]);
        }
        self.end += count;
    }

    pub(super) fn skip(&mut self, at: usize) -> bool {
        self.begin = at.clamp(self.begin, self.end);
        at < self.end
    }

    pub(super) fn take(&mut self, at: usize, out: &mut Vec<f32>) -> usize {
        if !(self.begin..self.end).contains(&at) {
            return 0;
        }
        let count = WINDOW.min(self.end - at);
        let offset = at % self.capacity;
        let first = count.min(self.capacity - offset);
        out.reserve(count * self.channels);
        for src in self.data.chunks_exact(self.capacity) {
            out.extend_from_slice(&src[offset..offset + first]);
            out.extend_from_slice(&src[..count - first]);
        }
        self.begin = at + count;
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_planes_keep_their_frame_numbers_across_wrap_and_skip() {
        let start = WINDOW - 7;
        let capacity = WINDOW * 2;
        let mut cache = Lookahead::new(start, start + capacity, 2).unwrap();
        let mut out = Vec::new();
        for n in 0..3 {
            let at = start + n * WINDOW;
            assert_eq!(cache.next(at, start + 3 * WINDOW), Some(at));
            let data: Vec<_> = (0..2)
                .flat_map(|ch| (at..at + WINDOW).map(move |i| (i + ch * 100_000) as f32))
                .collect();
            cache.append(&data, WINDOW, WINDOW);
            if n == 1 {
                assert_eq!(cache.next(start, start + 3 * WINDOW), None);
                assert!(cache.skip(start + WINDOW));
                assert_eq!(cache.take(start + WINDOW, &mut out), WINDOW);
                assert_eq!(out, data);
                out.clear();
            }
            if n == 2 {
                assert_eq!(cache.take(at, &mut out), WINDOW);
                assert_eq!(out, data);
            }
        }
    }
}
