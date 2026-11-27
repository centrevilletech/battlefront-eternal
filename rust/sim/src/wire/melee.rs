//! A Battlefront II hero's combat display as a world field: the melee pose of a saber
//! wielder (what its swing, guard and guard reaction show), the pose of a power it casts
//! or holds, and the choke that holds it. The fields are constant within a combo state
//! and within a cast (clip clocks travel as the tick at which they read zero), so the
//! display is sent again only when one of its parts changes (for a swing, also when the
//! state turns it to the stick). A Battlefront (2004) battle
//! has no infantry profile and never writes this schema.
use crate::combat::combo::{clip_playback_time, LoopMode};
use crate::combat::powers::{EffectTable, Powers};

/// World field schema of a hero's combat display: the 2005 record kind 5 as a shared
/// world field, `0x0200 + kind`, its payload led by the record's revision. Its id is
/// the soldier's own, `ID_BASE` plus the unit slot.
pub const SCHEMA: f64 = 517.;
pub const ID_BASE: f64 = 100000.;
/// Revision 2 adds the cast and choke parts and makes the melee pose optional.
pub const REVISION: u16 = 2;
/// Encoded melee pose: state, clock origin, loop, clip length, blade mask, reaction,
/// swing turn.
pub const MELEE_LEN: usize = 34;
/// Encoded cast pose: weapon class hash, fire slot, clock origin.
pub const CAST_LEN: usize = 13;
/// Encoded choke: the clip the holding power names.
pub const CHOKE_LEN: usize = 4;
/// Longest display: revision, part flags and all three parts.
pub const MAX_LEN: usize = 3 + MELEE_LEN + CAST_LEN + CHOKE_LEN;
const NO_REACTION: u16 = u16::MAX;
/// Longest clip or reaction a pose may name, in seconds.
const MAX_CLIP: f64 = 600.;
const HAS_MELEE: u8 = 1;
const HAS_CAST: u8 = 2;
const CAST_HELD: u8 = 4;
const CHOKED: u8 = 8;
/// Highest fire slot a cast pose names (fire, fire 2, throw).
const MAX_FIRE_SLOT: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeleePose {
    /// Index of the active state in the class's combo machine.
    pub state: u16,
    /// Battle tick at which the state's clip clock read zero.
    pub origin: f64,
    pub loop_mode: LoopMode,
    /// Length of the state's clip in seconds (0 for a state without one).
    pub clip_len: f64,
    /// Bit `i` set hides blade edge `i`.
    pub blades_off: u8,
    /// Guard reaction: its clip, the battle tick it began at and its length in seconds.
    pub reaction: Option<(u16, f64, f64)>,
    /// Yaw in radians the combo state turns the swing and the drawn body by, relative
    /// to the aim heading.
    pub swing_turn: f64,
}

/// The pose of a power its soldier casts or holds: the offhand weapon's fire clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CastPose {
    /// Case-folded FNV-1a of the power's weapon class.
    pub weapon: u32,
    /// The weapon's fire slot: 0 fire, 1 fire 2, 2 throw.
    pub fire_anim: u8,
    /// Battle tick at which the cast pose's clock read zero.
    pub origin: f64,
    /// The power is held (a choke's hold, a held repeating cast): the clip keeps its last frame.
    pub holding: bool,
}

/// What a remote viewer draws of a hero's combat: any of its three parts may be absent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Display {
    pub melee: Option<MeleePose>,
    pub cast: Option<CastPose>,
    /// Held by a choke: the clip its power names (0: the default choking clip).
    pub choked: Option<u32>,
}

fn f64_at(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn f32_at(bytes: &[u8], at: usize) -> f64 {
    f64::from(f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()))
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

impl MeleePose {
    fn encode_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.state.to_le_bytes());
        out.extend_from_slice(&self.origin.to_le_bytes());
        out.push(match self.loop_mode {
            LoopMode::None => 0,
            LoopMode::Wrap => 1,
            LoopMode::FinalFrame => 2,
        });
        out.extend_from_slice(&(self.clip_len as f32).to_le_bytes());
        out.push(self.blades_off);
        let (clip, start, len) = self.reaction.unwrap_or((NO_REACTION, 0., 0.));
        out.extend_from_slice(&clip.to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&(len as f32).to_le_bytes());
        out.extend_from_slice(&(self.swing_turn as f32).to_le_bytes());
    }

    fn decode_body(bytes: &[u8]) -> Option<Self> {
        let length = |v: f64| (v.is_finite() && (0. ..=MAX_CLIP).contains(&v)).then_some(v);
        let origin = Some(f64_at(bytes, 2)).filter(|v| v.is_finite())?;
        let loop_mode = match bytes[10] {
            0 => LoopMode::None,
            1 => LoopMode::Wrap,
            2 => LoopMode::FinalFrame,
            _ => return None,
        };
        let clip = u16_at(bytes, 16);
        let reaction = if clip == NO_REACTION {
            None
        } else {
            Some((clip, Some(f64_at(bytes, 18)).filter(|v| v.is_finite())?, length(f32_at(bytes, 26))?))
        };
        Some(Self {
            state: u16_at(bytes, 0),
            origin,
            loop_mode,
            clip_len: length(f32_at(bytes, 11))?,
            blades_off: bytes[15],
            reaction,
            swing_turn: Some(f32_at(bytes, 30)).filter(|v| v.abs() <= std::f64::consts::PI)?,
        })
    }

    /// The clip time the battle itself shows at `tick`.
    pub fn anim_time(&self, tick: f64) -> f64 {
        let clock = ((tick - self.origin) / 60.).max(0.);
        clip_playback_time(clock, self.clip_len, self.loop_mode)
    }

    /// The guard reaction's clip and time at `tick`, while it plays.
    pub fn reaction_at(&self, tick: f64) -> Option<(usize, f64)> {
        let (clip, start, len) = self.reaction?;
        let time = (tick - start) / 60.;
        (time >= 0. && time < len).then_some((usize::from(clip), time))
    }
}

