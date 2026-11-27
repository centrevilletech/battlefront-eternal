//! A 2005 hero's combat checkpoint larger than one datagram: the parts that
//! carry it, and the client's bounded reassembly, anchored to the own
//! correction it belongs to.
//!
//! Each part is its own datagram on the state lane, sent in the same tick as
//! the own correction it belongs to: `BNK1`, the epoch, the authority tick,
//! the subject (the unit the correction is of) and its life, the record kind
//! and revision, the part's index and count, the whole checkpoint's length,
//! then its share of the bytes.
//!
//! A checkpoint belongs to exactly one own correction: the same epoch, tick,
//! subject and life, of the kind and revision the match's plan uses. The
//! client commits it only together with that correction, whichever of the
//! two arrives last. Checkpoints only move forward, by epoch and then by
//! authority tick: a part older than the newest accepted correction or than
//! the checkpoint being assembled is stale and ignored, a checkpoint whose
//! correction was superseded is never committed, and a lost part costs that
//! tick's checkpoint, never a wrong one. Only 2005 matches send parts; a 2004
//! battle never does.
pub const MAGIC: u32 = u32::from_le_bytes(*b"BNK1");
/// The most parts one checkpoint may use, and the bytes each carries.
pub const MAX_PARTS: usize = 4;
pub const PART_BYTES: usize = 1040;
/// The largest checkpoint the framing carries.
pub const MAX_BYTES: usize = MAX_PARTS * PART_BYTES;
/// Header bytes before a part's share: magic, epoch, tick, subject, life,
/// kind, revision, index, count and total.
pub const HEADER: usize = 4 + 2 + 4 + 4 + 4 + 1 + 2 + 1 + 1 + 2;
const MALFORMED: &str = "invalid Battlefront II checkpoint part";

/// The own correction a checkpoint belongs to: one unit's life at one
/// authority tick of one epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub epoch: u16,
    pub tick: u32,
    pub subject: u32,
    pub life: u32,
}

impl Anchor {
    /// Whether `self` is later than `other`: a later epoch, or a later tick
    /// of the same epoch. Both wrap.
    pub fn after(&self, other: &Anchor) -> bool {
        if self.epoch != other.epoch { return self.epoch.wrapping_sub(other.epoch) < 0x8000; }
        self.tick != other.tick && self.tick.wrapping_sub(other.tick) < 0x8000_0000
    }
    fn same_time(&self, other: &Anchor) -> bool { self.epoch == other.epoch && self.tick == other.tick }
}

/// Who and what a checkpoint is: its correction, its record kind and revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub anchor: Anchor,
    pub kind: u8,
    pub revision: u16,
}

/// The parts of `payload` for `identity`, each a whole datagram.
pub fn split(identity: Identity, payload: &[u8], out: &mut Vec<Vec<u8>>) -> Result<(), &'static str> {
    out.clear();
    if payload.is_empty() || payload.len() > MAX_BYTES { return Err("Battlefront II checkpoint exceeds its parts"); }
    let count = payload.len().div_ceil(PART_BYTES);
    let Anchor { epoch, tick, subject, life } = identity.anchor;
    for (index, chunk) in payload.chunks(PART_BYTES).enumerate() {
        let mut part = Vec::with_capacity(HEADER + chunk.len());
        part.extend(MAGIC.to_le_bytes());
        part.extend(epoch.to_le_bytes());
        part.extend(tick.to_le_bytes());
        part.extend(subject.to_le_bytes());
        part.extend(life.to_le_bytes());
        part.push(identity.kind);
        part.extend(identity.revision.to_le_bytes());
        part.push(index as u8);
        part.push(count as u8);
        part.extend((payload.len() as u16).to_le_bytes());
        part.extend_from_slice(chunk);
        out.push(part);
    }
    Ok(())
}

