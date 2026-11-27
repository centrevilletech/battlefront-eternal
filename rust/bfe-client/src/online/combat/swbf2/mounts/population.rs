//! Expected steady population and view-dependent rates of continuous sources.
use super::{MountObservation, Source};
use bfe_presentation::Content;

#[derive(Clone, Copy, Default)]
pub(super) struct Demand(pub f64);

/// The lowest rate a rest is worked out for: a source never rests more than
/// ninety-nine times as long as it played.
const MIN_REST_RATE: f64 = 0.01;

/// One mounted effect's plays. The budget slows a source by its birth clock
/// (`mount_rate`), which a burst capped by its MaxParticles outruns: it has its
/// whole count out within moments of every start, and an effect that burns out
/// starts again at once. So a burnt-out effect rests before it plays again, for
/// (1 / rate - 1) times as long as it last played, which keeps its mean
/// population at its rate as a steady source's is. At the full rate it starts
/// again at once.
#[derive(Clone, Copy)]
pub(super) struct Plays {
    started: f64,
    rest_until: Option<f64>,
}

impl Plays {
    pub fn started(now: f64) -> Self {
        Self { started: now, rest_until: None }
    }

    /// Whether the burnt-out effect may play again at `now` (display seconds).
    pub fn may_restart(&mut self, now: f64, rate: f64) -> bool {
        let started = self.started;
        let until = *self.rest_until.get_or_insert_with(|| {
            if rate >= 1. {
                now
            } else {
                now + (now - started) * (1. / rate.max(MIN_REST_RATE) - 1.)
            }
        });
        now >= until
    }
}

impl Demand {
    pub fn of(content: &Content, group: usize) -> Self {
        // Mean births per second times residence time (including finite child bursts).
        // Leave one eighth for random bursts and discrete presentation samples.
        Self((Self::population(content, group, f64::INFINITY) * 1.125).max(1.))
    }

    fn population(content: &Content, group: usize, duration: f64) -> f64 {
        let mean = |r: [f64; 2]| r[0] * 0.5 + r[1] * 0.5;
        content.groups[group]
            .iter()
            .map(|&index| {
                let d = &content.definitions[index];
                let life = d.transformer.lifetime.max(0.);
                let [a, b] = d.burst_delay;
                let shortest = a.min(b);
                let longest = a.max(b);
                let interval = if shortest < 0.001 && longest > 0.001 {
                    0.001 + (longest - 0.001) * 0.5 * ((longest - 0.001) / (longest - shortest))
                } else {
                    mean(d.burst_delay).max(0.001)
                };
                let [a, b] = d.burst_count;
                let lo = a.min(b);
                let hi = a.max(b);
                // The player rounds each sampled burst and emits at most 128.
                let burst = if lo == hi { lo.round().clamp(0., 128.) } else {
                    (1..=128).map(|n| ((hi - (n as f64 - 0.5)) / (hi - lo)).clamp(0., 1.)).sum()
                };
                let rate = burst / interval;
                let max = mean(d.max_particles);
                let active = (duration - mean(d.start_delay)).max(0.);
                let births = if active.is_infinite() {
                    rate
                } else if duration < mean(d.start_delay) {
                    0.
                } else {
                    (burst + active * rate).min(if max < 0. { f64::INFINITY } else { max })
                };
                let carriers = if duration.is_infinite() && max >= 0. {
                    (births * life).min(max)
                } else {
                    births * life
                };
                let children = if d.geometry.r#type == "emitter" && d.child != 0 {
                    births * Self::population(content, d.child, life)
                } else {
                    0.
                };
                carriers + children
            })
            .sum::<f64>()
            .min(u32::MAX as f64)
    }
}

#[derive(Default)]
pub(super) struct Lens {
    // The last submitted projection and inverse camera transform.
    camera: Option<([f64; 16], [f64; 16])>,
}
impl Lens {
    pub fn update(&mut self, values: &[f64]) {
        if let Some(m) = values.get(..32) {
            self.camera = Some((m[..16].try_into().unwrap(), m[16..].try_into().unwrap()));
        }
    }