impl CastPose {
    /// Seconds since the cast pose began, at `tick`.
    pub fn elapsed(&self, tick: f64) -> f64 {
        ((tick - self.origin) / 60.).max(0.)
    }
}

impl Display {
    pub fn is_empty(&self) -> bool {
        self.melee.is_none() && self.cast.is_none() && self.choked.is_none()
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(&REVISION.to_le_bytes());
        let flags = u8::from(self.melee.is_some()) * HAS_MELEE
            | u8::from(self.cast.is_some()) * HAS_CAST
            | u8::from(self.cast.is_some_and(|c| c.holding)) * CAST_HELD
            | u8::from(self.choked.is_some()) * CHOKED;
        out.push(flags);
        if let Some(melee) = &self.melee {
            melee.encode_body(out);
        }
        if let Some(cast) = &self.cast {
            out.extend_from_slice(&cast.weapon.to_le_bytes());
            out.push(cast.fire_anim);
            out.extend_from_slice(&cast.origin.to_le_bytes());
        }
        if let Some(anim) = self.choked {
            out.extend_from_slice(&anim.to_le_bytes());
        }
    }

    /// A received display, or `None` for bytes no producer writes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 3 || bytes[..2] != REVISION.to_le_bytes() {
            return None;
        }
        let flags = bytes[2];
        if flags & !(HAS_MELEE | HAS_CAST | CAST_HELD | CHOKED) != 0 || (flags & CAST_HELD != 0 && flags & HAS_CAST == 0) {
            return None;
        }
        let len = 3
            + usize::from(flags & HAS_MELEE != 0) * MELEE_LEN
            + usize::from(flags & HAS_CAST != 0) * CAST_LEN
            + usize::from(flags & CHOKED != 0) * CHOKE_LEN;
        if bytes.len() != len {
            return None;
        }
        let mut at = 3;
        let mut out = Self::default();
        if flags & HAS_MELEE != 0 {
            out.melee = Some(MeleePose::decode_body(&bytes[at..at + MELEE_LEN])?);
            at += MELEE_LEN;
        }
        if flags & HAS_CAST != 0 {
            let fire_anim = bytes[at + 4];
            let origin = f64_at(bytes, at + 5);
            if fire_anim > MAX_FIRE_SLOT || !origin.is_finite() {
                return None;
            }
            out.cast = Some(CastPose { weapon: u32_at(bytes, at), fire_anim, origin, holding: flags & CAST_HELD != 0 });
            at += CAST_LEN;
        }
        if flags & CHOKED != 0 {
            out.choked = Some(u32_at(bytes, at));
        }
        Some(out)
    }
}

/// Ticks a display without a melee pose keeps travelling after its last part ended: a
/// viewer never sees a world field leave, only its new content, so the display without
/// the part must arrive first (an online host re-sends it until the viewer has it).
pub const KEEP_TICKS: f64 = 120.;
/// World payloads a display or a throw never takes for a new field, so the battle's
/// other world fields always find room.
pub(super) const RESERVE: usize = 32;

/// Publishes the combat displays of living, unseated soldiers with the battle's world
/// fields: every saber wielder's, and any other soldier's while a cast or a choke shows
/// on it and for `KEEP_TICKS` after.
#[derive(Default)]
pub struct Fields {
    payload: Vec<u8>,
    /// Per slot without a melee pose: its life and the tick a part last showed.
    shown: Vec<(f64, f64)>,
}