fn header(part: &[u8]) -> Result<(Identity, usize, usize, usize, &[u8]), &'static str> {
    let (head, bytes) = part.split_at_checked(HEADER).filter(|(_, bytes)| !bytes.is_empty()).ok_or(MALFORMED)?;
    let u16_at = |at: usize| u16::from_le_bytes([head[at], head[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    if u32_at(0) != MAGIC { return Err(MALFORMED); }
    let anchor = Anchor { epoch: u16_at(4), tick: u32_at(6), subject: u32_at(10), life: u32_at(14) };
    let identity = Identity { anchor, kind: head[18], revision: u16_at(19) };
    if identity.kind == 0 || identity.revision == 0 { return Err(MALFORMED); }
    let (index, count, total) = (usize::from(head[21]), usize::from(head[22]), usize::from(u16_at(23)));
    // The count, index and total are checked before any length is derived
    // from them, so no part can make that arithmetic overflow.
    if count == 0 || count > MAX_PARTS || index >= count || total == 0 || total > count * PART_BYTES
        || total <= (count - 1) * PART_BYTES
    {
        return Err(MALFORMED);
    }
    // Every part but the last is full; the last holds the rest exactly.
    let expected = if index + 1 == count { total - index * PART_BYTES } else { PART_BYTES };
    if bytes.len() != expected { return Err(MALFORMED); }
    Ok((identity, index, count, total, bytes))
}

/// The validated identity a part names: its correction, kind and revision,
/// read from its own header, with its index, count and extent checked.
pub fn identity(part: &[u8]) -> Result<Identity, &'static str> {
    header(part).map(|(identity, ..)| identity)
}

/// One checkpoint being reassembled, in storage reserved once, and the newest
/// own correction the client accepted.
pub struct Assembly {
    anchor: Option<Anchor>,
    identity: Option<Identity>,
    count: usize,
    total: usize,
    received: u8,
    bytes: Box<[u8; MAX_BYTES]>,
    /// Unfinished checkpoints given up for a newer one.
    pub superseded: u32,
    /// Parts, and checkpoints, older than what the client already holds.
    pub stale: u32,
}

impl Default for Assembly {
    fn default() -> Self {
        Self { anchor: None, identity: None, count: 0, total: 0, received: 0, bytes: Box::new([0; MAX_BYTES]), superseded: 0, stale: 0 }
    }
}

impl Assembly {
    fn whole(&self) -> bool { self.identity.is_some() && self.received.count_ones() as usize == self.count }

    // The checkpoint committed with the accepted correction, once every part arrived.
    fn committed(&self) -> Option<Identity> {
        self.identity.filter(|identity| self.whole() && Some(identity.anchor) == self.anchor)
    }

    /// The own correction the client accepted. Corrections only move forward:
    /// an older one, or another one of the same tick, changes nothing. Returns
    /// the identity of the checkpoint committed with it when every part of
    /// that checkpoint arrived first; anything older is dropped as stale.
    pub fn accept(&mut self, anchor: Anchor) -> Option<Identity> {
        if let Some(current) = self.anchor {
            if current == anchor { return self.committed(); }
            if !anchor.after(&current) { return None; }
        }
        self.anchor = Some(anchor);
        if self.identity.is_some_and(|identity| anchor.after(&identity.anchor)) {
            if !self.whole() { self.superseded = self.superseded.saturating_add(1); }
            self.stale = self.stale.saturating_add(1);
            self.identity = None;
            self.received = 0;
        }
        self.committed()
    }

    /// Takes one part of a match whose plan uses `revision` of `kind`.
    /// Returns the identity of the checkpoint it committed together with the
    /// accepted correction; a checkpoint whose correction has not arrived
    /// waits for it. A part of another kind or revision, another unit or
    /// life at an accepted tick, or a second checkpoint for one tick is
    /// refused.
    pub fn offer(&mut self, part: &[u8], kind: u8, revision: u16) -> Result<Option<Identity>, &'static str> {
        let (identity, index, count, total, bytes) = header(part)?;
        if identity.kind != kind || identity.revision != revision { return Err(MALFORMED); }
        if let Some(anchor) = self.anchor {
            if anchor.after(&identity.anchor) { self.stale = self.stale.saturating_add(1); return Ok(None); }
            if anchor.same_time(&identity.anchor) && anchor != identity.anchor { return Err(MALFORMED); }
        }
        match self.identity {
            Some(current) if current == identity => {
                if count != self.count || total != self.total { return Err(MALFORMED); }
            }
            Some(current) if current.anchor.after(&identity.anchor) => { self.stale = self.stale.saturating_add(1); return Ok(None); }
            Some(current) if current.anchor.same_time(&identity.anchor) => return Err(MALFORMED),
            _ => {
                if self.identity.is_some() && !self.whole() { self.superseded = self.superseded.saturating_add(1); }
                self.identity = Some(identity);
                self.count = count;
                self.total = total;
                self.received = 0;
            }
        }
        if self.received & (1 << index) != 0 { return Ok(None); }
        self.bytes[index * PART_BYTES..index * PART_BYTES + bytes.len()].copy_from_slice(bytes);
        self.received |= 1 << index;
        Ok(self.committed())
    }

    /// The checkpoint committed with the accepted correction: its identity and bytes.
    pub fn complete(&self) -> Option<(Identity, &[u8])> {
        self.committed().map(|identity| (identity, &self.bytes[..self.total]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(epoch: u16, tick: u32, subject: u32, life: u32) -> Anchor { Anchor { epoch, tick, subject, life } }
    fn id(tick: u32, life: u32) -> Identity { Identity { anchor: at(1, tick, 7, life), kind: super::super::revisions::COMBAT, revision: 1 } }
    fn payload(n: usize, salt: u8) -> Vec<u8> { (0..n).map(|i| (i as u8).wrapping_mul(7).wrapping_add(salt)).collect() }
    fn offer(assembly: &mut Assembly, part: &[u8]) -> Result<Option<Identity>, &'static str> {
        assembly.offer(part, super::super::revisions::COMBAT, 1)
    }

    #[test]
    fn a_checkpoint_commits_only_whole_and_with_its_correction() {
        let mut parts = Vec::new();
        let full = payload(MAX_BYTES, 3);
        split(id(30, 2), &full, &mut parts).unwrap();
        assert_eq!(parts.len(), MAX_PARTS);
        assert!(parts.iter().all(|p| p.len() <= 1100));
        let mut assembly = Assembly::default();
        for &n in &[2, 0, 3] { assert_eq!(offer(&mut assembly, &parts[n]).unwrap(), None); }
        // A duplicate changes nothing.
        assert_eq!(offer(&mut assembly, &parts[0]).unwrap(), None);
        // Whole, but its correction has not been accepted: nothing commits.
        assert_eq!(offer(&mut assembly, &parts[1]).unwrap(), None);
        assert!(assembly.complete().is_none());
        assert_eq!(assembly.accept(id(30, 2).anchor), Some(id(30, 2)));
        assert_eq!(assembly.complete(), Some((id(30, 2), &full[..])));
        // The other order: the correction first, then the last part commits.
        split(id(33, 2), &payload(170, 9), &mut parts).unwrap();
        assert_eq!(assembly.accept(id(33, 2).anchor), None);
        assert_eq!((parts.len(), offer(&mut assembly, &parts[0]).unwrap()), (1, Some(id(33, 2))));
    }

    #[test]
    fn a_lost_part_costs_its_tick_and_a_newer_tick_replaces_it() {
        let mut assembly = Assembly::default();
        let (mut old, mut new) = (Vec::new(), Vec::new());
        split(id(60, 4), &payload(2500, 1), &mut old).unwrap();
        split(id(63, 4), &payload(2400, 2), &mut new).unwrap();
        assembly.accept(id(60, 4).anchor);
        offer(&mut assembly, &old[0]).unwrap();
        offer(&mut assembly, &old[1]).unwrap();
        // The third part of tick 60 is lost; tick 63 arrives and supersedes it.
        for part in &new[..2] { assert_eq!(offer(&mut assembly, part).unwrap(), None); }
        assert_eq!(assembly.superseded, 1);
        // The late part of tick 60 is ignored rather than completing it.
        assert_eq!(offer(&mut assembly, &old[2]).unwrap(), None);
        assert_eq!(offer(&mut assembly, &new[2]).unwrap(), None);
        assert_eq!(assembly.accept(id(63, 4).anchor), Some(id(63, 4)));
    }

    // Delayed parts: a whole checkpoint of an older life, tick or epoch never
    // commits after a newer correction was accepted, whatever order its parts
    // arrive in.
    #[test]
    fn delayed_checkpoints_of_older_lives_and_epochs_never_commit() {
        let mut parts = Vec::new();
        let older = [Identity { anchor: at(6, 198, 7, 4), ..id(0, 0) }, Identity { anchor: at(6, 205, 7, 4), ..id(0, 0) }];
        let mut assembly = Assembly::default();
        assert_eq!(assembly.accept(at(7, 201, 9, 5)), None);
        for identity in older {
            split(identity, &payload(1500, 1), &mut parts).unwrap();
            for part in &parts { assert_eq!(offer(&mut assembly, part).unwrap(), None); }
        }
        assert_eq!(assembly.stale, 4);
        // The same epoch: life 4 at tick 198 after life 5 at tick 201.
        let mut assembly = Assembly::default();
        assembly.accept(at(7, 201, 9, 5));
        split(Identity { anchor: at(7, 198, 9, 4), ..id(0, 0) }, &payload(1500, 2), &mut parts).unwrap();
        for part in &parts { assert_eq!(offer(&mut assembly, part).unwrap(), None); }
        // A whole checkpoint waiting for its correction is dropped when a
        // later correction is accepted instead: a stale completion.
        split(Identity { anchor: at(7, 204, 9, 5), ..id(0, 0) }, &payload(900, 3), &mut parts).unwrap();
        assert_eq!(offer(&mut assembly, &parts[0]).unwrap(), None);
        assert_eq!(assembly.accept(at(7, 207, 9, 5)), None);
        assert!(assembly.complete().is_none());
        assert_eq!(assembly.accept(at(7, 204, 9, 5)), None);
        // An older correction never moves the anchor back.
        assert_eq!(offer(&mut assembly, &parts[0]).unwrap(), None);
        assert!(assembly.complete().is_none());
    }

    #[test]
    fn malformed_parts_are_refused() {
        let mut parts = Vec::new();
        assert!(split(id(1, 1), &payload(MAX_BYTES + 1, 0), &mut parts).is_err());
        assert!(split(id(1, 1), &[], &mut parts).is_err());
        split(id(1, 1), &payload(1500, 0), &mut parts).unwrap();
        let mut assembly = Assembly::default();
        // The last of two parts claiming a total below its own offset.
        let short_total = { let mut p = parts[1].clone(); p[23..25].copy_from_slice(&100u16.to_le_bytes()); p };
        for bad in [parts[0][..HEADER].to_vec(), [&parts[0][..], &[0]].concat(), { let mut p = parts[1].clone(); p[22] = 9; p },
            { let mut p = parts[0].clone(); p[0] ^= 1; p }, { let mut p = parts[1].clone(); p[21] = 2; p }, short_total]
        {
            assert!(offer(&mut assembly, &bad).is_err(), "{:?}", &bad[..HEADER.min(bad.len())]);
        }
        // Another kind or revision than the match's.
        assert!(assembly.offer(&parts[0], super::super::revisions::COMBAT, 2).is_err());
        assert!(assembly.offer(&parts[0], super::super::revisions::COMBAT_DISPLAY, 1).is_err());
        // The same tick cannot change its part count midway.
        offer(&mut assembly, &parts[0]).unwrap();
        let mut other = Vec::new();
        split(id(1, 1), &payload(2500, 0), &mut other).unwrap();
        assert!(offer(&mut assembly, &other[1]).is_err());
        // Nor carry a second checkpoint of another unit or life.
        split(Identity { anchor: at(1, 1, 8, 1), ..id(1, 1) }, &payload(20, 0), &mut other).unwrap();
        assert!(offer(&mut assembly, &other[0]).is_err());
        // A part of another unit at the accepted correction's tick is refused.
        let mut assembly = Assembly::default();
        assembly.accept(id(1, 1).anchor);
        assert!(offer(&mut assembly, &other[0]).is_err());
    }
}
