//! Reader for compiled Lua script (`scr_`) chunks inside PC `.lvl` (`ucfb`) archives.
//!
//! Layout observed in PC Battlefront II (2005) archives (`mission.lvl`, `common.lvl`,
//! `ingame.lvl`, `shell.lvl`):
//! - Top-level `ucfb` container holding either direct `scr_` children or nested `lvl_`
//!   sub-archives (each `lvl_` payload begins with an 8-byte name-hash + size header
//!   followed by child chunks).
//! - Each `scr_` chunk contains child chunks `NAME` (NUL-terminated Latin-1 string),
//!   `INFO` (body count byte, 1 in retail archives), and `BODY` (compiled Lua 5.0 chunk
//!   with one trailing `0x00` byte).

use crate::error::ArchiveError;

/// Safety limits enforced while scanning a `.lvl` archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum raw archive byte length accepted.
    pub max_input_bytes: usize,
    /// Maximum nested `ucfb` / `lvl_` container depth.
    pub max_depth: usize,
    /// Maximum cumulative chunks inspected across the entire archive.
    pub max_chunks: usize,
    /// Maximum cumulative `scr_` scripts extracted from the archive.
    pub max_scripts: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * 1024 * 1024,
            max_depth: 32,
            max_chunks: 200_000,
            max_scripts: 16_384,
        }
    }
}

/// A single compiled script entry found inside a `.lvl` archive.
#[derive(Debug, Clone)]
pub struct ArchiveScript<'a> {
    /// Script name from the `NAME` child chunk.
    pub name: String,
    /// Raw compiled bytecode payload from the `BODY` child chunk.
    pub body: &'a [u8],
}

/// Parsed index of `scr_` entries in a `.lvl` archive, preserving archive order.
#[derive(Debug, Clone)]
pub struct ScriptArchive<'a> {
    scripts: Vec<ArchiveScript<'a>>,
}

impl<'a> ScriptArchive<'a> {
    /// Scan a `.lvl` archive buffer for all `scr_` chunks using default [`ArchiveLimits`].
    pub fn parse(data: &'a [u8]) -> Result<Self, ArchiveError> {
        Self::parse_with_limits(data, ArchiveLimits::default())
    }

    /// Scan a `.lvl` archive buffer under explicit [`ArchiveLimits`] using an
    /// iterative traversal stack (no recursion).
    pub fn parse_with_limits(
        data: &'a [u8],
        limits: ArchiveLimits,
    ) -> Result<Self, ArchiveError> {
        if data.len() > limits.max_input_bytes {
            return Err(ArchiveError::InputTooLarge {
                size: data.len(),
                limit: limits.max_input_bytes,
            });
        }
        if data.len() < 8 || &data[0..4] != b"ucfb" {
            return Err(ArchiveError::InvalidHeader);
        }

        let mut chunks_seen = 0usize;
        let top = iter_chunks(data, 0, data.len(), &mut chunks_seen, limits.max_chunks)?;
        let Some((tag, body_start, body_end)) = top.into_iter().next() else {
            return Err(ArchiveError::InvalidHeader);
        };
        if tag != *b"ucfb" {
            return Err(ArchiveError::InvalidHeader);
        }

        let mut scripts = Vec::new();
        // Explicit traversal stack of `(start, end, depth)` ranges. Pushing child
        // containers in reverse order preserves exact pre-order archive ordering.
        let mut stack: Vec<(usize, usize, usize)> = vec![(body_start, body_end, 0)];
        while let Some((start, end, depth)) = stack.pop() {
            let children = iter_chunks(data, start, end, &mut chunks_seen, limits.max_chunks)?;
            let mut subcontainers: Vec<(usize, usize, usize)> = Vec::new();
            // To preserve exact depth-first archive order when a container mixes
            // direct `scr_` entries and nested `lvl_` containers, walk items in
            // reverse onto a work stack of actions.
            // Fast path: if there are no nested containers, handle `scr_` directly.
            let has_nested = children
                .iter()
                .any(|(t, _, _)| t == b"ucfb" || t == b"lvl_");
            if !has_nested {
                for (ctag, b, e) in children {
                    if &ctag == b"scr_" {
                        let script =
                            parse_script_chunk(data, b, e, &mut chunks_seen, limits.max_chunks)?;
                        if scripts.len() >= limits.max_scripts {
                            return Err(ArchiveError::ChunkLimit {
                                limit: limits.max_scripts,
                            });
                        }
                        scripts.push(script);
                    }
                }
                continue;
            }

            // Mixed container: keep exact archive order by pushing each item onto
            // a local work sequence or descending cleanly.
            for (ctag, b, e) in children.into_iter().rev() {
                match &ctag {
                    b"ucfb" => {
                        let next_depth = depth + 1;
                        if next_depth > limits.max_depth {
                            return Err(ArchiveError::DepthLimit {
                                limit: limits.max_depth,
                            });
                        }
                        subcontainers.push((b, e, next_depth));
                    }
                    b"lvl_" => {
                        let next_depth = depth + 1;
                        if next_depth > limits.max_depth {
                            return Err(ArchiveError::DepthLimit {
                                limit: limits.max_depth,
                            });
                        }
                        // Sub-archive payload starts with an 8-byte header (name hash + size).
                        if e.saturating_sub(b) > 8 {
                            subcontainers.push((b + 8, e, next_depth));
                        }
                    }
                    b"scr_" => {
                        // Wrap the single `scr_` span (with header) as a depth-preserving
                        // leaf container so it pops in exact original order.
                        subcontainers.push((b - 8, e, depth));
                    }
                    _ => {}
                }
            }
            stack.extend(subcontainers);
        }

        Ok(Self { scripts })
    }