impl Fields {
    pub(crate) fn take(
        &mut self,
        tick: f64,
        units: &crate::units::owner::Units,
        store: &crate::gen::Store,
        powers: Option<&Powers>,
        effects: &EffectTable,
        stagger: &mut super::stagger::Stagger,
    ) -> super::codec::Result<()> {
        let Some(infantry) = units.infantry.as_deref() else {
            return Ok(());
        };
        let mut result = Ok(());
        let (payload, shown) = (&mut self.payload, &mut self.shown);
        infantry.hero_displays(store, tick, powers, effects, |slot, generation, display| {
            let index = slot as usize;
            if display.melee.is_none() {
                if shown.len() <= index {
                    shown.resize(index + 1, (f64::NAN, f64::NEG_INFINITY));
                }
                let (life, last) = &mut shown[index];
                if !display.is_empty() {
                    (*life, *last) = (generation, tick);
                } else if *life != generation || tick - *last > KEEP_TICKS {
                    return;
                }
            }
            let id = ID_BASE + f64::from(slot);
            if result.is_ok() && stagger.has_room(SCHEMA, id, RESERVE) {
                display.encode_into(payload);
                result = stagger.offer_native(tick, SCHEMA, id, generation, payload, None);
            }
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swing() -> MeleePose {
        MeleePose { state: 7, origin: 1200., loop_mode: LoopMode::Wrap, clip_len: 0.5, blades_off: 0b10, reaction: Some((41, 1230., 0.25)), swing_turn: -0.5 }
    }

    fn cast() -> CastPose {
        CastPose { weapon: 0x1357_9bdf, fire_anim: 1, origin: 1210., holding: true }
    }

    #[test]
    fn a_display_survives_the_wire_and_replays_the_battles_clocks() {
        let mut bytes = Vec::new();
        for display in [
            Display { melee: Some(swing()), ..Display::default() },
            Display { melee: Some(swing()), cast: Some(cast()), ..Display::default() },
            Display { cast: Some(CastPose { holding: false, ..cast() }), ..Display::default() },
            Display { choked: Some(0x2468_ace0), ..Display::default() },
            Display { melee: Some(MeleePose { reaction: None, ..swing() }), cast: Some(cast()), choked: Some(0) },
            Display::default(),
        ] {
            display.encode_into(&mut bytes);
            assert!(bytes.len() <= MAX_LEN);
            assert_eq!(Display::decode(&bytes), Some(display), "{display:?}");
        }
        let pose = swing();
        // 42 ticks in: 0.7 s wraps to 0.2 s of a half-second loop.
        assert!((pose.anim_time(1242.) - 0.2).abs() < 1e-12);
        let once = MeleePose { loop_mode: LoopMode::FinalFrame, ..pose };
        assert_eq!(once.anim_time(1242.), 0.5);
        assert_eq!(pose.anim_time(1100.), 0.);
        assert_eq!(pose.reaction_at(1236.), Some((41, 0.1)));
        assert_eq!(pose.reaction_at(1245.), None);
        assert_eq!(pose.reaction_at(1229.), None);
        assert!((cast().elapsed(1240.) - 0.5).abs() < 1e-12);
        assert_eq!(cast().elapsed(1200.), 0.);
    }

    #[test]
    fn bytes_no_producer_writes_are_refused() {
        let display = Display { melee: Some(MeleePose { state: 1, origin: 3., loop_mode: LoopMode::None, clip_len: 1., blades_off: 0, reaction: Some((2, 4., 0.5)), swing_turn: 0.25 }), cast: Some(cast()), choked: Some(9) };
        let mut bytes = Vec::new();
        display.encode_into(&mut bytes);
        assert_eq!(bytes.len(), MAX_LEN);
        let refused = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut b = bytes.clone();
            edit(&mut b);
            Display::decode(&b).is_none()
        };
        let melee = 3;
        let cast_at = melee + MELEE_LEN;
        assert!(refused(&|b| b.truncate(MAX_LEN - 1)));
        assert!(refused(&|b| b.push(0)));
        assert!(refused(&|b| b[0] = 1), "revision 1's layout");
        assert!(refused(&|b| b[2] |= 0x10), "an unknown part");
        assert!(refused(&|b| b[2] &= !CHOKED), "a part the flags do not name");
        assert!(refused(&|b| { b[2] &= !HAS_CAST; b.drain(cast_at..cast_at + CAST_LEN); }), "a held cast without its pose");
        assert!(refused(&|b| b[cast_at + 4] = 3), "a fire slot no weapon has");
        assert!(refused(&|b| b[cast_at + 5..cast_at + 13].copy_from_slice(&f64::NAN.to_le_bytes())));
        assert!(refused(&|b| b[melee + 10] = 3));
        assert!(refused(&|b| b[melee + 2..melee + 10].copy_from_slice(&f64::NAN.to_le_bytes())));
        assert!(refused(&|b| b[melee + 11..melee + 15].copy_from_slice(&(-1f32).to_le_bytes())));
        assert!(refused(&|b| b[melee + 11..melee + 15].copy_from_slice(&f32::INFINITY.to_le_bytes())));
        assert!(refused(&|b| b[melee + 18..melee + 26].copy_from_slice(&f64::INFINITY.to_le_bytes())));
        assert!(refused(&|b| b[melee + 26..melee + 30].copy_from_slice(&1e9f32.to_le_bytes())));
        assert!(refused(&|b| b[melee + 30..melee + 34].copy_from_slice(&f32::NAN.to_le_bytes())));
        assert!(refused(&|b| b[melee + 30..melee + 34].copy_from_slice(&4f32.to_le_bytes())));
        assert!(refused(&|b| b.truncate(2)));
    }
}
