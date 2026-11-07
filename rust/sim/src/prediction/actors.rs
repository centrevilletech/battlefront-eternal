//! Private infantry and droideka replay against an explicitly supplied
//! collision world. Commands own their bodies, recoil, clips and caches;
//! no gameplay or JavaScript callback runs while collision state is borrowed.

use crate::collision::{
    self, broad,
    spheres::{self, GatherCache, Gathered},
    State, MISS, TERRAIN,
};
use crate::combat::combat::update_kick;
use crate::gen::{Air, Combat, Kick, MoveInput, Posture, Soldier, Unit, UnitBody};
use crate::movement::{
    scene::{GroundCache, Scene},
    soldier::{Air as MAir, Input, Mover, Posture as MPosture, Trig, PART_CACHES, V3},
};
use crate::pose::Clip;
use crate::units::{
    droideka::{self, DroidekaDef, DroidekaParts, ModeClips},
    shapes::{Bounds, HitShape},
    world::{CastHit, UnitsWorld},
};
use std::rc::Rc;
use three::Vector3;

const INVALID: &str = "invalid prediction actor command";
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}
fn or_zero(x: f64) -> f64 {
    if truthy(x) {
        x
    } else {
        0.0
    }
}

struct Reader<'a> {
    words: &'a [f64],
    at: usize,
}
impl<'a> Reader<'a> {
    fn word(&mut self) -> Result<f64, &'static str> {
        let x = *self.words.get(self.at).ok_or(INVALID)?;
        self.at += 1;
        Ok(x)
    }
    fn count(&mut self) -> Result<usize, &'static str> {
        let x = self.word()?;
        if !x.is_finite() || x < 0.0 || x.fract() != 0.0 || x > u32::MAX as f64 {
            return Err(INVALID);
        }
        Ok(x as usize)
    }
    fn flag(&mut self) -> Result<bool, &'static str> {
        match self.word()? {
            0.0 => Ok(false),
            1.0 => Ok(true),
            _ => Err(INVALID),
        }
    }
    fn slice(&mut self, n: usize) -> Result<&'a [f64], &'static str> {
        let end = self.at.checked_add(n).ok_or(INVALID)?;
        let x = self.words.get(self.at..end).ok_or(INVALID)?;
        self.at = end;
        Ok(x)
    }
    fn optional(&mut self) -> Result<Option<f64>, &'static str> {
        let present = self.flag()?;
        let x = self.word()?;
        Ok(present.then_some(x))
    }
    fn done(&self) -> Result<(), &'static str> {
        if self.at == self.words.len() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}

