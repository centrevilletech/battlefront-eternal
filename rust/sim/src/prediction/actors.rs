//! Private infantry and droideka replay against an explicitly supplied
//! collision world. Commands own their bodies, recoil, clips and caches;
//! no gameplay or JavaScript callback runs while collision state is borrowed.

use crate::collision::{
    self, broad,
    spheres::{self, GatherCache, Gathered},
    State, MISS, TERRAIN,
};
use crate::combat::combat::update_kick;
use crate::gen::{Combat, Unit, UnitBody};
use bfe_family::GameFamily;
use crate::movement::{
    advance_profiled, scene::{GroundCache, Scene},
    soldier::{swbf2::Step, Mover, PART_CACHES, V3},
};
use crate::pose::Clip;
use crate::units::{
    droideka::{self, DroidekaDef, DroidekaParts, ModeClips},
    shapes::{Bounds, HitShape},
    soldier::{load_body, load_class, profile_input, store_body},
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

/// The private body one client's replay steps: configured once per body
/// (`configure`), then stepped tick by tick (`step`). It holds no live store
/// references.
pub struct Session {
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
    /// Native joins admit collision geometry before opening their socket.
    /// Reserve the same pooled footprint as movement::reserve_mover; body
    /// switches reset these caches without discarding their backing storage.
    pub(crate) fn reserve_collision(&mut self,collision:&State,world:usize){
        let Some(w)=collision.worlds.get(world)else{return;};
        let statics=w.statics.map_or(0,|set|collision.sets[set].polygons.len());
        self.gathered.reserve(256,16);self.gathered.prepare_dense(collision,world,16);
        for cache in &mut self.caches{cache.reserve(statics,0,w.order.len());}
    }
    /// Configure the body a replay steps: its kind (0 infantry, 1 droideka);
    /// a droideka adds five (present, value) definition numbers, then fourteen
    /// clips (present; frames, fps, rootMotion, track count/tracks, Float32
    /// data count/data).
    pub fn configure(&mut self, input: &[f64]) -> Result<(), &'static str> {
        self.configure_for_family(input, self.family())
    }

    pub(crate) fn family(&self) -> GameFamily { self.mover.family() }

    pub fn configure_for_family(&mut self, input: &[f64], family: GameFamily) -> Result<(), &'static str> {
        let mut r = Reader { words: input, at: 0 };
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
        self.mover = Mover::for_family(0, family);
        Ok(())
    }

    /// InfantryPrediction.step / DroidekaPrediction.step on retained private
    /// state. A whole replay uses this entry without rows or foreign callbacks.
    pub(crate) fn step(&mut self, collision: &mut State, world: usize, ignored: i64,
        flags: &[u8], dt: f64, scratch: (f64, V3, bool), movement_backend: bool,
        shield: bool, underwater: bool, pressed: bool, airborne: bool, dive: bool, lock: f64,
        swbf2: Option<Step<'_>>,
    ) -> Result<bool, &'static str> {
        let can_fire = if self.kind == Some(0) {
            let UnitBody::Soldier(s) = &mut self.unit.soldier else {
                unreachable!()
            };
            let m = &mut self.mover;
            m.world = world;
            load_class(m, s);
            load_body(m, s, ignored);
            m.set_prediction_scratch(
                if movement_backend { 0.0 } else { scratch.0 },
                if movement_backend {
                    V3::ZERO
                } else {
                    scratch.1
                },
                if movement_backend { false } else { scratch.2 },
            );
            let profiled = swbf2.is_some();
            let inp = profile_input(&self.unit.input, profiled);
            let mut scene = Scene {
                s: collision,
                world,
                flags,
                gathered: &mut self.gathered,
                caches: &mut self.caches,
                ground_cache: &mut self.ground,
            };
            let permission = advance_profiled(m, &inp, dt, &mut scene, swbf2).map_err(|e| e.message())?;
            if profiled && dt == 0.0 { return Ok(permission); }
            store_body(s, m, dt);
            permission
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
            true
        };
        update_kick(&mut self.combat, &mut self.unit, dt);
        Ok(can_fire)
    }

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
        visit: &mut dyn FnMut(bool, f64, &[Vector3; 3]),
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
                visit(p.moving, p.id, &[t.a, t.b, t.c].map(|v| Vector3::new(v[0], v[1], v[2])));
            }
        }
    }
    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<(f64, Vector3)> {
        spheres::ground(self.collision, self.world, x, z, top).map(|(height, n)| (height, Vector3::new(n[0], n[1], n[2])))
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
