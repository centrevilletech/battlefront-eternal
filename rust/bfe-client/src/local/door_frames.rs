//! Door pairs retained at their simulation tick for an independently clocked view.
use bfe_sim::space::SpaceDoorSnapshot;

#[derive(Default)]
pub(crate) struct DoorFrames {
    frames: std::collections::VecDeque<(f64, f64, Vec<SpaceDoorSnapshot>)>,
}

impl DoorFrames {
    pub fn clear(&mut self) {
        self.frames.clear();
    }
    pub fn publish(&mut self, tick: f64, states: &[SpaceDoorSnapshot]) {
        if states.is_empty() {
            self.clear();
            return;
        }
        if self
            .frames
            .back()
            .is_some_and(|(_, previous, _)| *previous >= tick)
        {
            self.clear();
        }
        let prev_tick = self
            .frames
            .back()
            .filter(|(_, end, _)| end.is_finite() && *end < tick)
            .map_or(tick - 1., |(_, end, _)| *end);
        // The renderer retains the most recent quarter second of local ticks.
        const CAPACITY: usize = 16;
        if self.frames.is_empty() {
            self.frames.reserve(CAPACITY);
            self.frames.extend((0..CAPACITY).map(|_| {
                (
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                    Vec::with_capacity(states.len()),
                )
            }));
        }
        let (_, _, mut copy) = self.frames.pop_front().unwrap();
        copy.clear();
        copy.extend_from_slice(states);
        self.frames.push_back((prev_tick, tick, copy));
    }
    pub fn sample(&self, tick: f64) -> Option<(&[SpaceDoorSnapshot], f64)> {
        let (start, end, states) = self
            .frames
            .iter()
            .find(|(_, end, _)| *end >= tick)
            .or_else(|| self.frames.back())?;
        let span = (*end - *start).max(1.);
        Some((states, ((tick - *start) / span).clamp(0., 1.)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_interpolates_the_same_world_time_across_render_rates_and_epochs() {
        let mut history = DoorFrames::default();
        let mut state = SpaceDoorSnapshot {
            id: 0,
            instance: 12,
            previous_fraction: 0.,
            open_fraction: 0.,
            locked: false,
            visible: true,
            collidable: true,
            prev_door_open: false,
            door_open: true,
            position: [0.; 3],
        };
        for tick in 1..=30 {
            state.previous_fraction = state.open_fraction;
            state.open_fraction = f64::from(tick) / 60.;
            history.publish(f64::from(tick), &[state]);
        }
        for hz in [30, 60, 144, 240] {
            for frame in (hz / 4)..=hz / 2 {
                let time = f64::from(frame) / f64::from(hz);
                let (rows, alpha) = history.sample(60. * time).unwrap();
                let fraction = rows[0].previous_fraction
                    + (rows[0].open_fraction - rows[0].previous_fraction) * alpha;
                assert!((fraction - time).abs() < 1e-10);
            }
        }
        history.publish(1., &[]);
        assert!(history.sample(1.).is_none());
    }
}