fn clip(r: &mut Reader<'_>) -> Result<Option<Rc<Clip>>, &'static str> {
    if !r.flag()? {
        return Ok(None);
    }
    let frames = r.count()?;
    let fps = r.word()?;
    let root_motion = r.flag()?;
    let count = r.count()?;
    let tracks = r
        .slice(count)?
        .iter()
        .map(|&x| {
            if !x.is_finite() || x.fract() != 0.0 || x < -1.0 || x > i32::MAX as f64 {
                Err(INVALID)
            } else {
                Ok(x as i32)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let count = r.count()?;
    if frames == 0
        || frames
            .checked_mul(tracks.len())
            .and_then(|n| n.checked_mul(7))
            != Some(count)
    {
        return Err(INVALID);
    }
    let data = r.slice(count)?.iter().map(|&x| x as f32).collect();
    Ok(Some(Rc::new(Clip {
        frames,
        fps,
        root_motion,
        tracks,
        data,
    })))
}

/// Scalar commands shared by the adapter and native corpus replay.
/// 0: kind (0 infantry, 1 droideka); droideka adds five (present,value)
/// definition numbers, then fourteen clips (present; frames,fps,rootMotion,
/// track count/tracks, Float32 data count/data).
/// 1: world, ignored body slot, dt, body-flag count/flags, generated Soldier
/// (64) or Droideka (77) row; infantry adds contactCount, contactMean xyz,
/// stepBlocked and whether the original world uses MovementKernel. Then
/// forward,right,turn,pitch,jump,crouch,prone,use,jetPresent,jet xyz;
/// kick rate/hold/offset; shieldEquipped,underwater,shieldPressed,airBlocked,
/// dive truthiness,lock. Output: the body row, infantry scratch (five words),
/// kick-write flag, kick rate/hold/offset. No row carries live store references.
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    kind: Option<u32>,
    pub(crate) unit: Unit,
    pub(crate) combat: Combat,
    mover: Mover,
    caches: [GatherCache; PART_CACHES],
    gathered: Gathered,
    ground: GroundCache,
    parts: Option<DroidekaParts>,
    // Private replay casts cannot overwrite the authoritative query bank.
    candidates: Vec<u32>,
    enabled: Vec<u8>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            output: Vec::new(),
            kind: None,
            unit: Unit::default(),
            combat: Combat::default(),
            mover: Mover::new(0),
            caches: std::array::from_fn(|_| GatherCache::new(0.75)),
            gathered: Gathered::new(),
            ground: GroundCache::default(),
            parts: None,
            candidates: vec![0; collision::MAX_BODIES],
            enabled: vec![0; collision::MAX_BODIES],
        }
    }
}
impl Session {
    pub fn run(&mut self, op: u32, collision: &mut State) -> Result<(), &'static str> {
        self.output.clear();
        let mut r = Reader {
            words: &self.input,
            at: 0,
        };
        if op == 0 {
            let kind = r.count()?;
            let parts = match kind {
                0 => None,
                1 => {
                    let def = DroidekaDef {
                        max_pitch_speed: r.optional()?,
                        ball_turn_speed: r.optional()?,
                        ball_max_lean: r.optional()?,
                        ball_acceleration: r.optional()?,
                        ball_rolling_friction: r.optional()?,
                    };
                    let mut clips = ModeClips::default();
                    for slot in &mut clips.by_mode {
                        *slot = clip(&mut r)?;
                    }
                    Some(DroidekaParts {
                        def,
                        clips: Rc::new(clips),
                        rig: None,
                    })
                }
                _ => return Err(INVALID),
            };
            r.done()?;
            self.kind = Some(kind as u32);
            self.parts = parts;
            // ActorPrediction.configure used to replace the complete scene cache.
            // Body changes invalidate query facts, not the owned reservations.
            for cache in &mut self.caches { cache.reset(); }
            self.gathered.reset();
            self.ground.reset();
            self.mover = Mover::new(0);
            return Ok(());
        }
        if op != 1 {
            return Err(INVALID);
        }
        let kind = self.kind.ok_or(INVALID)?;
        let world = r.count()?;
        let ignored = r.word()?;
        let dt = r.word()?;
        if !ignored.is_finite()
            || ignored < -1.0
            || ignored.fract() != 0.0
            || ignored > u32::MAX as f64
        {
            return Err(INVALID);
        }
        let count = r.count()?;
        if collision.worlds.get(world).map(|w| w.order.len()) != Some(count)
            || count > collision::MAX_BODIES
        {
            return Err(INVALID);
        }
        let flags = (0..count)
            .map(|_| r.flag().map(u8::from))
            .collect::<Result<Vec<_>, _>>()?;
        let soldier = if kind == 0 {
            let row = r.slice(Soldier::WORDS)?;
            if ![0., 1., 2.].contains(&row[43])
                || ![0., 1., 2.].contains(&row[47])
                || ![0., 1., 2.].contains(&row[44])
                || row[44] == 0.0 && ![0., 1., 2., 3.].contains(&row[45])
                || row[58] == 0.0
            {
                return Err(INVALID);
            }
            UnitBody::Soldier(Soldier::take(row, &[]))
        } else {
            let row = r.slice(crate::gen::Droideka::WORDS)?;
            if !row[40].is_finite()
                || row[40] < 0.0
                || row[40] > 13.0
                || row[40].fract() != 0.0
                || row[62] == 0.0
            {
                return Err(INVALID);
            }
            UnitBody::Droideka(crate::gen::Droideka::take(row, &[]))
        };
        let (scratch, movement_backend) = if kind == 0 {
            (
                (
                    r.word()?,
                    V3::new(r.word()?, r.word()?, r.word()?),
                    r.flag()?,
                ),
                r.flag()?,
            )
        } else {
            ((0.0, V3::ZERO, false), false)
        };
        let mut input = MoveInput {
            forward: r.word()?,
            right: r.word()?,
            turn: r.word()?,
            pitch: r.word()?,
            jump: r.flag()?,
            crouch: r.flag()?,
            prone: r.flag()?,
            r#use: r.flag()?,
            ..MoveInput::default()
        };
        let jet = r.flag()?;
        let direction = [r.word()?, r.word()?, r.word()?];
        input.jet_direction = jet.then_some(direction);
        let kick = Kick {
            rate: r.word()?,
            hold: r.word()?,
            offset: r.word()?,
        };
        let shield = r.flag()?;
        let underwater = r.flag()?;
        let pressed = r.flag()?;
        let airborne = r.flag()?;
        let dive = r.flag()?;
        let lock = r.word()?;
        r.done()?;
        self.unit.soldier = soldier;
        self.unit.input = input;
        let kick_write = truthy(kick.rate) || truthy(kick.hold) || truthy(kick.offset);
        self.combat.kick = kick;
        self.step(collision, world, ignored as i64, &flags, dt, scratch, movement_backend,
            shield, underwater, pressed, airborne, dive, lock)?;
        match &self.unit.soldier {
            UnitBody::Soldier(s) => {
                self.output.resize(Soldier::WORDS, 0.0);
                s.put(&mut self.output, &mut Vec::new());
                let (count, mean, blocked) = if movement_backend {
                    scratch
                } else {
                    self.mover.prediction_scratch()
                };
                self.output
                    .extend([count, mean.x, mean.y, mean.z, blocked as u8 as f64]);
            }
            UnitBody::Droideka(d) => {
                self.output.resize(crate::gen::Droideka::WORDS, 0.0);
                d.put(&mut self.output, &mut Vec::new());
            }
        }
        self.output.extend([
            kick_write as u8 as f64,
            self.combat.kick.rate,
            self.combat.kick.hold,
            self.combat.kick.offset,
        ]);
        Ok(())
    }

    /// InfantryPrediction.step / DroidekaPrediction.step on retained private
    /// state. A whole replay uses this entry without rows or foreign callbacks.
    pub(crate) fn step(&mut self, collision: &mut State, world: usize, ignored: i64,
        flags: &[u8], dt: f64, scratch: (f64, V3, bool), movement_backend: bool,
        shield: bool, underwater: bool, pressed: bool, airborne: bool, dive: bool, lock: f64,
    ) -> Result<(), &'static str> {
        if self.kind == Some(0) {
            let UnitBody::Soldier(s) = &mut self.unit.soldier else {
                unreachable!()
            };
            let m = &mut self.mover;
            load_mover(m, s, world, ignored);
            m.set_prediction_scratch(
                if movement_backend { 0.0 } else { scratch.0 },
                if movement_backend {
                    V3::ZERO
                } else {
                    scratch.1
                },
                if movement_backend { false } else { scratch.2 },
            );
            let input = &self.unit.input;
            let inp = Input {
                forward: input.forward,
                right: input.right,
                turn: or_zero(input.turn),
                pitch: or_zero(input.pitch),
                jump: input.jump,
                crouch: input.crouch,
                prone: input.prone,
                jet_direction: input
                    .jet_direction
                    .filter(|v| v.iter().any(|&x| truthy(x)))
                    .map(V3::from),
            };
            let yaw = m.aim_yaw;
            let mut trig = Trig {
                sin0: v8math::sin(yaw),
                cos0: v8math::cos(yaw),
                ..Trig::default()
            };
            if m.air == MAir::Jet && m.jet.hover {
                trig.sin_pitch = v8math::sin(m.aim_pitch);
                trig.cos_pitch = v8math::cos(m.aim_pitch);
            }
            let rest = m.begin(&inp, dt, &trig);
            (trig.sin1, trig.cos1) = if m.aim_yaw.to_bits() != yaw.to_bits() {
                (v8math::sin(m.aim_yaw), v8math::cos(m.aim_yaw))
            } else {
                (trig.sin0, trig.cos0)
            };
            let mut scene = Scene {
                s: collision,
                world,
                flags,
                gathered: &mut self.gathered,
                caches: &mut self.caches,
                ground_cache: &mut self.ground,
            };
            m.finish(rest, &inp, dt, &trig, &mut scene);
            store_mover(s, m, dt);
        } else {
            let mut scene = ActorWorld {
                candidates: &mut self.candidates,
                enabled: &mut self.enabled,
                collision,
                world,
                flags,
                caches: &mut self.caches,
                gathered: &mut self.gathered,
            };
            droideka::tick(
                0,
                &mut self.unit,
                dt,
                &mut scene,
                self.parts.as_mut().ok_or(INVALID)?,
            );
            let UnitBody::Droideka(d) = &mut self.unit.soldier else {
                unreachable!()
            };
            let stick = v8math::max(
                or_zero(self.unit.input.forward).abs(),
                or_zero(self.unit.input.right).abs(),
            );
            // A normal droideka's posture follows its mode; keep the generic
            // air/dive/lock fields supplied by the replay wrapper as well.
            let blocked = airborne || dive || lock > 0.0 || d.rolled() || underwater;
            let _ = stick;
            if shield && pressed && !blocked {
                d.toggle_shield();
            }
        }
        update_kick(&mut self.combat, &mut self.unit, dt);
        Ok(())
    }

}

