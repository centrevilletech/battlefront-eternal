//! NetworkInputs' cumulative look, held-transition backup, ACK and successful
//! publication accounting. Callbacks still run at the same page boundaries;
//! native state advances only after their recorded success.
use super::{Output, Result, Words, EXCHANGE};
use crate::network_layout::net_op as ops;
#[derive(Default)]
pub struct Outbound { life: Option<[f64; 2]>, look: [f64; 2], sent_look: [f64; 2], sent_sequence: f64,
    held: Option<([u16; 4096], usize)>, transition: Option<[f64; 2]>, acked: f64, }
impl Outbound {
    pub fn run(&mut self, op: u32, input: &mut Words, out: &mut Output) -> Result<()> { match op {
            ops::OUTBOUND_RESET => { *self = Self::default(); }
            // NetworkInputs.clearPending retains the life's cumulative look.
            // A focus pause drops held-transition and unsent bookkeeping only.
            ops::OUTBOUND_CLEAR => { self.held = None;
                self.transition = None; }
            // NetworkInputs.ack is monotonic and ignores unsafe scalar input.
            // An acknowledged movement backup no longer needs redundancy.
            ops::OUTBOUND_ACK => { let sequence = input.number()?;
                if sequence.is_finite() && sequence.fract() == 0. && sequence.abs() <= 9007199254740991.
                    && sequence > self.acked
                { self.acked = sequence;
                    if self.transition.is_some_and(|p| p[0] <= sequence) { self.transition = None; } }
                out.extend(&[self.acked, self.transition.map_or(0., |p| p[0])])?; }
            ops::OUTBOUND_COMMAND => { let life = [input.number()?, input.number()?];
                let sequence = input.number()?;
                let tick = input.number()?;
                let held_length = input.uint(4096)?;
                let mut held = [0u16; 4096];
                for n in &mut held[..held_length] { *n = input.uint(65535)? as u16; }
                let count = input.uint(super::ROOM / 2)?;
                let reset = self.life != Some(life);
                if reset { *self = Self::default();
                    self.life = Some(life); }
                // Each sample is added separately: summing on the page first
                // would change floating-point cancellation across commands.
                for _ in 0..count { self.look[0] += input.number()?;
                    self.look[1] += input.number()?; }
                // Keep the source sorted-held comparison exact, including
                // repeated names; a lossy hash could miss a held transition.
                let changed = self .held .as_ref()
                    .is_some_and(|(last, length)| *length != held_length || last[..*length] != held[..held_length]);
                if changed { self.transition = Some([sequence, tick]); }
                self.held = Some((held, held_length));
                if self.transition.is_some_and(|p| p[0] <= self.acked || p[1] < tick - 24.) { self.transition = None; }
                out.extend(&[ reset as u8 as f64, changed as u8 as f64, self.transition.map_or(0., |p| p[0]),
                    self.look[0], self.look[1], ])?; }
            // NetworkInputs.sent runs only after transport accepted the packet.
            // A failed callback must leave cumulative look available to repair.
            ops::OUTBOUND_SENT => { let sequence = input.number()?;
                let look = [input.number()?, input.number()?];
                if sequence > self.sent_sequence {
                    out.extend(&[1., look[0] - self.sent_look[0], look[1] - self.sent_look[1]])?;
                    self.sent_look = look;
                    self.sent_sequence = sequence; } else { out.extend(&[0., 0., 0.])?; } }
            _ => return Err(EXCHANGE), }
        input.finish() } }
