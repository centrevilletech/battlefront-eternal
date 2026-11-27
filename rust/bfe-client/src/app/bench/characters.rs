//! Play a fixed sequence of published character records through the battle renderer.
use crate::scene::{Scene, TextureOptions};
use bfe_content_native::animation2::state::State;
use serde::Deserialize;

#[derive(Deserialize)]
struct Sequence {
    query: String,
    #[serde(default)]
    receiver: bool,
    #[serde(default)]
    remote: bool,
    #[serde(default)]
    transforms: bool,
    playout: Option<Cadence>,
    vehicle: Option<Vehicle>,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Vehicle {
    id: u32,
    generation: u32,
    position: [f64; 3],
}
#[derive(Deserialize)]
struct Cadence {
    fps: u32,
    interval: u32,
}
struct Display {
    clock: crate::online::clock::Clock,
    frame: u32,
    cadence: Cadence,
}
impl Display {
    fn advance_to(&mut self, frame: u32) -> Result<f64, String> {
        if frame < self.frame {
            return Err("display frames must be chronological".into());
        }
        while self.frame < frame {
            self.clock.advance(1. / f64::from(self.cadence.fps))?;
            self.frame += 1;
        }
        Ok(self.clock.shown)
    }
    fn arrive(&mut self, tick: u32) -> Result<(), String> {
        let frame = tick.saturating_mul(self.cadence.fps).div_ceil(60);
        self.advance_to(frame.saturating_sub(1))?;
        self.clock.arrive(
            f64::from(tick),
            f64::from(frame) / f64::from(self.cadence.fps),
            f64::from(self.cadence.interval),
        )?;
        Ok(())
    }
}
#[derive(Deserialize)]
struct Step {
    id: Option<u32>,
    class: String,
    state: State,
    #[serde(default)]
    reset: bool,
    #[serde(default)]
    draws: Vec<Frame>,
}
#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    tick: f64,
    frame: Option<u32>,
    file: String,
}