fn load_mover(m: &mut Mover, s: &Soldier, world: usize, ignored: i64) {
    m.world = world;
    m.max_speed = s.max_speed;
    m.max_strafe = s.max_strafe;
    m.max_jump = s.max_jump;
    m.max_turn = s.max_turn;
    m.max_pitch = s.max_pitch;
    m.accel = s.accel;
    m.pitch_limits = s.pitch_limits;
    m.jet.jump = s.jet.jump;
    m.jet.push = s.jet.push;
    m.jet.recharge = s.jet.recharge;
    m.jet.cost = s.jet.cost;
    m.jet.initial = s.jet.initial;
    m.jet.min_border = s.jet.min_border;
    m.jet.hover = s.jet.hover;
    m.threshold = s.collision_rule.threshold;
    m.scale = s.collision_rule.scale;
    m.max_fly_height = s.max_fly_height;
    m.getup_time = s.getup_time;
    m.dive_time = s.dive_time;
    m.position = V3::from(s.position);
    m.settled = V3::from(s.settled);
    m.velocity = V3::from(s.velocity);
    m.normal = V3::from(s.normal);
    m.avoidance = V3::from(s.avoidance);
    m.aim_yaw = s.aim_yaw;
    m.aim_pitch = s.aim_pitch;
    m.state_time = s.state_time;
    m.lock = s.lock;
    m.fuel = s.fuel;
    m.dive = s.dive;
    m.posture = match s.posture {
        Posture::Stand => MPosture::Stand,
        Posture::Crouch => MPosture::Crouch,
        Posture::Prone => MPosture::Prone,
    };
    m.dive_from = match s.dive_from {
        Posture::Stand => MPosture::Stand,
        Posture::Crouch => MPosture::Crouch,
        Posture::Prone => MPosture::Prone,
    };
    m.air = match s.air {
        None => MAir::None,
        Some(Air::Jump) => MAir::Jump,
        Some(Air::Fall) => MAir::Fall,
        Some(Air::Thrown) => MAir::Thrown,
        Some(Air::Jet) => MAir::Jet,
    };
    m.grounded = s.grounded;
    m.ground_adjustment = s.ground_adjustment;
    m.pass_through = ignored;
}
fn store_mover(s: &mut Soldier, m: &Mover, dt: f64) {
    s.previous = s.position;
    s.position = m.position.arr();
    s.settled = m.settled.arr();
    s.velocity = m.velocity.arr();
    s.normal = m.normal.arr();
    s.avoidance = m.avoidance.arr();
    s.aim_yaw = m.aim_yaw;
    s.aim_pitch = m.aim_pitch;
    s.state_time = m.state_time;
    s.lock = m.lock;
    s.fuel = m.fuel;
    s.dive = m.dive;
    s.posture = match m.posture {
        MPosture::Stand => Posture::Stand,
        MPosture::Crouch => Posture::Crouch,
        MPosture::Prone => Posture::Prone,
    };
    s.dive_from = match m.dive_from {
        MPosture::Stand => Posture::Stand,
        MPosture::Crouch => Posture::Crouch,
        MPosture::Prone => Posture::Prone,
    };
    s.air = match m.air {
        MAir::None => None,
        MAir::Jump => Some(Air::Jump),
        MAir::Fall => Some(Air::Fall),
        MAir::Thrown => Some(Air::Thrown),
        MAir::Jet => Some(Air::Jet),
    };
    s.grounded = m.grounded;
    s.ground_adjustment = m.ground_adjustment;
    s.collision_damage = m.collision_damage;
    s.tick_length = dt;
    s.look_x = 0.0;
    s.look_y = 0.0;
}

