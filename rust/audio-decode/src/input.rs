//! Bounded compressed input. A decoder consumes complete frames/pages, so
//! network reads can stop at any byte without discarding parser state.
// The budget covers unread compressed data, not track length. A corrupt
// frame or packet cannot turn streaming into unbounded whole-file storage.
pub const INPUT_LIMIT: usize = 2 * 1024 * 1024;
#[derive(Default)]
pub struct Input {
    data: Vec<u8>,
    // Consumed bytes remain below this cursor until a batch compaction. The
    // parser borrows bytes only during a call, never across push or consume.
    at: usize,
    pub eof: bool,
}
impl Input {
    pub fn bytes(&self) -> &[u8] {
        &self.data[self.at..]
    }
    // Only complete, validated parser spans advance this cursor. Partial
    // headers remain intact for the next network chunk.
    pub fn consume(&mut self, count: usize) {
        self.at += count;
    }
    // This runs on the loading worker, where extending and compacting a Vec
    // is allowed. No shared memory or audio-thread lock protects this buffer;
    // the worker serializes every call on a decoder handle.
    pub fn push(&mut self, bytes: &[u8], eof: bool) -> Result<(), String> {
        if self.bytes().len() + bytes.len() > INPUT_LIMIT {
            return Err("compressed frame exceeds the input budget".into());
        }
        // Compact in read-sized batches to avoid shifting a window per frame.
        // Count unread bytes for the limit so already-consumed metadata cannot
        // artificially exhaust the next frame's budget.
        if self.at > 0 && (self.at >= 65536 || self.data.len() + bytes.len() > INPUT_LIMIT) {
            self.data.drain(..self.at);
            self.at = 0;
        }
        self.data.extend_from_slice(bytes);
        self.eof |= eof;
        Ok(())
    }
    // An incomplete structure is recoverable until EOF. The same condition
    // at EOF is a truncated asset, not clean codec completion.
    pub fn short(&self) -> Result<(), String> {
        if self.eof {
            Err("truncated audio asset".into())
        } else {
            Ok(())
        }
    }
}