    pub fn rank(&self, at: [f64; 3], was_inside: bool) -> (bool, f64) {
        let Some((p, v)) = &self.camera else {
            return (false, f64::INFINITY);
        };
        let transform = |m: &[f64; 16], [x, y, z]: [f64; 3], i: usize| {
            m[i] * x + m[4 + i] * y + m[8 + i] * z + m[12 + i]
        };
        let point = std::array::from_fn(|i| transform(v, at, i));
        let w = transform(p, point, 3);
        let edge = if was_inside { 1.08 } else { 1. };
        let inside = w > 0.
            && transform(p, point, 0).abs() <= w * edge
            && transform(p, point, 1).abs() <= w * edge
            && transform(p, point, 2).abs() <= w;
        (inside, point.iter().map(|x| x * x).sum())
    }
}

#[derive(Clone, Copy)]
pub(super) struct Ranked {
    pub index: usize,
    pub source: Source,
    pub owner: u64,
    pub inside: bool,
    pub distance: f64,
    pub previous: usize,
    granted: usize,
    pub demand: f64,
    pub sockets: usize,
    pub rate: f64,
}
impl Ranked {
    pub fn new(
        index: usize,
        source: Source,
        sample: &MountObservation,
        inside: bool,
        distance: f64,
    ) -> Self {
        Self {
            index,
            source,
            owner: sample.owner,
            inside,
            distance,
            previous: 0,
            granted: 0,
            demand: 0.,
            sockets: 0,
            rate: 1.,
        }
    }
}

// Stable byte passes give an exact front-to-back order in linear time.
// Distances are squared and nonnegative, so their IEEE bits sort numerically.
fn rank(rows: &mut [Ranked], scratch: &mut Vec<Ranked>) {
    let Some(&first) = rows.first() else { return };
    scratch.resize(rows.len(), first);
    for pass in 0..17 {
        let byte = |r: &Ranked| -> usize {
            let bits = match pass {
                0 => r.source as u64,
                1..=8 => r.owner >> ((pass - 1) * 8),
                _ => (r.distance.to_bits() | (u64::from(!r.inside) << 63)) >> ((pass - 9) * 8),
            };
            (bits & 255) as usize
        };
        let mut counts = [0; 256];
        for r in &*rows {
            counts[byte(r)] += 1;
        }
        if counts[byte(&rows[0])] == rows.len() {
            continue;
        }
        let mut end = 0;
        for n in &mut counts {
            let size = *n;
            *n = end;
            end += size;
        }
        for &r in &*rows {
            let at = &mut counts[byte(&r)];
            scratch[*at] = r;
            *at += 1;
        }
        rows.copy_from_slice(scratch);
    }
}