struct ActorWorld<'a> {
    candidates: &'a mut [u32],
    enabled: &'a mut [u8],
    collision: &'a mut State,
    world: usize,
    flags: &'a [u8],
    caches: &'a mut [GatherCache; PART_CACHES],
    gathered: &'a mut Gathered,
}
impl UnitsWorld for ActorWorld<'_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        let ray = collision::Ray {
            ox: origin.x,
            oy: origin.y,
            oz: origin.z,
            dx: direction.x,
            dy: direction.y,
            dz: direction.z,
        };
        let n =
            broad::world_candidates_near_into(self.collision, self.world, &ray, far, -1, false, false, self.candidates);
        for i in 0..n {
            self.enabled[i] = self.flags[self.candidates[i] as usize];
        }
        let (code, h) = collision::world_cast_ray_in(
            self.collision,
            self.world,
            &ray,
            far,
            v8math::hypot(direction.x, direction.z),
            &self.candidates[..n],
            &self.enabled[..n],
        );
        (code != MISS).then(|| CastHit {
            point: Vector3::new(h.px, h.py, h.pz),
            normal: Vector3::new(h.nx, h.ny, h.nz),
            distance: h.distance,
            terrain: code == TERRAIN,
        })
    }
    fn visit_gathered(
        &mut self,
        _: u32,
        c: &Vector3,
        radius: f64,
        cache: usize,
        visit: &mut dyn FnMut(bool, &[Vector3; 3]),
    ) {
        spheres::gather(
            self.collision,
            self.world,
            [c.x, c.y, c.z],
            radius,
            None,
            Some(&mut self.caches[cache]),
            -1,
            self.flags,
            self.gathered,
        );
        for p in self.gathered.list() {
            for t in &p.tris {
                visit(p.moving, &[t.a, t.b, t.c].map(|v| Vector3::new(v[0], v[1], v[2])));
            }
        }
    }
    fn death_blast(&mut self, _: u32, _: &mut Unit) {}
    fn blow_apart(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn seat_exposed(&mut self, _: u32, _: &Unit) -> bool {
        unreachable!()
    }
    fn rider_shapes(&mut self, _: u32, _: &Unit, _: &mut [HitShape]) -> usize {
        unreachable!()
    }
    fn held_shapes(&mut self, _: u32, _: &Unit, _: &mut [HitShape]) -> usize {
        unreachable!()
    }
    fn target_alive(&mut self, _: u32) -> bool {
        unreachable!()
    }
    fn target_dead(&mut self, _: u32) -> bool {
        unreachable!()
    }
    fn target_sphere(&mut self, _: u32) -> Bounds {
        unreachable!()
    }
    fn move_soldier(&mut self, _: u32, _: &mut Unit, _: f64) {
        unreachable!()
    }
    fn damage(&mut self, _: u32, _: &mut Unit, _: f64) {
        unreachable!()
    }
    fn aim(&mut self, _: u32, _: &mut Unit, _: f64) {
        unreachable!()
    }
    fn has_remote_step(&mut self, _: u32, _: &Unit) -> bool {
        unreachable!()
    }
    fn remote_step(&mut self, _: u32, _: &mut Unit, _: f64, _: bool) {
        unreachable!()
    }
    fn combat_reset(&mut self, _: u32, _: &mut Unit) {
        unreachable!()
    }
    fn combat_update(&mut self, _: u32, _: &mut Unit, _: f64, _: f64) {
        unreachable!()
    }
    fn flush_muzzles(&mut self, _: u32, _: &mut Unit) {
        unreachable!()
    }
    fn clear_charge(&mut self, _: u32, _: &mut Unit) {
        unreachable!()
    }
    fn let_go_of_character(&mut self, _: u32, _: &mut Unit) {
        unreachable!()
    }
    fn release(&mut self, _: u32, _: &Unit) {
        unreachable!()
    }
}