    /// All scripts in archive order.
    pub fn scripts(&self) -> &[ArchiveScript<'a>] {
        &self.scripts
    }

    /// Look up a script by name (ASCII case-insensitive, matching `ScriptCB_DoFile`).
    pub fn get(&self, name: &str) -> Option<&ArchiveScript<'a>> {
        self.scripts
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
}

fn parse_script_chunk<'a>(
    data: &'a [u8],
    b: usize,
    e: usize,
    chunks_seen: &mut usize,
    max_chunks: usize,
) -> Result<ArchiveScript<'a>, ArchiveError> {
    let mut name: Option<String> = None;
    let mut body: Option<&'a [u8]> = None;
    for (ctag, cb, ce) in iter_chunks(data, b, e, chunks_seen, max_chunks)? {
        match &ctag {
            b"NAME" => {
                let slice = &data[cb..ce];
                let nul = slice.iter().position(|&x| x == 0).unwrap_or(slice.len());
                name = Some(slice[..nul].iter().map(|&x| x as char).collect());
            }
            b"BODY" => {
                body = Some(&data[cb..ce]);
            }
            _ => {}
        }
    }
    let name = name.ok_or_else(|| ArchiveError::MissingChild {
        script: "<unnamed>".to_string(),
        child: "NAME",
    })?;
    let body = body.ok_or_else(|| ArchiveError::MissingChild {
        script: name.clone(),
        child: "BODY",
    })?;
    Ok(ArchiveScript { name, body })
}

#[inline]
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn iter_chunks(
    data: &[u8],
    start: usize,
    end: usize,
    chunks_seen: &mut usize,
    max_chunks: usize,
) -> Result<Vec<([u8; 4], usize, usize)>, ArchiveError> {
    let mut pos = start;
    let mut out = Vec::new();
    while pos + 8 <= end {
        let tag = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
        if tag == [0, 0, 0, 0] {
            // Trailing zero padding at the end of a container is valid; interior
            // zero words followed by non-zero bytes indicate malformed structure.
            if data[pos..end].iter().all(|&b| b == 0) {
                return Ok(out);
            }
            return Err(ArchiveError::MalformedChunk { offset: pos });
        }
        if !tag.iter().all(|&b| (32..127).contains(&b)) {
            return Err(ArchiveError::MalformedChunk { offset: pos });
        }
        *chunks_seen = chunks_seen.saturating_add(1);
        if *chunks_seen > max_chunks {
            return Err(ArchiveError::ChunkLimit { limit: max_chunks });
        }
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let body = pos + 8;
        let Some(raw_end) = body.checked_add(size) else {
            return Err(ArchiveError::TruncatedChunk { offset: pos });
        };
        if raw_end > end {
            return Err(ArchiveError::TruncatedChunk { offset: pos });
        }
        out.push((tag, body, raw_end));
        let aligned = align4(raw_end);
        // Padding bytes between raw_end and aligned (up to 3 bytes) must be inside end.
        pos = aligned.min(end);
    }
    // Any remaining trailing bytes (< 8) must be zero padding, not truncated/corrupt data.
    if pos < end && data[pos..end].iter().any(|&b| b != 0) {
        return Err(ArchiveError::MalformedChunk { offset: pos });
    }
    Ok(out)
}