/// Reserve a common rate for the entire fleet before distributing the rest
/// from the nearest visible body outwards. Only the independent handle limit
/// can remove a complete socket. Particle pressure changes birth rates.
pub(super) fn distribute(
    rows: &mut [Ranked],
    scratch: &mut Vec<Ranked>,
    capacity: usize,
    slots: usize,
) {
    let requested = rows.iter().map(|r| r.sockets).sum::<usize>();
    rank(rows, scratch);
    if requested > slots {
        // Give each body a proportional number of physical sockets. Keep a
        // fractional socket with its current owner while demand stays the
        // same. Camera movement can then change rates without resetting tails.
        let minimum = usize::from(slots >= rows.len());
        let common = minimum * rows.len();
        let entitlement = |sockets| minimum + (sockets - minimum) * (slots - common) / (requested - common);
        let mut left = slots;
        for r in &mut *rows {
            r.granted = entitlement(r.sockets);
            left -= r.granted;
        }
        for retain in [true, false] {
            for r in &mut *rows {
                if left == 0 { break; }
                let base = entitlement(r.sockets);
                if r.granted == base && base < r.sockets && (r.previous > base) == retain {
                    r.granted += 1;
                    left -= 1;
                }
            }
        }
        for r in &mut *rows {
            r.demand *= r.granted as f64 / r.sockets as f64;
            r.sockets = r.granted;
        }
    }
    let total: f64 = rows.iter().map(|r| r.demand).sum();
    let ceiling: f64 = if requested > slots { 0.875 } else { 1. };
    let room = capacity as f64;
    let floor = if total * ceiling <= room {
        ceiling
    } else {
        0.5 * room / total
    };
    let mut extra = (room - floor * total).max(0.);
    for r in rows {
        let granted = extra.min((ceiling - floor) * r.demand);
        r.rate = if r.demand > 0. {
            floor + granted / r.demand
        } else {
            0.
        };
        extra -= granted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::effects::Spawner;
    use bfe_presentation::{
        layout::{draw, frame, header},
        Limits, Presentation,
    };
    use serde_json::json;

    fn emitter(burst: u32, maximum: i32) -> serde_json::Value {
        let color = json!({"channels": [[5,5],[7,7],[9,9],[255,255]]});
        json!({
            "startDelay":[0,0], "burstDelay":[0.033,0.033], "burstCount":[burst,burst],
            "maxParticles":[maximum,maximum], "size":[1,1], "color":color, "registerStep":true,
            "spawner":{"shape":"box","box":[[0,0],[0,0],[0,0]],"positionScale":[0,0],
                "velocityScale":[0,0],"offset":[[0,0],[0,0],[0,0]],"inheritVelocity":[0,0],
                "size":[1,1],"color":color,"fadeInTime":0,"startRotation":[0,0],"rotationVelocity":[0,0]},
            "transformer":{"lifetime":0.25}, "geometry":{"type":"particle","blend":"add"},
            "child":0, "gpu":false
        })
    }

    #[test]
    fn shuffled_fleets_keep_the_near_visible_life_and_thin_every_trail() {
        let fleet = |reverse: bool| {
            let mut rows: Vec<_> = (0..32)
                .map(|i| Ranked {
                    index: i,
                    source: Source::Hull,
                    owner: i as u64,
                    inside: i % 2 != 0,
                    distance: (32 - i) as f64,
                    previous: 0,
            granted: 0,
                    demand: 23.,
                    sockets: 3,
                    rate: 0.,
                })
                .collect();
            if reverse {
                rows.reverse();
            }
            distribute(&mut rows, &mut Vec::with_capacity(32), 240, 96);
            assert_eq!(rows[0].owner, 31);
            assert_eq!(rows[0].rate, 1.);
            assert_eq!(rows[15].owner, 1);
            assert!(rows.iter().all(|r| r.sockets == 3 && r.rate > 0.));
            assert!((rows.iter().map(|r| r.rate * r.demand).sum::<f64>() - 240.).abs() < 1e-9);
            rows.iter().map(|r| (r.owner, r.rate)).collect::<Vec<_>>()
        };
        assert_eq!(fleet(true), fleet(false));
    }

    #[test]
    fn a_pan_can_change_rates_without_moving_handles_between_live_sockets() {
        let mut grants = vec![0; 128];
        let mut rows = Vec::with_capacity(grants.len());
        let mut scratch = Vec::with_capacity(grants.len());
        for frame in 0..180 {
            rows.clear();
            for (index, &previous) in grants.iter().enumerate() {
                let distance = (index as f64 - frame as f64).abs();
                rows.push(Ranked {
                    index, source: Source::Recorded, owner: index as u64,
                    inside: distance < 11., distance, previous, granted: 0,
                    demand: 37., sockets: 10, rate: 0.,
                });
            }
            distribute(&mut rows, &mut scratch, 900, 1023);
            assert_eq!(rows.iter().map(|r| r.sockets).sum::<usize>(), 1023);
            for r in &rows {
                assert!((7..=8).contains(&r.sockets));
                if frame > 0 { assert_eq!(r.sockets, grants[r.index]); }
                grants[r.index] = r.sockets;
            }
        }
    }

    #[test]
    fn camera_edge_has_separate_enter_and_leave_thresholds() {
        let mut lens = Lens::default();
        let mut camera = [0.; 32];
        for i in [0, 5, 10, 15, 16, 21, 26, 31] {
            camera[i] = 1.;
        }
        lens.update(&camera);
        assert!(!lens.rank([1.04, 0., 0.], false).0);
        assert!(lens.rank([1.04, 0., 0.], true).0);
        assert!(!lens.rank([1.09, 0., 0.], true).0);
    }

    fn player() -> Spawner {
        let mut continuous = emitter(2, -1);
        continuous["burstDelay"] = json!([0.001, 0.079]);
        let content = serde_json::from_value(json!({
            "definitions":[continuous,emitter(5,5)],"groups":[[],[0],[1]]
        }))
        .unwrap();
        let mut fx = Spawner::new(Presentation::new(
            content,
            Limits {
                effects: 192,
                emitters: 256,
                particles: 128,
                commands: 256,
                events: 4096,
            },
        ));
        fx.enable_mounts();
        fx.owner.input[frame::VERSION] = bfe_presentation::layout::VERSION as f64;
        fx.owner.input[frame::PRESENT] = 1.;
        fx
    }

    #[test]
    fn variable_burst_charge_tracks_the_steady_population_at_different_display_rates() {
        for hz in [30, 60, 144] {
            let mut fx = player();
            let charge = Demand::of(fx.owner.content(), 1).0;
            let dt = 1. / hz as f64;
            fx.owner.input[frame::DT] = dt;
            let mut total = 0.;
            let mut handle = None;
            for i in 0..hz * 12 {
                fx.owner.input[frame::FRAME] = i as f64;
                fx.begin_mount_frame();
                let decor = fx.mount_player().unwrap();
                let h = *handle
                    .get_or_insert_with(|| decor.start(1, [0.; 3], [0.; 3], None, 7).unwrap());
                decor.mount_rate(h, 0.5, dt);
                fx.frame();
                if i >= 2 * hz {
                    total += fx.owner.header[header::PARTICLES];
                }
            }
            let mean = total / (10 * hz) as f64;
            assert!(
                (mean - charge * 0.5 / 1.125).abs() < 1.,
                "{hz} Hz: {mean} for {charge}"
            );
        }
    }

    #[test]
    fn a_burnt_out_burst_rests_so_its_mean_population_follows_its_rate() {
        // A burst of five that burns out in a quarter second, started again as a
        // mount restarts it: its population averages its charge times its rate.
        for rate in [1., 0.5, 0.25] {
            let mut fx = player();
            let charge = Demand::of(fx.owner.content(), 2).0;
            let dt = 1. / 60.;
            fx.owner.input[frame::DT] = dt;
            let (mut total, mut handle, mut plays, mut clock) = (0., None, Plays::started(0.), 0.);
            for i in 0..60 * 12 {
                clock += dt;
                fx.owner.input[frame::FRAME] = i as f64;
                fx.owner.input[frame::COMMANDS] = 0.;
                fx.begin_mount_frame();
                let decor = fx.mount_player().unwrap();
                let playing = handle.is_some_and(|h| decor.playing(h));
                if !playing && (handle.is_none() || plays.may_restart(clock, rate)) {
                    handle = decor.start(2, [0.; 3], [0.; 3], None, 7 + i as u32);
                    plays = Plays::started(clock);
                }
                fx.frame();
                if i >= 120 {
                    total += fx.owner.header[header::PARTICLES];
                }
            }
            let mean = total / 600.;
            let want = charge * rate / 1.125;
            assert!((mean / want - 1.).abs() < 0.08, "{rate}: {mean} for {want}");
        }
    }

    #[test]
    fn twenty_combat_bursts_keep_every_row_when_a_full_fleet_is_already_emitting() {
        fn run(ships: usize) -> Vec<Vec<f64>> {
            let mut fx = player();
            let dt = 1. / 60.;
            fx.owner.input[frame::DT] = dt;
            let mut snapshots = Vec::new();
            for tick in 0..46 {
                fx.owner.input[frame::FRAME] = tick as f64;
                fx.owner.input[frame::COMMANDS] = 0.;
                fx.begin_mount_frame();
                if tick == 0 {
                    for i in 0..ships {
                        fx.mount_player()
                            .unwrap()
                            .start(1, [90., 0., 0.], [0.; 3], None, i as u32)
                            .unwrap();
                    }
                }
                if tick == 40 {
                    if ships > 0 {
                        assert!(fx.owner.header[header::PARTICLES] >= 90.);
                    }
                    for i in 0..20 {
                        fx.start(2, [0.; 3], [0.; 3], None, 100 + i).unwrap();
                    }
                }
                fx.frame();
                assert!(fx.owner.header[header::PARTICLES] <= 128.);
                if tick >= 40 {
                    let rows: Vec<_> = fx
                        .owner
                        .draws
                        .chunks_exact(draw::STRIDE)
                        .take(fx.owner.header[header::DRAWS] as usize)
                        .filter(|row| row[draw::DEFINITION] == 1.)
                        .flatten()
                        .copied()
                        .collect();
                    assert_eq!(rows.len(), 100 * draw::STRIDE);
                    snapshots.push(rows);
                }
            }
            snapshots
        }
        let clean = run(0);
        assert_eq!(run(32), clean);
        assert_eq!(run(64), clean);
    }
}
