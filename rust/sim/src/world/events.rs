//! The shared simulation journal. Record values and callback identities
//! stay with the host; sequence numbers, ring indices, reader cursors and
//! overflow policy are advanced here, including across nested callbacks.
use crate::gen::{WorldEventCall, WorldEventState};

pub trait Host {
    /// Publish state before a host operation and refresh it afterwards.
    /// Load returns whether the selected record is truthy. The host keeps
    /// that exact record until Deliver, even if its ring slot is replaced.
    fn call(&mut self, state: &mut WorldEventState, op: WorldEventCall, index: f64) -> bool;
}

pub fn emit(state: &mut WorldEventState, host: &mut impl Host) {
    let index = state.seq % state.capacity;
    host.call(state, WorldEventCall::Write, index);
    state.seq += 1.0;
    host.call(state, WorldEventCall::Notify, 0.0);
}

pub fn reset(state: &mut WorldEventState, host: &mut impl Host) {
    host.call(state, WorldEventCall::Clear, 0.0);
    state.seq = 0.0;
}

pub fn read(state: &mut WorldEventState, host: &mut impl Host) {
    // Overflow is checked once on entry. Events emitted while the reader
    // runs extend this read, without another overflow check in that call.
    if state.seq - state.cursor > state.capacity {
        state.dropped += state.seq - state.cursor - state.capacity;
        state.cursor = state.seq - state.capacity;
    }
    while state.cursor < state.seq {
        let index = state.cursor % state.capacity;
        let present = host.call(state, WorldEventCall::Load, index);
        state.cursor += 1.0;
        if present { host.call(state, WorldEventCall::Deliver, 0.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Journal {
        ring: Vec<Option<f64>>, pending: Option<f64>, next: f64,
        delivered: Vec<(f64, f64)>, append: bool,
    }
    impl Host for Journal {
        fn call(&mut self, state: &mut WorldEventState, op: WorldEventCall, index: f64) -> bool {
            match op {
                WorldEventCall::Write => self.ring[index as usize] = Some(self.next),
                WorldEventCall::Notify => {},
                WorldEventCall::Clear => self.ring.fill(None),
                WorldEventCall::Load => self.pending = self.ring[index as usize],
                WorldEventCall::Deliver => {
                    self.delivered.push((self.pending.unwrap(), state.cursor));
                    if self.append {
                        self.append = false; self.next = 9.0;
                        emit(state, self);
                    }
                },
            }
            self.pending.is_some()
        }
    }
    #[test]
    fn independent_readers_drop_only_overwritten_records() {
        let mut state = WorldEventState { capacity: 3.0, ..Default::default() };
        let mut host = Journal { ring: vec![None; 3], ..Default::default() };
        for i in 0..5 { host.next = f64::from(i); emit(&mut state, &mut host); }
        read(&mut state, &mut host);
        assert_eq!(state.cursor, 5.0); assert_eq!(state.dropped, 2.0);
        assert_eq!(host.delivered, vec![(2.0, 3.0), (3.0, 4.0), (4.0, 5.0)]);
        state.cursor = 4.0; state.dropped = 0.0; host.delivered.clear();
        read(&mut state, &mut host);
        assert_eq!(host.delivered, vec![(4.0, 5.0)]);
        assert_eq!(state.dropped, 0.0);
    }
    #[test]
    fn reader_observes_nested_emissions_and_reset_keeps_its_cursor() {
        let mut state = WorldEventState { capacity: 3.0, time: 2.5, ..Default::default() };
        let mut host = Journal { ring: vec![None; 3], ..Default::default() };
        emit(&mut state, &mut host); host.append = true;
        read(&mut state, &mut host);
        assert_eq!(host.delivered, vec![(0.0, 1.0), (9.0, 2.0)]);
        reset(&mut state, &mut host);
        assert_eq!(state.seq, 0.0); assert_eq!(state.time, 2.5); assert_eq!(state.cursor, 2.0);
        assert!(host.ring.iter().all(Option::is_none));
    }
}