pub(super) fn capture(
    opts: &super::super::cli::StandaloneOptions,
    path: &str,
    mut load: impl FnMut() -> glow::Context,
) -> Result<bool, String> {
    let seq: Sequence = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let root = opts
        .content_dir
        .as_deref()
        .ok_or("character playback requires a content directory")?;
    let out = opts
        .shots_dir
        .as_deref()
        .ok_or("character playback requires an output directory")?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let textures = TextureOptions::discover(&load(), false);
    let prepared = Scene::prepare_characters(root, &seq.query, textures, seq.receiver)?;
    let (mut scene, mut camera) =
        crate::loading_work::finish(Scene::open_prepared(prepared, opts.size, load))?;
    let mut view = *camera.snapshot();
    if !super::apply_view(&mut view) {
        return Err("character playback requires a fixed camera".into());
    }
    scene.apply_authored_planes(&mut view);
    let mut generation = 0;
    let mut receiver = (seq.receiver || seq.remote)
        .then(|| crate::online::receiver::Receiver::new(serde_json::Value::Null));
    let mut slots = bfe_sim::wire::records::SnapshotSlotTable::default();
    let mut previous = None;
    let replay = scene.online.as_mut().and_then(|r| r.replay.take());
    let history = crate::online::replay::History::default();
    let mut display = if let Some(cadence) = seq.playout {
        if cadence.fps < 60 || cadence.interval == 0 || seq.receiver {
            return Err("invalid character playout cadence".into());
        }
        Some(Display {
            clock: if cadence.interval == 1 {
                Default::default()
            } else {
                crate::online::clock::Clock::online()
            },
            frame: 0,
            cadence,
        })
    } else {
        None
    };
    for step in seq.steps {
        if let Some(display) = &mut display {
            display.arrive(step.state.tick)?;
        }
        let actors = scene
            .characters
            .as_mut()
            .ok_or("mission has no character resources")?;
        if step.reset {
            actors.clear();
            generation += 1;
        }
        let model = actors
            .named(&step.class)
            .ok_or_else(|| format!("class is absent from mission: {}", step.class))?;
        let id = step.id.unwrap_or(1);
        if let Some(receiver) = &mut receiver {
            use bfe_content_native::animation2::state::Air;
            use bfe_presentation::network_layout::{net_kind as k, net_record as r};
            let s = step.state;
            let class = actors.registry(model);
            if seq.remote {
                use bfe_sim::{
                    gen::{WireOwn, WireRecord, WireRecordKind, WireRecordState},
                    wire::{codec, records},
                };
                receiver.registry = true;
                receiver.welcomed = true;
                let record = WireRecord {
                    kind: WireRecordKind::Soldier,
                    id: id.into(),
                    generation: generation.into(),
                    class_id: Some(class as f64),
                    team: Some(1.),
                    position: Some(s.position),
                    velocity: Some(s.velocity),
                    yaw: Some(s.yaw),
                    pitch: Some(s.pitch),
                    health: Some(
                        if s.action == bfe_content_native::animation2::state::Action::Death {
                            0.
                        } else {
                            1.
                        },
                    ),
                    state: WireRecordState::F64(
                        if s.action == bfe_content_native::animation2::state::Action::Death {
                            7.
                        } else {
                            0.
                        },
                    ),
                    ..Default::default()
                };
                let mut rows = vec![record];
                if let Some(v) = &seq.vehicle {
                    rows.push(WireRecord {
                        kind: WireRecordKind::Vehicle,
                        state: WireRecordState::F64(0.),
                        quaternion: Some([0., 0., 0., 1.]),
                        health: Some(1.),
                        id: v.id.into(),
                        generation: v.generation.into(),
                        position: Some(v.position),
                        ..Default::default()
                    });
                }
                if let Some((vehicle, generation, index)) = s.seat {
                    if seq
                        .vehicle
                        .as_ref()
                        .is_none_or(|v| (v.id, v.generation) != (vehicle, generation))
                    {
                        return Err("missing vehicle for character seat".into());
                    }
                    rows.push(WireRecord {
                        kind: WireRecordKind::Seat,
                        id: id.into(),
                        vehicle_id: Some(vehicle.into()),
                        seat_index: Some(index as f64),
                        ..Default::default()
                    });
                }
                let tick = f64::from(s.tick);
                let snapshot = records::snapshot_from_records(
                    tick,
                    1.,
                    &rows,
                    vec![],
                    &mut slots,
                    &bfe_sim::wire::PositionBounds::LEGACY,
                    previous.as_ref(),
                    &bfe_sim::store::Strings::new(),
                )?;
                let interval = previous.as_ref().map_or(1., |old| tick - old.tick);
                let now = tick * 1000. / 60.;
                for packet in codec::encode_entities(&snapshot, previous.as_ref(), interval, bfe_sim::wire::PositionBounds::LEGACY.width())? {
                    receiver.message(false, &packet, now);
                }
                // A spectator has no correction payload, but its own packet
                // still completes the entity snapshot for the next delta.
                let own = codec::own_datagram(&WireOwn {
                    tick,
                    epoch: snapshot.epoch,
                    interval,
                    baseline_tick: previous.as_ref().map_or(0., |old| old.tick),
                    own_parts: 1.,
                    ..Default::default()
                })?;
                receiver.message(false, &own, now);
                if let Some(err) = receiver.error() {
                    return Err(err);
                }
                let acknowledged = receiver.network.borrow().header
                    [bfe_presentation::network_layout::net_header::ACK_TICK];
                if acknowledged != tick {
                    return Err(format!(
                        "character snapshot {tick} left the acknowledged baseline at {acknowledged}"
                    ));
                }
                receiver.clock.advance(interval / 60.)?;
                if !step.draws.is_empty() {
                    let row = receiver
                        .record(k::SOLDIER, id.into(), generation.into())
                        .ok_or("missing remote character")?;
                    eprintln!(
                        "character remote snapshot={} soldier_tick={} health={} discontinuity={} seated={} position={:?}",
                        receiver.latest,
                        row[r::TICK],
                        row[r::HEALTH],
                        row[r::DISC],
                        row[r::SEAT_PRESENT],
                        &row[r::POSITION..r::POSITION + 3]
                    );
                }
                previous = Some(snapshot);
            } else {
                receiver.corrections += 1;
                receiver.latest = s.tick as f64;
                receiver.clock.shown = receiver.latest;
                receiver.own = crate::online::receiver::Own {
                    id: f64::from(id),
                    generation: generation as f64,
                    class,
                    position: s.position,
                    velocity: s.velocity,
                    yaw: s.yaw,
                    alive: true,
                    tick: receiver.latest,
                    health: 1.,
                    ..Default::default()
                };
                let row = &mut receiver.own_fact;
                row[r::KIND] = k::SOLDIER as f64;
                row[r::ID] = f64::from(id);
                row[r::TICK] = receiver.latest;
                row[r::GENERATION] = generation as f64;
                row[r::CLASS_ID] = class as f64;
                row[r::HEALTH] = 1.;
                row[r::POSTURE] = f64::from(s.crouch as u8);
                row[r::STATE] = match s.air {
                    Air::Jump => 1.,
                    Air::Fall => 2.,
                    Air::Jet => 4.,
                    _ => 0.,
                };
                row[r::YAW] = s.yaw;
                row[r::POSITION..r::POSITION + 3].copy_from_slice(&s.position);
                row[r::VELOCITY..r::VELOCITY + 3].copy_from_slice(&s.velocity);
            }
            if seq.remote {
                actors.present_records(receiver, None, None, false);
            } else {
                scene.present_network(
                    receiver,
                    replay.as_ref().ok_or("live battle has no replay")?,
                    &history,
                    &mut camera,
                    1. / 60.,
                );
            }
        } else {
            actors.publish((id, generation), model, step.state);
        }
        for sample in step.draws {
            if sample.file.is_empty()
                || !sample
                    .file
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || !sample.file.ends_with(".png")
                || !sample.tick.is_finite()
            {
                return Err("invalid character sample".into());
            }
            let shown = match (&mut display, sample.frame) {
                (Some(clock), Some(frame)) => clock.advance_to(frame)?,
                (None, None) => sample.tick,
                _ => return Err("clocked captures require a display frame".into()),
            };
            scene.characters.as_mut().unwrap().at(shown);
            scene.draw(&view)?;
            scene.canvas.show();
            let pixels = scene.pixels()?;
            super::super::script::write_png(
                &out.join(&sample.file),
                opts.size[0] as u32,
                opts.size[1] as u32,
                &pixels,
            )?;
            if seq.transforms {
                use bfe_render::{
                    frame_plan::layout::{meta, role},
                    instances as d,
                };
                let count =
                    scene.bridge.r.frames.meta[role::NEAR * meta::LEN + meta::RECORDS] as usize;
                let (rows, _) =
                    scene.bridge.r.core.input[d::IN_RECORDS..].as_chunks::<{ d::REC_STRIDE }>();
                for row in rows.iter().take(count) {
                    if row[d::REC_MOTION_ID] == crate::characters::motion_id(id)
                        && row[d::REC_MOTION_GENERATION] == f64::from(generation)
                        && row[d::REC_BONES] == 0.
                    {
                        eprintln!(
                            "character transform {} part={} flags={} matrix={:?}",
                            sample.file,
                            row[d::REC_MOTION_PART],
                            row[d::REC_FLAGS],
                            &row[d::REC_WORLD..d::REC_WORLD + 16]
                        );
                    }
                }
            }
            let actors = scene.characters.as_ref().unwrap();
            eprintln!(
                "character frame {} tick={} class={} {:?} visible_records={} fallback_deaths={} blended_peak={} dropped_blended={}",
                sample.file,
                shown,
                step.class,
                actors.drawn,
                actors.visible_budget(&scene.bridge), actors.fallback_deaths,scene.bridge.r.instances.overflow.peak_blended,scene.bridge.r.instances.overflow.dropped_blended
            );
        }
    }
    Ok(true)
}
