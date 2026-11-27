//! Construct a display sample on the authority thread, after a completed tick.
use super::{BattleView, Body, Caption, Gauge, Mode, Phase, Role, Sample, Slot, Subject, Team};

/// One on-foot observation. Map coordinates are normalized with the origin at top left.
/// The match epoch, slab lease and gameplay generation identify the observed life.
#[derive(Clone, Copy, Debug)]
pub struct InfantrySample {
    pub epoch: u32,
    pub tick: u32,
    pub subject: Subject,
    pub slab: u32,
    pub health: f32,
    pub stamina: f32,
    pub map: [f32; 2],
    pub yaw: f32,
}

impl<'a> Body<'a> {
    /// Keep adjacent samples of the same life. A gap, new match or replacement
    /// starts at the new pose. The authority can supply blocking when observed.
    pub fn from_infantry(
        previous: Option<InfantrySample>,
        now: InfantrySample,
        max_health: f32,
        max_stamina: f32,
        role: Role,
        slots: [Option<Slot<'a>>; 2],
        reticle: &'a str,
    ) -> Self {
        let old = previous
            .filter(|p| {
                p.epoch == now.epoch
                    && p.subject == now.subject
                    && p.slab == now.slab
                    && p.tick.checked_add(1) == Some(now.tick)
            })
            .unwrap_or(now);
        let sample = |previous, current| Sample { previous, current };
        Self {
            subject: now.subject,
            role,
            health: Gauge {
                value: sample(old.health, now.health),
                maximum: max_health,
            },
            stamina: Gauge {
                value: sample(old.stamina, now.stamina),
                maximum: max_stamina,
            },
            map: std::array::from_fn(|i| sample(old.map[i], now.map[i])),
            yaw: sample(old.yaw, now.yaw),
            slots,
            reticle,
            blocking: false,
        }
    }
}

impl<'a> BattleView<'a> {
    /// Start a Hero Assault observation for a player and team. Assign `body` and
    /// borrow the producer's contacts, objectives, selected timer, events and score rows.
    /// Reuse those slices for every display frame until another fixed tick is published.
    pub fn for_hero_assault(
        tick: u32,
        phase: Phase,
        (player, team): (u32, u8),
        map_name: Caption<'a>,
        map_image: &'a str,
        teams: [Team<'a>; 2],
    ) -> Self {
        Self {
            tick,
            phase,
            mode: Mode::HeroAssault,
            player,
            team,
            map_name,
            map_image,
            teams,
            body: None,
            vehicle: None,
            aim: super::Aim::default(),
            target: None,
            contacts: &[],
            systems: &[],
            objectives: &[],
            timers: &[],
            events: &[],
            scores: &[],
            leaving: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_interpolation_requires_consecutive_ticks_and_the_same_life() {
        let now = InfantrySample {
            epoch: 2,
            tick: 44,
            subject: Subject {
                id: 5,
                generation: 8,
            },
            slab: 7,
            health: 31.,
            stamina: -2.,
            map: [0.3, 0.7],
            yaw: -3.12,
        };
        let old = InfantrySample {
            tick: 43,
            health: 32.,
            stamina: 5.,
            map: [0.2, 0.6],
            yaw: 3.12,
            ..now
        };
        let build = |previous| {
            Body::from_infantry(previous, now, 50., 12., Role::RosterHero, [None; 2], "")
        };
        let body = build(Some(old));
        assert_eq!(body.health.value.at(0.5), 31.5);
        assert_eq!(body.stamina.value.current, -2.);
        assert!(body.yaw.angle_at(0.5).abs() > 3.);
        for previous in [
            None,
            Some(InfantrySample { epoch: 3, ..old }),
            Some(InfantrySample { tick: 42, ..old }),
            Some(InfantrySample { slab: 8, ..old }),
            Some(InfantrySample {
                subject: Subject {
                    generation: 9,
                    ..old.subject
                },
                ..old
            }),
        ] {
            let body = build(previous);
            assert_eq!(body.health.value, Sample::steady(31.));
            assert_eq!(body.map, now.map.map(Sample::steady));
            assert_eq!(body.yaw, Sample::steady(now.yaw));
        }
    }
}
