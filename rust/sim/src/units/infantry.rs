//! Cold class profiles and fixed-tick local infantry state.
mod blade;
pub mod bot_input;
pub mod screen;
mod solid;
mod throw;
mod weapon_input;
pub(super) use weapon_input::{mask_special_actions, offhand_power};
pub(super) use hero_powers::{input as power_input, kit_slots, pass as powers_pass};
pub(crate) use weapon_input::special_action;
pub mod contact;
pub mod hero_audio;
mod hero_powers;
use crate::{
    combat::combo::{self, ComboInput, ComboMachine, ComboState, WeaponMeleeSpec},
    gen::{MoveInput, Soldier},
    movement::{
        soldier::{
            swbf2::{
                classes::{self, control_speed, pairs},
                Actions, Roll, State, Step, Tumble, TumblePhase, Tuning, TurnScales,
            },
        },
        MovementOwner, ProfileError,
    },
    store::StrId,
    world::owner::World,
};
use bfe_content::{content::Content, defs::Defs, files::Files, Value};
use blade::{BladeHistory, BladePose, Root};
use contact::{Contacts, LifeId};
#[cfg(test)]
use crate::movement::soldier::{swbf2::{ControlSpeed, Jetpack, DEFAULT_GETUP_BACK_SECONDS, DEFAULT_GETUP_FRONT_SECONDS}, Jet};

#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub sprint: bool,
    pub roll: bool,
    pub block: bool,
    pub block_pressed: bool,
    pub hold_to_block: bool,
    pub offhand_pressed: bool,
    pub lock_pressed: bool,
    pub melee_held: u8,
    pub melee_pressed: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub id: u32,
    pub generation: f64,
    pub energy: f64,
    pub exhausted: bool,
    pub sprint: bool,
    pub sprint_time: Option<f64>,
    pub roll: Option<Roll>,
    /// Window used by the fixed-tick mover for this roll.
    pub roll_seconds: f64,
    pub jump_count: u32,
    /// Time left of the posture lock after a landing (no jump, crouch, sprint or roll).
    pub land_cooldown: f64,
    /// Active 2005 victim tumble phase, if any.
    pub tumble: Option<TumblePhase>,
    /// Full 2005 victim tumble state, so the client's look prediction can carry it
    /// through the commands the server has not applied yet.
    pub tumble_state: Option<Tumble>,
    pub combo: Option<u16>,
    pub state_time: f64,
    pub anim_time: f64,
    pub blades_off: u8,
    pub turn_factor: f64,
    /// Yaw the combo state turns the swing and the drawn body by, relative to the aim heading.
    pub swing_turn: f64,
    pub saber: Option<SaberSample>,
    /// Fixed-size melee combo state snapshot for per-command client look prediction.
    pub combo_state: Option<ComboState>,
    /// Armed melee button mask so client look prediction matches life edge gating.
    pub buttons_armed: u8,
    /// True while a thrown saber suppresses primary and secondary melee inputs.
    pub saber_locked: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SaberSample {
    pub thrown: Option<([f64; 3], f64)>,
    pub weapon_hash: u32,
    pub first_omitted_attack: Option<u32>,
    pub combo_hash: u32,
    pub reaction: Option<(usize, f64)>,
    pub state_hash: u32,
}

struct AttackFrame {
    life: LifeId,
    entered_tick: u32,
    outgoing: combo::StateId,
    entries: combo::StateEntries,
    before: Root,
    after: Root,
    start: f64,
    dt: f64,
    attacks: combo::ActiveAttackList,
}

impl AttackFrame {
    fn entry_tick(&self, state: combo::StateId, tick: u32) -> u32 {
        attack_origin(
            self.outgoing,
            self.entries.as_slice(),
            state,
            self.entered_tick,
            tick,
        )
    }
}

fn attack_origin(
    outgoing: combo::StateId,
    entries: &[combo::StateEntry],
    emitting: combo::StateId,
    previous: u32,
    tick: u32,
) -> u32 {
    if emitting != outgoing && entries.iter().any(|entry| entry.state == emitting) {
        tick
    } else {
        previous
    }
}

struct MeleeWeapon {
    weapon: u32,
    effects: contact::ContactEffects,
    state_hashes: Vec<u32>,
    machine: ComboMachine,
    spec: WeaponMeleeSpec,
    pose: BladePose,
    offhand: Option<throw::Throw>,
}

struct Hero {
    class: usize,
    history: BladeHistory,
    buttons: LifeButtons,
    flight: combo::SaberThrowState,
    throw_pressed: bool,
    throw_anim_start: Option<u32>,
    launch_due: Option<u32>,
    launched: u32,
    launch_origin: [f64; 3],
    ready_at: u32,
    frame: Option<AttackFrame>,
    entered_tick: u32,
    guard: Option<combo::DeflectDef>,
    input_lock: combo::InputLockMask,
    reaction: Option<(u32, usize, f64)>,
    state: ComboState,
    /// The settled combo state's turn of the swing and drawn body after the latest step.
    swing_turn: f64,
}

/// A held level establishes an edge only after a release or a press in this life.
#[derive(Default)]
struct LifeButtons {
    armed: u8,
}

impl LifeButtons {
    fn input(&mut self, controls: Controls, forward: f64, right: f64) -> ComboInput {
        let block = combo::ComboButton::FireSecondary.bit();
        let held = (controls.melee_held & !block) | if controls.block { block } else { 0 };
        let pressed =
            (controls.melee_pressed & !block) | if controls.block_pressed { block } else { 0 };
        self.armed |= !held | pressed;
        let mut input = ComboInput::default().stick(forward, right);
        input.buttons_pressed = pressed & !block;
        input.buttons_held = held & self.armed & !block;
        let pressed = pressed & block != 0;
        let held = held & self.armed & block != 0;
        if controls.hold_to_block {
            input.with_hold_to_block(pressed, held)
        } else {
            input.with_block_button(pressed, held)
        }
    }
}

impl Hero {
    fn new(class: usize, machine: &ComboMachine, tick: u32) -> Self {
        Self {
            class,
            history: Default::default(),
            buttons: Default::default(),
            flight: Default::default(),
            throw_pressed: false,
            throw_anim_start: None,
            launch_due: None,
            launched: 0,
            launch_origin: [0.; 3],
            ready_at: 0,
            frame: None,
            entered_tick: tick,
            guard: None,
            input_lock: combo::InputLockMask::NONE,
            reaction: None,
            state: ComboState::new(machine),
            swing_turn: 0.0,
        }
    }
}

struct Class {
    name: StrId,
    registry_id: Option<u16>,
    tuning: Tuning,
    acceleration: Option<f64>,
    /// Channel-1 non-power offhand weapons with `InstantPlayFireAnim`:
    /// `(arsenal_index, weapon_hash, fire_anim)`.
    offhand_guns: Vec<(usize, u32, u8)>,
}

#[derive(Clone, Copy)]
struct Life {
    slab: u32,
    class: usize,
    generation: f64,
    movement: State,
    allow_fire: bool,
    allow_reload: bool,
    offhand_pressed: bool,
    bot_secondary_held: Option<bool>,
    guard: Option<combo::DeflectDef>,
    lock_pressed: bool,
    power_input: crate::combat::powers::PowerInput,
    energy_before: f64,
    power_thrust: f64,
    blast_pending: bool,
    offhand_gun_salvo: bool,
    offhand_gun_anim: Option<(u32, u8, u32)>,
}

pub(crate) struct Infantry {
    power_kits: Vec<Option<u16>>,
    classes: Vec<Class>,
    lives: Vec<Option<Life>>,
    pub(super) bot_inputs: bot_input::BotInputs,
    pub mailbox: u32,
    player: u32,
    weapons: Vec<Option<MeleeWeapon>>,
    heroes: Vec<Option<Hero>>,
    sample: Option<Sample>,
    contacts: Contacts,
    pub(crate) strike: Option<contact::Strike>,
    bolts: Vec<Option<contact::ProjectileCredit>>,
    bullets: Vec<Option<contact::ProjectileCredit>>,
    observed: Vec<Option<(LifeId, u32)>>,
    /// The serial of the newest observed life. It saturates at `u32::MAX` (billions of respawns).
    life_serial: u32,
    pub(crate) tick: u32,
}

/// Resolved 2005 per-posture `ControlSpeed` turn multipliers from a class record.
pub fn turn_scales(record: &Value) -> Result<TurnScales, String> {
    let row = |mode| control_speed(record, mode).map(|c| c.turn).map_err(str::to_string);
    let scales = TurnScales {
        stand: row("stand")?,
        crouch: row("crouch")?,
        sprint: row("sprint")?,
        jump: row("jump")?,
        jet: row("jet")?,
        roll: row("roll")?,
        tumble: row("tumble")?,
    };
    if ![
        scales.stand,
        scales.crouch,
        scales.sprint,
        scales.jump,
        scales.jet,
        scales.roll,
        scales.tumble,
    ]
    .into_iter()
    .all(|v| v.is_finite() && v >= 0.0)
    {
        return Err("invalid ControlSpeed".into());
    }
    Ok(scales)
}

/// Resolved 2005 class movement tuning and optional melee combo state machine
/// used by client look preview to predict each pending command's turn scale.
#[derive(Clone)]
pub struct TurnModel {
    pub tuning: Tuning,
    pub max_speed: f64,
    pub machine: Option<ComboMachine>,
}

/// Resolve a 2005 soldier class's [`TurnModel`] from its prepared class record.
pub fn turn_model(
    record: &Value,
    side: &str,
    read_file: impl Fn(&str) -> Option<Vec<u8>>,
    roll_frames: u32,
    getup_front_seconds: f64,
    getup_back_seconds: f64,
    max_speed: f64,
) -> Result<TurnModel, String> {
    let tuning = classes::tuning(record, roll_frames, getup_front_seconds, getup_back_seconds)?;
    let machine = melee_machine(record, side, read_file)?;
    Ok(TurnModel {
        tuning,
        max_speed: v8math::max(max_speed, 0.0),
        machine,
    })
}

fn melee_machine(
    record: &Value,
    side: &str,
    read_file: impl Fn(&str) -> Option<Vec<u8>>,
) -> Result<Option<ComboMachine>, String> {
    let Some((_weapon, binding)) = bfe_pose::character::array(record, "weapons")
        .iter()
        .find_map(|weapon| {
            bfe_pose::character::array(weapon, "comboBindings")
                .iter()
                .find(|binding| binding.get("mode").and_then(Value::as_str) == Some("melee"))
                .map(|binding| (weapon, binding))
        })
    else {
        return Ok(None);
    };
    let hash = binding
        .get("combo")
        .and_then(Value::as_str)
        .ok_or("missing melee combo")?;
    let spec = WeaponMeleeSpec {
        combo_hash: u32::from_str_radix(hash, 16).map_err(|_| "invalid melee combo")?,
        ..Default::default()
    };
    let bytes = read_file(&format!("sides/{side}/combos.json")).ok_or("missing melee machines")?;
    let timing = binding
        .get("clipTiming")
        .ok_or("missing melee animation timings")?;
    let machine = ComboMachine::load_combo_by_hash(
        std::str::from_utf8(&bytes).map_err(|_| "invalid melee data")?,
        &spec,
        combo::COMBO_ANIM_FPS,
        |action| {
            let clip = timing.get(action)?;
            let fps = clip.get("fps")?.as_f64()?;
            Some((clip.get("frames")?.as_f64()? - 1.) / fps)
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(Some(machine))
}

/// The one look-rate rule for 2005 soldiers, used by the simulation's mouse look, by the
/// per-command predictor and by the client's fallback when it has no local infantry sample.
///
/// A thrown, tumbling or landed soldier turns at the class's tumble column and nothing
/// else scales it; any other soldier turns at the column of its posture and air state,
/// times the active melee combo's turn factor. The stand-up lock is the body's look lock,
/// not a rate.
pub fn look_turn_scale(
    scales: TurnScales,
    posture: crate::gen::Posture,
    air: Option<crate::gen::Air>,
    sprinting: bool,
    rolling: bool,
    tumbling: bool,
    combo_turn: f64,
) -> f64 {
    if tumbling || air == Some(crate::gen::Air::Thrown) {
        return scales.tumble;
    }
    scales.for_state(posture, air, sprinting, rolling) * combo_turn
}

/// Fixed-size per-command predictor of a 2005 soldier's turn multiplier
/// (`ControlSpeed` row x active combo `TurnFactor`). Zero heap allocations.
///
/// It does not run the mover: the horizontal velocity stays the last
/// correction's, and the mover's own tests on the speed along the facing
/// decide when a sprint starts or stops. Only for the start, while the stick
/// is held on the ground, the speed along the facing also rises each tick by
/// what it rose between the last two corrections (`velocity_change_xz`, zero
/// when unknown): the drive's ramp from rest crosses the start threshold
/// between corrections. A fall in speed (a wall) is never carried forward.
#[derive(Clone, Copy, Debug)]
pub struct TurnPredictor {
    pub posture: crate::gen::Posture,
    pub air: Option<crate::gen::Air>,
    pub grounded: bool,
    pub lock: f64,
    /// World X and Z velocity of the last correction.
    pub velocity_xz: [f64; 2],
    /// Change of `velocity_xz` per tick between the last two corrections.
    pub velocity_change_xz: [f64; 2],
    /// Rise of the speed along the facing carried forward so far.
    pub ramp: f64,
    pub velocity_y: f64,
    /// The soldier's movement state, with the tumble it is in carried along.
    pub movement: State,
    pub combo: Option<ComboState>,
    pub buttons_armed: u8,
    pub saber_locked: bool,
}

impl TurnPredictor {
    pub fn from_sample(
        posture: crate::gen::Posture,
        air: Option<crate::gen::Air>,
        velocity: [f64; 3],
        lock: f64,
        sample: &Sample,
    ) -> Self {
        let tumble = sample.tumble_state.or_else(|| {
            sample.tumble.map(|phase| {
                let mut t = Tumble::enter(crate::movement::soldier::V3::ZERO, 0.0, 1);
                t.phase = phase;
                t
            })
        });
        Self {
            posture,
            air,
            grounded: air.is_none()
                && !matches!(
                    tumble.map(|t| t.phase),
                    Some(TumblePhase::Thrown | TumblePhase::Recover)
                ),
            lock: v8math::max(lock, 0.0),
            velocity_xz: [velocity[0], velocity[2]],
            velocity_change_xz: [0.0, 0.0],
            ramp: 0.0,
            velocity_y: velocity[1],
            movement: State {
                energy: sample.energy,
                exhausted: sample.exhausted,
                sprint_time: sample.sprint_time,
                roll: sample.roll,
                land_cooldown: sample.land_cooldown,
                unsupported_time: None,
                jump_count: sample.jump_count,
                tumble,
            },
            combo: sample.combo_state,
            buttons_armed: sample.buttons_armed,
            saber_locked: sample.saber_locked,
        }
    }

    /// Turn-rate multiplier applied to mouse look at the start of the next tick.
    pub fn scale(&self, model: &TurnModel) -> f64 {
        let combo_turn = match (&model.machine, &self.combo) {
            (Some(machine), Some(state)) if self.movement.tumble.is_none() => {
                machine.active_turn_factor(state)
            }
            _ => 1.0,
        };
        look_turn_scale(
            model.tuning.turn_scales(),
            self.posture,
            self.air,
            self.movement.sprint_time.is_some(),
            self.movement.roll.is_some(),
            self.movement.is_tumble_look(),
            combo_turn,
        )
    }

    /// The mover's thrown-state drag on the velocity the predictor carries.
    fn apply_tumble_drag(&mut self, dt: f64) {
        let speed_sq = self.speed_sq() + self.velocity_y * self.velocity_y;
        if speed_sq > 0.0 {
            let speed = v8math::sqrt(speed_sq);
            let factor = v8math::exp(-crate::movement::soldier::swbf2::THROWN_AIR_DRAG * speed * dt);
            self.velocity_xz[0] *= factor;
            self.velocity_xz[1] *= factor;
            self.velocity_y *= factor;
        }
    }

    /// Slows the ground speed the way the mover's tumble and slide phases do.
    fn apply_ground_friction(&mut self, friction: f64, dt: f64) {
        let damp = v8math::max(0.0, 1.0 - friction * dt);
        self.velocity_xz[0] *= damp;
        self.velocity_xz[1] *= damp;
        if self.grounded {
            self.velocity_y = 0.0;
        } else {
            self.velocity_y -= crate::movement::soldier::GRAVITY * dt;
        }
    }

    /// One fixed tick of the victim tumble, with the phase changes the mover makes
    /// without touching the ground: a thrown soldier flipping upright after the recover
    /// delay, a tumble ending in a slide, a slide ending in the stand-up, the stand-up's
    /// lock running out. A touchdown needs the ground and is not predicted: the next
    /// correction brings it.
    fn step_tumble(&mut self, tuning: &Tuning, mut t: Tumble, dt: f64) -> Option<Tumble> {
        use crate::gen::{Air as GenAir, Posture as GenPosture};
        use crate::movement::soldier::swbf2;

        match t.phase {
            TumblePhase::Thrown => {
                self.posture = GenPosture::Stand;
                self.air = Some(GenAir::Thrown);
                self.apply_tumble_drag(dt);
                self.velocity_y -= crate::movement::soldier::GRAVITY * dt;
                t.elapsed += dt;
                t.air_time += dt;
                if !t.dead
                    && tuning.recover_from_tumble
                    && t.elapsed > swbf2::THROWN_RECOVER_DELAY
                    && !self.grounded
                {
                    t.phase = TumblePhase::Recover;
                    t.elapsed = 0.0;
                    t.angle = 0.0;
                    self.air = Some(GenAir::Fall);
                }
                Some(t)
            }
            TumblePhase::Tumble => {
                self.posture = GenPosture::Crouch;
                self.air = None;
                self.apply_tumble_drag(dt);
                self.apply_ground_friction(swbf2::TUMBLE_FRICTION, dt);
                let speed_sq = self.speed_sq();
                let speed = if speed_sq > 0.0 { v8math::sqrt(speed_sq) } else { 0.0 };
                t.elapsed += dt;
                t.cycle_phase = swbf2::wrap_unit(
                    t.cycle_phase + dt * (speed / swbf2::TUMBLE_CLIP_SPEED) / swbf2::TUMBLE_CLIP_DURATION,
                );
                if t.elapsed > swbf2::TUMBLE_MIN_TIME
                    && (speed_sq < swbf2::TUMBLE_END_SPEED_SQ || t.elapsed >= swbf2::TUMBLE_MAX_TIME)
                {
                    if t.cycle_phase > 0.40 && t.cycle_phase < 0.90 {
                        t.front = !t.front;
                    }
                    t.bounce = false;
                    t.phase = TumblePhase::Landed;
                    t.elapsed = 0.0;
                }
                Some(t)
            }
            TumblePhase::Landed => {
                self.posture = GenPosture::Crouch;
                self.air = None;
                self.apply_tumble_drag(dt);
                self.apply_ground_friction(swbf2::LANDED_FRICTION, dt);
                t.elapsed += dt;
                if self.grounded
                    && (self.speed_sq() < swbf2::LANDED_STOP_SPEED_SQ || t.elapsed > swbf2::LANDED_MAX_TIME)
                {
                    self.velocity_xz = [0.0, 0.0];
                    if !t.dead {
                        t.phase = TumblePhase::GetUp;
                        t.elapsed = 0.0;
                        self.lock = if t.front {
                            tuning.getup_front_seconds
                        } else {
                            tuning.getup_back_seconds
                        };
                        self.posture = GenPosture::Stand;
                    }
                }
                Some(t)
            }
            TumblePhase::GetUp => {
                if t.dead {
                    t.phase = TumblePhase::Landed;
                    t.elapsed = 0.0;
                    self.lock = 0.0;
                    self.posture = GenPosture::Crouch;
                    return Some(t);
                }
                self.posture = GenPosture::Stand;
                self.air = None;
                if self.grounded {
                    self.velocity_xz = [0.0, 0.0];
                    self.velocity_y = 0.0;
                } else {
                    self.velocity_y -= crate::movement::soldier::GRAVITY * dt;
                }
                t.elapsed += dt;
                let duration = if t.front {
                    tuning.getup_front_seconds
                } else {
                    tuning.getup_back_seconds
                };
                if t.elapsed >= duration {
                    self.lock = 0.0;
                    None
                } else {
                    self.lock = v8math::max(duration - t.elapsed, 0.0);
                    Some(t)
                }
            }
            TumblePhase::Recover => {
                self.posture = GenPosture::Stand;
                self.air = Some(GenAir::Fall);
                self.apply_tumble_drag(dt);
                self.velocity_y -= crate::movement::soldier::GRAVITY * dt;
                t.elapsed += dt;
                if t.elapsed >= swbf2::THROWN_RECOVER_DURATION {
                    None
                } else {
                    Some(t)
                }
            }
        }
    }

    /// Advance combo state, posture, sprint, roll and stamina by one fixed tick.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        model: &TurnModel,
        forward: f64,
        right: f64,
        jump: bool,
        crouch: bool,
        controls: Controls,
        aim_yaw: f64,
        dt: f64,
    ) {
        use crate::gen::{Air as GenAir, Posture as GenPosture};
        use crate::movement::soldier::{Air as MoveAir, Posture as MovePosture, swbf2::RollDirection};

        // The landing lock runs down every tick, in a tumble too, and blocks jump, crouch,
        // sprint and roll while it lasts.
        let land_cooling = self.movement.land_cooldown > 0.0;
        self.movement.land_cooldown = v8math::max(self.movement.land_cooldown - dt, 0.0);
        if self.movement.land_cooldown <= 1e-9 {
            self.movement.land_cooldown = 0.0;
        }
        if self.air == Some(GenAir::Thrown) && self.movement.tumble.is_none() {
            self.movement.tumble = Some(Tumble::enter(
                crate::movement::soldier::V3::new(
                    v8math::sqrt(v8math::max(self.speed_sq(), 0.0)),
                    self.velocity_y,
                    0.0,
                ),
                0.0,
                1,
            ));
        }
        if let Some(tumble) = self.movement.tumble {
            self.movement.roll = None;
            self.movement.sprint_time = None;
            self.movement.tumble = self.step_tumble(&model.tuning, tumble, dt);
            return;
        }

        let move_posture = match self.posture {
            GenPosture::Stand => MovePosture::Stand,
            GenPosture::Crouch => MovePosture::Crouch,
            GenPosture::Prone => MovePosture::Prone,
        };
        let move_air = match self.air {
            None => MoveAir::None,
            Some(GenAir::Jump) => MoveAir::Jump,
            Some(GenAir::Fall) => MoveAir::Fall,
            Some(GenAir::Jet) => MoveAir::Jet,
            Some(GenAir::Thrown) => MoveAir::Thrown,
        };
        let mut eff_tuning = model.tuning;
        let mut eff_actions = Actions {
            sprint: controls.sprint,
            roll: controls.roll,
            ai: false,
        };
        let mut eff_forward = forward;
        let mut eff_right = right;
        let mut eff_jump = jump;
        let mut eff_crouch = crouch;
        let mut force_jump = false;
        let mut force_roll = false;
        let mut energy_override = false;

        let moving = self.speed_sq() >= 1.0;
        if let (Some(machine), Some(combo_state)) = (&model.machine, &mut self.combo) {
            let body = combo::BodyContext {
                posture: combo::ComboPosture::from_movement(move_posture, move_air, &self.movement),
                aim_yaw: 0.0,
                velocity_y: self.velocity_y,
                grounded: self.grounded,
                moving,
            };
            let mut buttons = LifeButtons {
                armed: self.buttons_armed,
            };
            let mut cmd = buttons.input(controls, forward, right);
            self.buttons_armed = buttons.armed;
            if self.saber_locked {
                let fire = combo::ComboButton::Fire.bit() | combo::ComboButton::FireSecondary.bit();
                cmd.buttons_held &= !fire;
                cmd.buttons_pressed &= !fire;
            }
            let out = combo::step(
                bfe_family::GameFamily::Swbf2,
                machine,
                combo_state,
                &cmd,
                &body,
                &mut self.movement,
                &model.tuning,
                false,
                dt,
            );
            if out.active {
                let (t, a) = combo::prepare_movement_step(&model.tuning, eff_actions, &out);
                eff_tuning = t;
                eff_actions = a;
                let mut lock_bits = out.input_lock.0;
                if let Some(p @ (combo::ComboPosture::Stand | combo::ComboPosture::Crouch)) =
                    out.posture_override
                {
                    eff_crouch = (p == combo::ComboPosture::Crouch)
                        != (self.posture == GenPosture::Crouch);
                    lock_bits &= !combo::InputLockMask::CROUCH.0;
                }
                let mask = combo::InputLockMask(lock_bits);
                if mask.contains(combo::InputLockMask::THRUST) {
                    eff_forward = 0.0;
                }
                if mask.contains(combo::InputLockMask::STRAFE) {
                    eff_right = 0.0;
                }
                if mask.contains(combo::InputLockMask::JUMP) {
                    eff_jump = false;
                }
                if mask.contains(combo::InputLockMask::CROUCH) {
                    eff_crouch = false;
                    eff_actions.roll = false;
                }
                if mask.contains(combo::InputLockMask::SPRINT) {
                    eff_actions.sprint = false;
                }
                force_jump = out.posture_override == Some(combo::ComboPosture::Jump);
                force_roll = out.posture_override == Some(combo::ComboPosture::Roll);
                energy_override = out.energy_override_active;
            }
        }

        let locked = self.lock > 0.0;
        self.lock = v8math::max(self.lock - dt, 0.0);
        // The mover ends a lock that a tick's subtraction leaves a rounding error short of zero.
        if self.lock <= 1e-9 {
            self.lock = 0.0;
        }
        let on_foot = !locked && self.grounded && self.air.is_none();
        let f = v8math::max(-1.0, v8math::min(1.0, eff_forward));
        let s = v8math::max(-1.0, v8math::min(1.0, eff_right));
        let directed = f * f + s * s >= 0.81;
        let stick_roll =
            !land_cooling && eff_actions.roll && directed && (f > 0.0 || v8math::abs(s) > -0.839 * f);
        let try_roll = on_foot && self.movement.roll.is_none() && (force_roll || stick_roll);
        if try_roll
            && eff_tuning.roll_speed_factor > 0.0
            && eff_tuning.roll_frames > 0
            && self.movement.charge(eff_tuning.energy_cost_roll, &eff_tuning)
        {
            let direction = if f > 0.0 && v8math::abs(s) <= f {
                RollDirection::Forward
            } else if s < 0.0 {
                RollDirection::Left
            } else {
                RollDirection::Right
            };
            self.posture = GenPosture::Crouch;
            self.movement.roll = Some(Roll {
                direction,
                elapsed: 0.0,
            });
        }
        if let Some(mut roll) = self.movement.roll {
            self.movement.sprint_time = None;
            roll.elapsed += dt;
            if roll.elapsed > eff_tuning.roll_seconds() {
                self.posture = GenPosture::Stand;
                self.movement.roll = None;
            } else {
                self.movement.roll = Some(roll);
            }
            return;
        }

        let free = on_foot && !try_roll;
        let continuing = self.movement.sprint_time.is_some();
        let (sin, cos) = v8math::sin_cos(aim_yaw);
        let along = self.velocity_xz[0] * sin + self.velocity_xz[1] * cos;
        let rise = v8math::max(self.velocity_change_xz[0] * sin + self.velocity_change_xz[1] * cos, 0.0);
        let jump_requested = (!land_cooling && eff_jump) || force_jump;
        let change_stance = free
            && !continuing
            && ((!land_cooling && eff_crouch && !force_jump)
                || (self.posture == GenPosture::Crouch && jump_requested));
        let mut jumping = false;
        if change_stance {
            self.posture = if self.posture == GenPosture::Crouch {
                GenPosture::Stand
            } else {
                GenPosture::Crouch
            };
        } else if free && jump_requested && self.posture == GenPosture::Stand {
            let cost = if continuing {
                v8math::max(
                    eff_tuning.energy_cost_jump,
                    eff_tuning.energy_cost_jump_sprint.unwrap_or(0.0),
                )
            } else {
                eff_tuning.energy_cost_jump
            };
            jumping = self.movement.charge(cost, &eff_tuning);
            if jumping {
                self.grounded = false;
                self.air = Some(GenAir::Jump);
                self.velocity_y += (2.0 * crate::movement::soldier::GRAVITY * eff_tuning.jump_height).sqrt();
            }
        }

        let sprinting = if try_roll {
            continuing
        } else {
            free && !land_cooling && !change_stance
                && !jumping
                && self.posture == GenPosture::Stand
                && eff_actions.sprint
                && !self.movement.exhausted
                && eff_tuning.sprint.forward > eff_tuning.stand.forward
                && directed
                && f > 0.0
                && if continuing {
                    v8math::abs(s) < 1.732 * f && along > 0.2
                } else {
                    v8math::abs(s) < 0.577 * f && along + self.ramp > 0.9 * model.max_speed
                }
        };
        if sprinting {
            let time = v8math::min(
                self.movement.sprint_time.unwrap_or(0.0) + dt,
                eff_tuning.sprint_accelerate_time,
            );
            self.movement.sprint_time = Some(time);
            self.movement
                .charge(eff_tuning.energy_drain_sprint * dt, &eff_tuning);
        } else {
            self.movement.sprint_time = None;
            if !energy_override && self.grounded && self.air.is_none() {
                let rate = if self.speed_sq() < 1.0 {
                    eff_tuning.energy_restore_idle
                } else {
                    eff_tuning.energy_restore
                };
                self.movement.recover(rate * dt, &eff_tuning);
            }
        }
        if !self.grounded || self.air.is_some() || self.lock > 0.0 {
            self.movement.sprint_time = None;
            self.velocity_y -= crate::movement::soldier::GRAVITY * dt;
            self.ramp = 0.0;
        } else if f * f + s * s > 0.01 {
            self.ramp += rise;
        } else {
            self.ramp = 0.0;
        }
    }

    fn speed_sq(&self) -> f64 {
        self.velocity_xz[0] * self.velocity_xz[0] + self.velocity_xz[1] * self.velocity_xz[1]
    }
}

/// The contact effects a class's data names for its melee weapon and its thrown blade.
fn contact_effects(record: &Value, strings: &mut crate::store::Strings) -> contact::ContactEffects {
    let weapons = bfe_pose::character::array(record, "weapons");
    let melee = weapons.iter().find(|weapon| {
        bfe_pose::character::array(weapon, "comboBindings")
            .iter()
            .any(|binding| binding.get("mode").and_then(Value::as_str) == Some("melee"))
    });
    let thrown = weapons
        .iter()
        .find(|w| w.get("base").and_then(Value::as_str) == Some("meleethrow"))
        .and_then(|w| w.get("ordnance"));
    let mut name = |holder: Option<&Value>, key: &str| {
        holder?
            .get(key)
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .map(|n| strings.intern_str(n))
    };
    contact::ContactEffects {
        hit: name(melee, "HitEffect"),
        deflect: name(melee, "DeflectEffect"),
        thrown_hit: name(thrown, "ImpactEffect"),
    }
}

fn melee(
    content: &Content,
    name: &str,
    record: &Value,
    side: &str,
    files: &dyn Files,
) -> Result<Option<MeleeWeapon>, String> {
    let Some((weapon, binding)) = bfe_pose::character::array(record, "weapons")
        .iter()
        .find_map(|weapon| {
            bfe_pose::character::array(weapon, "comboBindings")
                .iter()
                .find(|binding| binding.get("mode").and_then(Value::as_str) == Some("melee"))
                .map(|binding| (weapon, binding))
        })
    else {
        return Ok(None);
    };
    let hash = binding
        .get("combo")
        .and_then(Value::as_str)
        .ok_or("missing melee combo")?;
    let mut spec = WeaponMeleeSpec {
        combo_hash: u32::from_str_radix(hash, 16).map_err(|_| "invalid melee combo")?,
        ..Default::default()
    };
    if let Some(n) = pairs(weapon).find(|(key, _)| key.eq_ignore_ascii_case("NumDamageEdges")) {
        spec.num_edges = n.1.parse().map_err(|_| "invalid melee edge count")?;
    }
    for (slot, key) in [
        "PersonScale",
        "AnimalScale",
        "DroidScale",
        "VehicleScale",
        "BuildingScale",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some((_, v)) = pairs(weapon)
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            spec.damage_scales[slot] = v.parse().map_err(|_| format!("invalid {key}"))?;
        }
    }
    if let Some((_, v)) = pairs(weapon)
        .rev()
        .find(|(k, _)| k.eq_ignore_ascii_case("ShieldScale"))
    {
        spec.shield_scale = v.parse().map_err(|_| "invalid shield scale")?;
    }
    let blades = bfe_pose::character::array(weapon, "saberHilts")
        .iter()
        .flat_map(|h| bfe_pose::character::array(h, "blades"));
    for (i, blade) in blades.enumerate() {
        let edge = spec.edges.get_mut(i).ok_or("too many blade sockets")?;
        for (field, key, out) in [
            ("length", "LightSaberLength", &mut edge.default_length),
            ("width", "LightSaberWidth", &mut edge.default_width),
        ] {
            if let Some(value) = blade.get(field).and_then(Value::as_f64) {
                *out = value;
            } else if let Some((_, v)) = pairs(weapon)
                .filter(|(k, _)| k.eq_ignore_ascii_case(key))
                .nth(i)
                .or_else(|| pairs(weapon).find(|(k, _)| k.eq_ignore_ascii_case(key)))
            {
                *out = v.parse().map_err(|_| format!("invalid blade {field}"))?;
            }
        }
    }
    let mut edge: usize = 0;
    for (key, value) in pairs(weapon) {
        if key.eq_ignore_ascii_case("FirePointName")
            || key.eq_ignore_ascii_case("AttachedFirePoint")
        {
            edge += 1;
        } else if let Some(slot) = edge
            .checked_sub(1_usize)
            .and_then(|i| spec.edges.get_mut(i))
        {
            if key.eq_ignore_ascii_case("DamageEdgeLength") {
                slot.default_length = value.parse().map_err(|_| "invalid damage edge length")?;
            } else if key.eq_ignore_ascii_case("DamageEdgeWidth") {
                slot.default_width = value.parse().map_err(|_| "invalid damage edge width")?;
            }
        }
    }
    let bytes = files
        .read(&format!("sides/{side}/combos.json"))
        .ok_or("missing melee machines")?;
    let timing = binding
        .get("clipTiming")
        .ok_or("missing melee animation timings")?;
    let machine = ComboMachine::load_combo_by_hash(
        std::str::from_utf8(&bytes).map_err(|_| "invalid melee data")?,
        &spec,
        combo::COMBO_ANIM_FPS,
        |action| {
            let clip = timing.get(action)?;
            let fps = clip.get("fps")?.as_f64()?;
            Some((clip.get("frames")?.as_f64()? - 1.) / fps)
        },
    )
    .map_err(|e| e.to_string())?;
    let offhand = throw::bind(record, &spec)?;
    let pose = BladePose::bind(
        content,
        name,
        side,
        record,
        weapon,
        binding,
        &machine,
        offhand.as_ref().map(|t| t.windup_secs),
    )?;
    let name = weapon
        .get("class")
        .and_then(Value::as_str)
        .ok_or("missing saber weapon class")?;
    let state_hashes = machine
        .state_names
        .iter()
        .map(|s| combo::zero_fnv1a_lower(s))
        .collect();
    Ok(Some(MeleeWeapon {
        offhand,
        effects: contact::ContactEffects::default(),
        weapon: combo::zero_fnv1a_lower(name),
        state_hashes,
        machine,
        spec,
        pose,
    }))
}

pub(crate) fn admit(
    world: &mut World,
    content: &Content,
    defs: &Defs,
    files: &dyn Files,
    mailbox: u32,
    player: u32,
) -> Result<(), String> {
    let mut classes = Vec::new();
    let mut class_names: Vec<String> = Vec::new();
    let mut weapons = Vec::new();
    for class in &defs.classes {
        // The one rule for a class's movement numbers: the admission registers
        // them by it and a client predicts with them.
        let Some(tuning) = classes::of_class(content, defs, class) else {
            continue;
        };
        let name = defs.names.text(class.name);
        let side = defs.names.text(class.side);
        let tuning = match tuning {
            Ok(t) => t,
            Err(e) if name == content.battle.policy.class_name => {
                return Err(format!("{name}: {e}"))
            }
            Err(_) => continue,
        };
        let class_id = world.sim.strings.intern_str(name);
        let mut melee_weapon =
            melee(content, name, &class.record, side, files).map_err(|e| format!("{side}/{name}: {e}"))?;
        if let Some(weapon) = &mut melee_weapon {
            weapon.effects = contact_effects(&class.record, &mut world.sim.strings);
        }
        weapons.push(melee_weapon);
        let acceleration = pairs(&class.record)
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("Acceleration"))
            .and_then(|(_, v)| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.);
        let mut offhand_guns = Vec::new();
        for (arsenal_idx, w) in class
            .record
            .get("weapons")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let channel = w.get("channel").and_then(Value::as_f64).unwrap_or(0.);
            if channel != 1. {
                continue;
            }
            if crate::combat::powers::from_weapon(w).ok().flatten().is_some() {
                continue;
            }
            let base = w.get("base").and_then(Value::as_str).unwrap_or("");
            if matches!(base, "melee" | "meleethrow" | "areaeffectweapon") {
                continue;
            }
            let num = |k: &str| {
                w.get(k)
                    .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(0.)
            };
            if num("OffhandWeapon") != 0. && num("InstantPlayFireAnim") != 0. {
                if let Some(wname) = w.get("class").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    let fire_anim = num("FireAnim").clamp(0., 3.) as u8;
                    offhand_guns.push((arsenal_idx, combo::zero_fnv1a_lower(wname), fire_anim));
                }
            }
        }
        class_names.push(name.to_owned());
        classes.push(Class {
            name: class_id,
            registry_id: class.registry_id,
            tuning,
            acceleration,
            offhand_guns,
        });
    }
    // A body made later, for a player who joins, has its rows too: as many as
    // the battle's bodies can ever be (admission's census holds them to it).
    let bodies = crate::wire::bounds::Capacity::for_battle(bfe_family::GameFamily::Swbf2).slots;
    let capacity = super::f3_construction::reserved_slots(&world.units)
        .max(world.sim.store.unit.slots() as usize)
        .max(bodies);
    // Hero powers: each soldier class's record and the side explosion records.
    // A battle whose records the powers refuse plays without them.
    let power_classes: Vec<(String, Value)> = defs.classes.iter()
        .filter(|c| c.record.get("base").and_then(Value::as_str) == Some("soldier"))
        .map(|c| (defs.names.text(c.name).to_owned(), c.record.clone())).collect();
    let explosions: Vec<Value> = ["all", "cis", "imp", "rep"].iter()
        .filter_map(|side| files.read(&format!("sides/{side}/classes.json")))
        .filter_map(|bytes| bfe_content::json::parse(std::str::from_utf8(&bytes).ok()?).ok()).collect();
    if let Err(e) = crate::combat::powers::install_in(world, capacity + 1, power_classes.iter().map(|(n, r)| (n.as_str(), r)),
        |name| hero_powers::explosion_record(&explosions, name)) {
        eprintln!("hero powers refused for this battle: {e}");
    }
    let power_kits = class_names.iter().map(|n| crate::combat::powers::powers_in(world).and_then(|p| p.kit_of(n))).collect::<Vec<_>>();
    world.units.infantry = Some(Box::new(Infantry {
        power_kits,
        classes,
        lives: vec![None; capacity + 1],
        bot_inputs: bot_input::BotInputs::new(capacity + 1),
        mailbox,
        player,
        weapons,
        heroes: (0..=capacity).map(|_| None).collect(),
        strike: None,
        bolts: vec![None; world.sim.store.ordnance.bolts.capacity()],
        bullets: vec![None; world.sim.store.ordnance.rounds.capacity()],
        sample: None,
        contacts: Contacts::default(),
        observed: vec![None; capacity + 1],
        life_serial: 0,
        tick: 0,
    }));
    let m = world
        .input
        .mailboxes
        .get_mut(mailbox as usize)
        .ok_or("missing local input mailbox")?;
    m.enable_infantry();
    world.register_bot_melee_driver(Box::new(bot_input::stage_goal_frame));
    // Soldiers admitted with the battle are targets from its first tick.
    contact::observe_lives(world);
    Ok(())
}

impl World {
    pub fn select_local_spawn_path(&mut self, path: String) {
        self.units.spawn_path = Some(path);
    }

    pub fn local_infantry_input(&mut self, sequence: f64, input: Controls) {
        if let Some(battle) = &self.units.infantry {
            let Some(player) = self
                .input
                .players
                .pool
                .get(battle.player as usize)
                .filter(|p| !p.spectator && p.disconnected_at.is_none())
            else {
                return;
            };
            let Some(character) = &player.character else {
                return;
            };
            let Some(r) = crate::battle::f4_connections::wire_view_in(
                &self.battle,
                &self.sim.store,
                character,
            )
            .and_then(|connection| connection.unit) else {
                return;
            };
            let Some(unit) = self
                .sim
                .store
                .unit
                .get(r)
                .filter(|u| u.human && u.alive && !u.dead && !u.removed && u.seat.is_none())
            else {
                return;
            };
            let life = contact::identity(r, unit);
            if let Some(mailbox) = self.input.mailboxes.get_mut(battle.mailbox as usize) {
                mailbox.queue_infantry(sequence, life, input);
            }
        }
    }

    pub fn local_infantry_sample(&self) -> Option<Sample> {
        let infantry = self.units.infantry.as_ref()?;
        let mut sample = infantry.sample?;
        if let (Some(saber), Some((at, clip, duration))) = (
            &mut sample.saber,
            infantry
                .heroes
                .get(sample.id as usize)?
                .as_ref()
                .and_then(|h| h.reaction),
        ) {
            let time = infantry.tick.saturating_sub(at) as f64 / 60.;
            if time < duration {
                saber.reaction = Some((clip, time));
            }
        }
        Some(sample)
    }

    /// Populate `dest` with every 2005 soldier's active power caster animation,
    /// instant-play offhand gun animation, choke hold and the bounded power log.
    /// No-op on 2004 battles (which never install `units.infantry`).
    pub fn infantry_power_frame(
        &self,
        dest: &mut Option<Box<crate::combat::powers::PowerFrame>>,
    ) {
        let Some(infantry) = self.units.infantry.as_ref() else {
            return;
        };
        let out = dest.get_or_insert_with(|| {
            Box::new(crate::combat::powers::PowerFrame::with_capacity(
                infantry.lives.len().max(64),
            ))
        });
        let now = infantry.tick as u32;
        if let Some(powers) = crate::combat::powers::powers_in(self) {
            powers.fill_frame(self, out);
        } else {
            out.tick = now;
            out.clear();
        }
        for (slot, life) in infantry.lives.iter().enumerate() {
            let Some(life) = life else { continue };
            let Some((_, u)) = self.sim.store.unit.at(slot as u32) else {
                continue;
            };
            if u.removed || u.generation != life.generation {
                continue;
            }
            let alive_unit = u.alive && !u.dead;
            if alive_unit {
                if let Some((weapon, fire_anim, start_tick)) = life.offhand_gun_anim {
                    let elapsed_ticks = infantry.tick.saturating_sub(start_tick);
                    if let Some(existing) = out
                        .units
                        .iter_mut()
                        .find(|p| p.slot == slot as u32 && p.generation == life.generation)
                    {
                        if existing.weapon == 0 {
                            existing.weapon = weapon;
                            existing.fire_anim = fire_anim;
                            existing.elapsed_ticks = elapsed_ticks;
                        }
                    } else {
                        out.units.push(crate::combat::powers::UnitPowerPose {
                            slot: slot as u32,
                            generation: life.generation,
                            weapon,
                            fire_anim,
                            elapsed_ticks,
                            ..crate::combat::powers::UnitPowerPose::default()
                        });
                    }
                }
            }
            if let Some(tumble) = life.movement.tumble {
                let phase = if alive_unit {
                    tumble.phase as u8
                } else {
                    6
                };
                let flags = u8::from(tumble.front)
                    | (u8::from(tumble.bounce) << 1)
                    | ((tumble.fly_dir & 3) << 2);
                let elapsed = tumble.elapsed as f32;
                let angle = if phase == TumblePhase::Thrown as u8 {
                    tumble.angle as f32
                } else {
                    0.0
                };
                let heading = tumble.heading_offset as f32;
                if let Some(existing) = out
                    .units
                    .iter_mut()
                    .find(|p| p.slot == slot as u32 && p.generation == life.generation)
                {
                    existing.tumble_phase = phase;
                    existing.tumble_flags = flags;
                    existing.tumble_elapsed = elapsed;
                    existing.tumble_angle = angle;
                    existing.tumble_heading = heading;
                } else {
                    out.units.push(crate::combat::powers::UnitPowerPose {
                        slot: slot as u32,
                        generation: life.generation,
                        tumble_phase: phase,
                        tumble_flags: flags,
                        tumble_elapsed: elapsed,
                        tumble_angle: angle,
                        tumble_heading: heading,
                        ..crate::combat::powers::UnitPowerPose::default()
                    });
                }
            }
        }
    }
}

impl Infantry {
    /// Visit every living, unseated melee soldier's pose at the completed tick `tick`:
    /// its combo state, the tick its clip clock read zero, the state's loop and clip,
    /// its blade mask and its guard reaction (the same fields the local player's own
    /// sample carries).
    pub(crate) fn melee_poses(
        &self,
        store: &crate::gen::Store,
        tick: f64,
        mut visit: impl FnMut(u32, f64, crate::wire::melee::MeleePose),
    ) {
        for slot in 0..self.heroes.len() {
            let Some(generation) = self.unseated_life(store, slot) else { continue };
            if let Some(pose) = self.melee_pose(slot, tick) {
                visit(slot as u32, generation, pose);
            }
        }
    }

    /// Every living, unseated hero that carries a throw: when its throw animation began and,
    /// while the blade is out, what a viewer needs to replay the flight.
    pub(crate) fn throws(
        &self,
        store: &crate::gen::Store,
        wire_tick: f64,
        mut each: impl FnMut(u32, f64, crate::wire::thrown::Throw),
    ) {
        // The ticks of a throw are sent in the numbering of the publication.
        let shift = wire_tick - self.tick as f64;
        for (slot, hero) in self.heroes.iter().enumerate() {
            let (Some(hero), Some(life)) = (hero, self.lives.get(slot).and_then(Option::as_ref)) else {
                continue;
            };
            let Some((_, u)) = store.unit.at(slot as u32) else {
                continue;
            };
            if !u.alive || u.dead || u.removed || u.seat.is_some() || u.generation != life.generation {
                continue;
            }
            let Some(weapon) = self.weapons.get(hero.class).and_then(Option::as_ref) else {
                continue;
            };
            if let Some(mut throw) = hero.published_throw(weapon) {
                throw.press = throw.press.map(|t| t + shift);
                if let Some(launch) = &mut throw.launch {
                    launch.tick += shift;
                }
                each(slot as u32, life.generation, throw);
            }
        }
    }

    /// The life of the living, unseated soldier in `slot`.
    fn unseated_life(&self, store: &crate::gen::Store, slot: usize) -> Option<f64> {
        let (r, u) = store.unit.at(slot as u32)?;
        let life = self.lives.get(slot).and_then(Option::as_ref)?;
        (u.alive && !u.dead && !u.removed && u.seat.is_none() && life.slab == r.gen && life.generation == u.generation)
            .then_some(life.generation)
    }

    /// The melee pose of the saber wielder in `slot`.
    fn melee_pose(&self, slot: usize, tick: f64) -> Option<crate::wire::melee::MeleePose> {
        let hero = self.heroes.get(slot)?.as_ref()?;
        let weapon = self.weapons.get(hero.class).and_then(Option::as_ref)?;
        let state = weapon.machine.state(hero.state.state);
        let clip_len = state
            .animation
            .and_then(|clip| weapon.machine.animations.get(clip.0 as usize))
            .map_or(0., |meta| meta.duration_secs);
        let ago = |at| self.tick.saturating_sub(at) as f64;
        Some(crate::wire::melee::MeleePose {
            state: hero.state.state.0,
            origin: tick - hero.state.eval_time * 60.,
            loop_mode: state.loop_mode,
            clip_len,
            blades_off: hero.state.blades_off_mask,
            reaction: hero.reaction.and_then(|(at, clip, duration)| {
                Some((u16::try_from(clip).ok().filter(|&c| c != u16::MAX)?, tick - ago(at), duration))
            }),
            swing_turn: hero.swing_turn,
        })
    }

    /// Every living, unseated soldier's combat display: its melee pose, the pose of a
    /// power it casts or holds and the choke that holds it. A soldier with none of
    /// the three still has its (empty) display, so a part that ends is seen to end.
    pub(crate) fn hero_displays(
        &self,
        store: &crate::gen::Store,
        tick: f64,
        powers: Option<&crate::combat::powers::Powers>,
        effects: &crate::combat::powers::EffectTable,
        mut visit: impl FnMut(u32, f64, crate::wire::melee::Display),
    ) {
        use crate::combat::powers::LifeKey;
        use crate::wire::melee::{CastPose, Display};
        for slot in 0..self.lives.len() {
            let Some(generation) = self.unseated_life(store, slot) else { continue };
            let Some((r, u)) = store.unit.at(slot as u32) else { continue };
            let key = LifeKey::of(r, u);
            let cast = powers.and_then(|p| p.cast_pose(key)).map(|(weapon, fire_anim, elapsed, holding)| CastPose {
                weapon,
                fire_anim,
                origin: tick - f64::from(elapsed),
                holding,
            });
            let choked = effects.held(key).map(|h| h.anim);
            visit(slot as u32, generation, Display { melee: self.melee_pose(slot, tick), cast, choked });
        }
    }

    pub(super) fn offhand_edge(
        &mut self,
        slot: u32,
        generation: f64,
        human: bool,
        held: bool,
    ) -> bool {
        let Some(life) = self
            .lives
            .get_mut(slot as usize)
            .and_then(Option::as_mut)
            .filter(|life| life.generation == generation)
        else {
            return false;
        };
        if human {
            return std::mem::take(&mut life.offhand_pressed);
        }
        let previous = life.bot_secondary_held.replace(held);
        previous != Some(true) && held
    }

    pub(super) fn class_of(&self, slot: u32, generation: f64) -> Option<usize> {
        self.lives.get(slot as usize)?.as_ref().filter(|l| l.generation == generation).map(|l| l.class)
    }

    pub(super) fn set_power_input(&mut self, slot: u32, generation: f64, input: crate::combat::powers::PowerInput) {
        if let Some(l) = self.lives.get_mut(slot as usize).and_then(Option::as_mut).filter(|l| l.generation == generation) {
            l.power_input = crate::combat::powers::PowerInput { pressed: l.power_input.pressed || input.pressed, ..input };
        }
    }

    pub(super) fn request_throw(&mut self, slot: u32) {
        if let Some(Some(hero)) = self.heroes.get_mut(slot as usize) {
            hero.throw_pressed = true;
        }
    }

    pub(crate) fn melee_throw_reloading(&self, slot: u32, generation: f64) -> bool {
        let Some(life) = self.lives.get(slot as usize).and_then(Option::as_ref) else {
            return false;
        };
        if life.generation != generation {
            return false;
        }
        self.heroes.get(slot as usize).and_then(Option::as_ref).is_some_and(|hero| {
            !hero.flight.is_active() && hero.launch_due.is_none() && self.tick < hero.ready_at
        })
    }

    pub(crate) fn evasion_flags(&self, slot: u32, generation: f64) -> u8 {
        let Some(life) = self.lives.get(slot as usize).and_then(Option::as_ref) else {
            return 0;
        };
        if life.generation != generation {
            return 0;
        }
        (u8::from(life.movement.roll.is_some()) << 1)
            | (u8::from(life.movement.sprint_time.is_some()) << 2)
    }

    pub(super) fn blocks_fire(&self, id: u32, generation: f64) -> bool {
        self.lives
            .get(id as usize)
            .and_then(Option::as_ref)
            .is_some_and(|life| life.generation == generation && !life.allow_fire)
    }

    pub(super) fn blocks_reload(&self, id: u32, generation: f64) -> bool {
        self.lives
            .get(id as usize)
            .and_then(Option::as_ref)
            .is_some_and(|life| life.generation == generation && !life.allow_reload)
    }

    fn combo_turn_factor(&self, slot: u32, class_index: usize) -> f64 {
        let Some(hero) = self
            .heroes
            .get(slot as usize)
            .and_then(Option::as_ref)
            .filter(|h| h.class == class_index)
        else {
            return 1.0;
        };
        let Some(weapon) = self.weapons.get(class_index).and_then(Option::as_ref) else {
            return 1.0;
        };
        weapon.machine.active_turn_factor(&hero.state)
    }

    pub(super) fn mouse_turn_scale(
        &self,
        slot: u32,
        class_name: StrId,
        generation: f64,
        soldier: &Soldier,
    ) -> Option<f64> {
        let life = self
            .lives
            .get(slot as usize)
            .and_then(Option::as_ref)
            .filter(|l| l.generation == generation);
        let class_index = life
            .map(|l| l.class)
            .or_else(|| self.classes.iter().position(|c| c.name == class_name))?;
        let tuning = &self.classes[class_index].tuning;
        let sprinting = life.is_some_and(|l| l.movement.sprint_time.is_some());
        let rolling = life.is_some_and(|l| l.movement.roll.is_some());
        let tumbling = life.is_some_and(|l| l.movement.is_tumble_look());
        let combo_turn = if life.is_some_and(|l| l.movement.tumble.is_none()) {
            self.combo_turn_factor(slot, class_index)
        } else {
            1.0
        };
        Some(look_turn_scale(
            tuning.turn_scales(),
            soldier.posture,
            soldier.air,
            sprinting,
            rolling,
            tumbling,
            combo_turn,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn advance(
        &mut self,
        movement: &mut MovementOwner,
        collision: &mut crate::collision::State,
        id: u32,
        slab: u32,
        class_name: StrId,
        generation: f64,
        human: bool,
        alive: bool,
        controls: Controls,
        delivery: Option<bot_input::Delivery>,
        held: Option<crate::combat::powers::Held>,
        body: &mut Soldier,
        input: &MoveInput,
        dt: f64,
        world: usize,
        pass: i64,
    ) -> Result<bool, ProfileError> {
        let index = self
            .classes
            .iter()
            .position(|c| c.name == class_name)
            .ok_or(ProfileError::InvalidTuning)?;
        let class = &self.classes[index];
        let slot = self
            .lives
            .get_mut(id as usize)
            .ok_or(ProfileError::InvalidState)?;
        let fresh = slot.is_none_or(|life| {
            life.slab != slab || life.class != index || life.generation != generation
        });
        let reset_binding = delivery.is_some_and(|d| d.reset_binding);
        if fresh || reset_binding {
            self.heroes[id as usize] = self.weapons[index]
                .as_ref()
                .map(|w| Hero::new(index, &w.machine, self.tick));
        }
        if fresh {
            *slot = Some(Life {
                slab,
                class: index,
                generation,
                movement: State::new(&class.tuning),
                allow_fire: true,
                allow_reload: true,
                offhand_pressed: false,
                bot_secondary_held: None,
                guard: None,
                lock_pressed: false,
                power_input: Default::default(),
                energy_before: class.tuning.energy_capacity,
                power_thrust: 1.0,
                blast_pending: false,
                offhand_gun_salvo: false,
                offhand_gun_anim: None,
            });
            if let Some(acceleration) = class.acceleration {
                body.accel = acceleration;
                if body.mover >= 0. {
                    movement.mover(body.mover as u32).accel = acceleration;
                }
            }
            if class.tuning.jetpack.is_some() {
                body.fuel = 1.0;
                if body.mover >= 0. {
                    movement.mover(body.mover as u32).fuel = 1.0;
                }
            }
        }
        let life = slot.as_mut().unwrap();
        if reset_binding {
            life.guard = None;
            life.allow_fire = true;
            life.allow_reload = true;
            life.bot_secondary_held = None;
        }
        // The world can move a body between its steps (a throw, a blast, a seat);
        // the windows of the state it left do not carry over.
        life.movement.end_windows_the_body_left(super::soldier::body_facts(body));
        let tumbling = life.movement.tumble.is_some()
            || body.air == Some(crate::gen::Air::Thrown);
        let was_locked = body.lock > 0.0;
        let bot_frame = delivery.map(|d| d.frame);
        // A choked or tumbling soldier cannot act; choked turns toward the caster,
        // while a tumbling soldier keeps its aim heading without spinning.
        let controls = if tumbling {
            Controls::default()
        } else if held.is_some() {
            Controls { hold_to_block: controls.hold_to_block, ..Controls::default() }
        } else {
            controls
        };
        life.offhand_pressed = controls.offhand_pressed;
        life.lock_pressed |= controls.lock_pressed;
        life.energy_before = life.movement.energy;
        let thrust = std::mem::replace(&mut life.power_thrust, 1.0);
        let mut input = input.clone();
        if tumbling {
            input.forward = 0.;
            input.right = 0.;
            input.turn = 0.;
            input.pitch = 0.;
            input.jump = false;
            input.crouch = false;
            input.prone = false;
            if !alive {
                if let Some(t) = life.movement.tumble.as_mut() {
                    t.dead = true;
                }
                body.lock = 0.0;
            }
        } else if let Some(h) = held {
            input.forward = 0.;
            input.right = 0.;
            input.jump = false;
            input.crouch = false;
            body.aim_yaw = v8math::atan2(h.toward[0], h.toward[2]);
        } else if thrust != 1.0 {
            input.forward *= thrust;
            input.right *= thrust;
        }
        let input = &input;
        let state = &mut life.movement;
        let actions = Actions {
            sprint: controls.sprint,
            roll: controls.roll,
            ai: !human,
        };
        let step = Step {
            actions,
            tuning: &class.tuning,
            state,
            override_: None,
        };
        let mut combo_frame = None;
        let mut saber = None;
        let allowed = if tumbling {
            life.guard = None;
            life.blast_pending = false;
            if let (Some(hero), Some(weapon)) =
                (&mut self.heroes[id as usize], &self.weapons[index])
            {
                hero.state = ComboState::new(&weapon.machine);
                hero.swing_turn = 0.0;
                hero.guard = None;
                hero.frame = None;
                hero.reaction = None;
                if let Some(frame) = bot_frame {
                    self.bot_inputs.commit(frame.stamp, bot_input::Feedback {
                        guarding: false,
                        energy: step.state.energy,
                        exhausted: step.state.exhausted,
                    });
                }
            }
            let allowed = super::soldier::tick_soldier_in(
                movement,
                collision,
                body,
                input,
                dt,
                world,
                pass,
                Some(step),
            )?;
            // A soldier killed mid-tumble stays down on the ground rather than standing back up.
            if !alive {
                if let Some(t) = state.tumble.as_mut() {
                    t.dead = true;
                    if t.phase == TumblePhase::GetUp {
                        t.phase = TumblePhase::Landed;
                        body.posture = crate::gen::Posture::Crouch;
                        body.lock = 0.0;
                    }
                }
            }
            allowed
        } else if let (Some(hero), Some(weapon)) =
            (&mut self.heroes[id as usize], &self.weapons[index])
        {
            let (machine, combo_state) = (&weapon.machine, &mut hero.state);
            super::soldier::prepare_in(movement, body, world);
            let mover = movement.mover(body.mover as u32);
            super::soldier::load_body(mover, body, pass);
            let mut base = super::soldier::profile_input(input, true);
            if let Some(frame) = bot_frame.filter(|_| held.is_none()) {
                base.forward = frame.forward * thrust;
                base.right = frame.right * thrust;
            }
            let mut cmd = hero.buttons.input(controls, base.forward, base.right);
            if hero.flight.is_active() || hero.launch_due.is_some() {
                let fire = combo::ComboButton::Fire.bit() | combo::ComboButton::FireSecondary.bit();
                cmd.buttons_held &= !fire;
                cmd.buttons_pressed &= !fire;
            }
            let id_for_life = id;
            let id = body.mover as u32;
            let mut before = Root {
                position: body.position,
                yaw: body.aim_yaw,
            };
            let start = combo_state.eval_time;
            let (out, allowed) = movement.with_soldier_scene(collision, id, |mover, scene| {
                combo::step_soldier_combo_and_move(
                    machine,
                    combo_state,
                    &cmd,
                    mover,
                    step.state,
                    &class.tuning,
                    &base,
                    actions,
                    scene,
                    dt,
                )
            })?;
            super::soldier::store_body(body, movement.mover(id), dt);
            // The swing samples use this tick's turn on both ends; the aim may still move.
            before.yaw += out.swing_turn;
            hero.swing_turn = out.swing_turn;
            if let Some(frame) = bot_frame {
                self.bot_inputs.commit(frame.stamp, bot_input::Feedback {
                    guarding: combo_state.is_guarding(machine),
                    energy: step.state.energy,
                    exhausted: step.state.exhausted,
                });
            }
            hero.guard = out.deflect;
            hero.input_lock = out.input_lock;
            life.guard = out.deflect;
            life.blast_pending |= out.play_explosion;
            if hero.guard.is_none() {
                hero.reaction = None;
            }
            hero.frame = (!out.active_attacks.is_empty()).then_some(AttackFrame {
                life: LifeId {
                    slot: id_for_life,
                    slab,
                    generation: generation as u32,
                },
                before,
                after: Root {
                    position: body.position,
                    yaw: body.aim_yaw + out.swing_turn,
                },
                entered_tick: hero.entered_tick,
                outgoing: out.previous_state,
                entries: out.entries,
                start,
                dt,
                attacks: out.active_attacks,
            });
            if !out.entries.as_slice().is_empty() {
                hero.entered_tick = self.tick;
            }
            saber = Some(SaberSample {
                thrown: hero
                    .flight
                    .is_active()
                    .then_some((hero.flight.position, hero.flight.spin_angle)),
                weapon_hash: weapon.weapon,
                first_omitted_attack: out.first_omitted_attack,
                combo_hash: machine.name_hash,
                reaction: None,
                state_hash: weapon.state_hashes[usize::from(combo_state.state.0)],
            });
            combo_frame = Some((
                combo_state.state.0,
                combo_state.state_time,
                combo_state.anim_time,
                combo_state.blades_off_mask,
                machine.active_turn_factor(combo_state),
                out.swing_turn,
            ));
            allowed
        } else {
            super::soldier::tick_soldier_in(
                movement,
                collision,
                body,
                input,
                dt,
                world,
                pass,
                Some(step),
            )?
        };
        life.allow_fire = allowed;
        life.allow_reload = !was_locked && body.lock == 0.0;
        if !allowed {
            life.offhand_pressed = false;
        }
        // The wire capture reads the movement owner's copy of the life.
        movement.swbf2.store(id, generation, class_name, *state);
        movement.swbf2.set_fire(id, allowed);
        if human {
            let (combo, state_time, anim_time, mask, turn_factor, swing_turn) = combo_frame
                .map_or((None, 0., 0., 0, 1.0, 0.), |(id, s, a, m, tf, turn)| {
                    (Some(id), s, a, m, tf, turn)
                });
            let hero = self.heroes[id as usize].as_ref();
            self.sample = Some(Sample {
                id,
                generation,
                energy: state.energy,
                exhausted: state.exhausted,
                sprint: state.sprint_time.is_some(),
                sprint_time: state.sprint_time,
                roll: state.roll,
                roll_seconds: class.tuning.roll_seconds(),
                jump_count: state.jump_count,
                land_cooldown: state.land_cooldown,
                tumble: state.tumble.map(|t| t.phase),
                tumble_state: state.tumble,
                combo,
                state_time,
                anim_time,
                blades_off: mask,
                turn_factor,
                swing_turn,
                saber,
                combo_state: hero.map(|h| h.state),
                buttons_armed: hero.map_or(0, |h| h.buttons.armed),
                saber_locked: hero.is_some_and(|h| h.flight.is_active() || h.launch_due.is_some()),
            });
        }
        Ok(allowed)
    }
}

impl World {
    /// The yaw a living melee soldier's combo state turns its swing and drawn body by.
    pub fn melee_swing_turn(&self, life: LifeId) -> Option<f64> {
        let infantry = self.units.infantry.as_ref()?;
        let (r, u) = self.sim.store.unit.at(life.slot)?;
        (contact::identity(r, u) == life).then_some(())?;
        Some(infantry.heroes.get(life.slot as usize)?.as_ref()?.swing_turn)
    }

    /// Visit equipped melee soldiers in physical slot order after a complete step.
    /// All clocks, movement energy and the active state belong to this gameplay life.
    pub fn visit_melee(&self, mut visit: impl FnMut(LifeId, u32, &ComboState, f64, bool)) {
        let Some(infantry) = &self.units.infantry else {
            return;
        };
        for (i, hero) in infantry.heroes.iter().enumerate() {
            let Some(hero) = hero else { continue };
            let Some((r, u)) = self.sim.store.unit.at(i as u32) else {
                continue;
            };
            let Some(life) = infantry.lives[i]
                .as_ref()
                .filter(|l| l.slab == r.gen && l.generation == u.generation)
            else {
                continue;
            };
            if !u.alive || u.dead || u.removed {
                continue;
            }
            let Some(weapon) = &infantry.weapons[hero.class] else {
                continue;
            };
            visit(
                contact::identity(r, u),
                weapon.weapon,
                &hero.state,
                life.movement.energy,
                !weapon.machine.state(hero.state.state).attacks.is_empty(),
            );
        }
    }
}

#[cfg(test)]
mod leaving_ground;

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use crate::wire::melee::{Display, MeleePose, SCHEMA, ID_BASE};

    /// An invented saber wielder in slot 0: a two-state machine (the second a
    /// half-second looping clip), named hit and guard effects, and one living unit.
    fn hero_world() -> (crate::gen::Store, crate::store::Strings, Infantry, crate::gen::Store) {
        let machine = ComboMachine::from_json_value(
            &serde_json::json!({"nameHash":"13579bdf","states":[
                {"name":"IDLE","children":[{"key":"Duration","values":[0]}]},
                {"name":"ARC","children":[{"key":"Duration","values":[2]},
                    {"key":"Animation","values":["arc"],"children":[{"key":"Loop","values":[]}]}]}
            ]}),
            &WeaponMeleeSpec::default(),
            combo::COMBO_ANIM_FPS,
            |name| (name == "arc").then_some(0.5),
        )
        .unwrap();
        let mut strings = crate::store::Strings::new();
        let effects = contact::ContactEffects {
            hit: Some(strings.intern_str("spark_ring")),
            deflect: Some(strings.intern_str("guard_glint")),
            thrown_hit: Some(strings.intern_str("pebble_puff")),
        };
        let mut store = crate::gen::Store::default();
        let unit = store.unit.insert(crate::gen::Unit { generation: 4., alive: true, ..Default::default() });
        let mut hero = Hero::new(0, &machine, 0);
        hero.state.state = combo::StateId(1);
        hero.state.eval_time = 0.7;
        hero.state.blades_off_mask = 0b1;
        hero.reaction = Some((96, 3, 0.25));
        hero.swing_turn = 0.25;
        let second = Hero::new(0, &machine, 0);
        let infantry = Infantry {
            power_kits: Vec::new(),
            classes: Vec::new(),
            lives: vec![Some(Life {
                slab: unit.gen,
                class: 0,
                generation: 4.,
                movement: State {
                    energy: 100.0, exhausted: false, sprint_time: None, roll: None, land_cooldown: 0.0,
                    unsupported_time: None, jump_count: 0, tumble: None,
                },
                allow_fire: true,
                allow_reload: true,
                offhand_pressed: false,
                bot_secondary_held: None,
                guard: None,
                lock_pressed: false,
                power_input: Default::default(),
                energy_before: 0.0,
                power_thrust: 1.0,
                blast_pending: false,
                offhand_gun_salvo: false,
                offhand_gun_anim: None,
            })],
            bot_inputs: bot_input::BotInputs::new(1),
            mailbox: 0,
            player: 0,
            weapons: vec![Some(MeleeWeapon {
                weapon: 0x2468,
                effects,
                state_hashes: Vec::new(),
                machine,
                spec: WeaponMeleeSpec::default(),
                pose: blade::BladePose::empty(),
                offhand: None,
            })],
            heroes: vec![Some(hero), Some(second)],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: Vec::new(),
            life_serial: 0,
            tick: 100,
        };
        let seated = {
            let mut other = crate::gen::Store::default();
            let r = other.unit.insert(crate::gen::Unit { generation: 4., alive: true, ..Default::default() });
            other.unit.get_mut(r).unwrap().seat = Some(Default::default());
            other
        };
        (store, strings, infantry, seated)
    }

    #[test]
    fn a_melee_soldier_publishes_its_pose_as_a_world_field() {
        let (store, _, infantry, seated) = hero_world();
        let mut seen = Vec::new();
        infantry.melee_poses(&store, 100., |slot, generation, pose| seen.push((slot, generation, pose)));
        let pose = MeleePose {
            state: 1,
            origin: 58.,
            loop_mode: combo::LoopMode::Wrap,
            clip_len: 0.5,
            blades_off: 0b1,
            reaction: Some((3, 96., 0.25)),
            swing_turn: 0.25,
        };
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].0, seen[0].1), (0, 4.));
        let got = seen[0].2;
        assert!((got.origin - pose.origin).abs() < 1e-9);
        assert_eq!(MeleePose { origin: pose.origin, ..got }, pose);
        // The battle's own clip time comes back from the field: 0.7 s wraps to 0.2 s.
        assert!((got.anim_time(100.) - 0.2).abs() < 1e-9);
        assert_eq!(got.reaction_at(100.).map(|r| r.0), Some(3));
        // A seated soldier shows its seat, not a combo pose.
        let mut none = 0;
        infantry.melee_poses(&seated, 100., |_, _, _| none += 1);
        assert_eq!(none, 0);
        // The capture offers it under its own schema and identity, encoded.
        let mut units = crate::units::owner::Units::new();
        units.infantry = Some(Box::new(infantry));
        let mut stagger = crate::wire::stagger::Stagger::new();
        stagger.begin();
        let effects = crate::combat::powers::EffectTable::with_slots(4);
        crate::wire::melee::Fields::default().take(100., &units, &store, None, &effects, &mut stagger).unwrap();
        let entry = stagger.entries.iter().find(|e| e.schema == SCHEMA).expect("display field");
        let field = entry.field.as_ref().expect("due at once");
        assert_eq!((field.id, field.generation), (ID_BASE, 4.));
        assert_eq!(Display::decode(&field.payload), Some(Display { melee: Some(got), ..Display::default() }));
    }

    #[test]
    fn a_choke_shows_in_the_held_soldiers_display_and_its_end_too() {
        let (store, _, infantry, _) = hero_world();
        let (r, u) = store.unit.at(0).unwrap();
        let held = crate::combat::powers::LifeKey::of(r, u);
        let by = crate::combat::powers::LifeKey { slot: 3, generation: 1, life: 2. };
        let mut effects = crate::combat::powers::EffectTable::with_slots(4);
        effects.hold(held, by, [0., 0., 1.], 0x0bad_f00d);
        let mut seen = Vec::new();
        infantry.hero_displays(&store, 100., None, &effects, |slot, generation, display| seen.push((slot, generation, display)));
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].0, seen[0].1, seen[0].2.choked, seen[0].2.cast), (0, 4., Some(0x0bad_f00d), None));
        assert!(seen[0].2.melee.is_some());
        // Released: the same soldier's display goes on without the choke.
        let free = crate::combat::powers::EffectTable::with_slots(4);
        let mut after = Vec::new();
        infantry.hero_displays(&store, 101., None, &free, |_, _, display| after.push(display));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].choked, None);
    }

    #[test]
    fn a_soldier_without_a_blade_shows_its_display_while_a_part_lasts_and_briefly_after() {
        use crate::wire::stagger::Stagger;
        let (store, _, mut infantry, _) = hero_world();
        infantry.heroes[0] = None;
        let (r, u) = store.unit.at(0).unwrap();
        let held = crate::combat::powers::LifeKey::of(r, u);
        let by = crate::combat::powers::LifeKey { slot: 3, generation: 1, life: 2. };
        let free = crate::combat::powers::EffectTable::with_slots(4);
        let mut choked = crate::combat::powers::EffectTable::with_slots(4);
        choked.hold(held, by, [0., 0., 1.], 0);
        let mut units = crate::units::owner::Units::new();
        units.infantry = Some(Box::new(infantry));
        let mut fields = crate::wire::melee::Fields::default();
        let mut stagger = Stagger::new();
        let mut take = |tick: f64, effects: &crate::combat::powers::EffectTable, stagger: &mut Stagger| {
            stagger.begin();
            fields.take(tick, &units, &store, None, effects, stagger).unwrap();
            stagger.finish(&[SCHEMA]);
            stagger.entries.iter().find(|e| e.schema == SCHEMA).and_then(|e| e.field.as_ref()).map(|f| Display::decode(&f.payload).unwrap())
        };
        let keep = crate::wire::melee::KEEP_TICKS;
        assert_eq!(take(100., &free, &mut stagger), None, "no blade and nothing to show: no field");
        assert_eq!(take(101., &choked, &mut stagger), Some(Display { choked: Some(0), ..Display::default() }));
        assert_eq!(take(102., &free, &mut stagger), Some(Display::default()), "the end of the choke travels");
        assert_eq!(take(101. + keep, &free, &mut stagger), Some(Display::default()), "and stays a while");
        assert_eq!(take(102. + keep, &free, &mut stagger), None, "then the field leaves");
        assert_eq!(take(103. + keep, &choked, &mut stagger).map(|d| d.choked), Some(Some(0)), "and comes back with a part");
    }

    #[test]
    fn a_kept_contact_sends_the_effect_its_data_names() {
        use contact::{Cause, Contact, LifeId, Outcome};
        let (store, strings, mut infantry, _) = hero_world();
        let hero = LifeId { slot: 0, slab: store.unit.at(0).unwrap().0.gen, generation: 4 };
        let other = LifeId { slot: 7, slab: 1, generation: 1 };
        let contact = |source, target, cause, outcome| Contact {
            sequence: 0, tick: 100, source: Some(source), target, cause, outcome,
            point: [1., 2., 3.], direction: combo::DeflectDirection::Forward, reaction: None,
            before: 5., after: 5., energy_spent: 0., critical: false,
        };
        let blade = Cause::Saber { weapon: 0x2468, entered_tick: 90, state: 1, attack: 0, edge: 0 };
        let bolt = Cause::Bolt { slot: 2, generation: 1, definition: 5 };
        let mut events = crate::store::calls::Events::default();
        for (c, name) in [
            (contact(hero, other, blade, Outcome::Hit), Some("spark_ring")),
            (contact(other, hero, blade, Outcome::Blocked), Some("guard_glint")),
            (contact(other, hero, bolt, Outcome::Deflected), Some("guard_glint")),
            (contact(other, hero, Cause::Shot { definition: 1 }, Outcome::Blocked), Some("guard_glint")),
            (contact(hero, other, Cause::Throw { weapon: 1, launched: 80, returning: false }, Outcome::Hit), Some("pebble_puff")),
            (contact(other, hero, Cause::Throw { weapon: 1, launched: 80, returning: true }, Outcome::Deflected), Some("guard_glint")),
            // No guard data for a soldier without a melee weapon, and powers show their own.
            (contact(hero, other, blade, Outcome::Blocked), None),
            (contact(other, hero, Cause::Power { definition: 3 }, Outcome::Blocked), None),
            (contact(other, hero, bolt, Outcome::Hit), None),
        ] {
            let before = events.count();
            let kept = infantry.contacts.latest;
            contact::keep_contact(&mut infantry, &mut events, c, None);
            assert_eq!(infantry.contacts.latest, kept + 1);
            let record = &events.words()[events.words().len() - (2 + crate::gen::F3PowerupEffect::WORDS)..];
            match name {
                Some(name) => {
                    assert_eq!(events.count(), before + 1);
                    assert_eq!(record[0] as u32, crate::gen::EVENT_F3_POWERUP_EFFECT);
                    let e = crate::gen::F3PowerupEffect::take(&record[2..], &[]);
                    assert_eq!((String::from_utf16_lossy(strings.units(e.name)), e.position), (name.to_string(), [1., 2., 3.]));
                }
                None => assert_eq!(events.count(), before),
            }
        }
        // While slot 0 sweeps, its hero and its class's weapon are out of the tables: the
        // weapon it holds names the hit, and a guard of the same class (slot 1) its glint.
        let effects = infantry.weapons[0].as_ref().unwrap().effects;
        let held = (infantry.heroes[0].take(), infantry.weapons[0].take());
        let partner = LifeId { slot: 1, slab: 1, generation: 1 };
        for (c, name) in [
            (contact(hero, other, blade, Outcome::Hit), "spark_ring"),
            (contact(hero, partner, blade, Outcome::Blocked), "guard_glint"),
        ] {
            let before = events.count();
            contact::keep_contact(&mut infantry, &mut events, c, Some((0, effects)));
            assert_eq!(events.count(), before + 1);
            let record = &events.words()[events.words().len() - (2 + crate::gen::F3PowerupEffect::WORDS)..];
            let e = crate::gen::F3PowerupEffect::take(&record[2..], &[]);
            assert_eq!(String::from_utf16_lossy(strings.units(e.name)), name);
        }
        (infantry.heroes[0], infantry.weapons[0]) = held;
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use combo::ComboButton;

    #[test]
    fn class_properties_select_the_source_and_motion_of_an_airborne_jump() {
        let properties = |s| bfe_content::json::parse(s).unwrap();
        assert!(classes::jetpack(&properties(r#"{"properties":[]}"#)).unwrap().is_none());
        let fuel = classes::jetpack(&properties(r#"{"properties":[["JetPush","2"],["JetType","hover"],["JetFuelCost",".36"]]}"#))
            .unwrap().unwrap();
        assert_eq!(fuel.energy_cost, 0.);
        assert!(!fuel.force_jump);
        assert!(fuel.physics.hover);
        assert_eq!((fuel.physics.push, fuel.physics.cost), (2., 0.36));
        let force_jumper = classes::jetpack(&properties(
            r#"{"properties":[["JetJump","2"],["jetjump","9"],["JetEnergyDrain","13"]],"weapons":[{"comboBindings":[{"mode":"melee"}]}]}"#,
        ))
        .unwrap().unwrap();
        assert_eq!(force_jumper.energy_cost, 13.);
        assert_eq!(force_jumper.physics.jump, 9.);
        assert!(!force_jumper.physics.hover);
        assert!(force_jumper.force_jump);
        let hover_melee = classes::jetpack(&properties(
            r#"{"properties":[["JetJump","5"],["JetType","hover"],["JetEnergyDrain","18"],["JetFuelCost",".25"]],"weapons":[{"comboBindings":[{"mode":"melee"}]}]}"#,
        ))
        .unwrap().unwrap();
        assert_eq!(hover_melee.energy_cost, 18.);
        assert!(hover_melee.physics.hover);
        assert!(!hover_melee.force_jump);
        let non_hover_ranged = classes::jetpack(&properties(
            r#"{"properties":[["JetJump","8"],["JetPush","6"],["JetFuelCost",".3"]]}"#,
        ))
        .unwrap().unwrap();
        assert!(!non_hover_ranged.physics.hover);
        assert!(!non_hover_ranged.force_jump);
        assert_eq!((non_hover_ranged.physics.jump, non_hover_ranged.physics.push), (8., 6.));
        assert!(classes::jetpack(&properties(r#"{"properties":[["JetJump","NaN"]]}"#)).is_err());
    }

    #[test]
    fn fresh_swbf2_jetpack_life_starts_with_full_fuel_tank() {
        use bfe_content::{
            bvh::{build, Positions},
            collision::{triangle_bounds, MapCollision, TriangleGrid},
        };
        let floor = vec![-20., 0., -20., 20., 0., -20., 20., 0., 20., -20., 0., 20.];
        let positions = vec![-6., -100., -6., 6., -100., -6., 6., -100., 6., -6., -100., 6.];
        let indices = vec![0, 2, 1, 0, 3, 2];
        let buildings = build(Positions::F32(&positions), &indices);
        let terrain = build(Positions::F32(&floor), &indices);
        let map = MapCollision {
            tri_bounds: triangle_bounds(&positions, &indices),
            grid: Some(TriangleGrid::new(&floor, &indices)),
            positions: positions.into(),
            indices: indices.clone().into(),
            polygons: vec![0, 1].into(),
            masks: Vec::new().into(),
            has_terrain: true,
            terrain_positions: floor.into(),
            terrain_indices: indices.into(),
            kill_y: -200.,
        };
        let mut owner = crate::collision::CollisionOwner::new();
        let world = owner.native_install(&map, &buildings, Some(&terrain)).unwrap().world;
        let mut collision = owner.state;
        let mut movement = MovementOwner::new();
        movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        let row = ControlSpeed { forward: 1., strafe: 1., turn: 1. };
        let tuning = Tuning {
            stand: row,
            crouch: row,
            sprint: row,
            jump: row,
            jet: row,
            jetpack: Some(Jetpack {
                physics: Jet {
                    jump: 7.,
                    push: 8.,
                    recharge: 0.12,
                    cost: 0.15,
                    initial: 0.25,
                    min_border: 0.24,
                    hover: true,
                },
                acceleration: 10.,
                energy_cost: 0.,
                force_jump: false,
            }),
            roll: row,
            tumble: row,
            sprint_accelerate_time: 0.5,
            energy_capacity: 100.,
            energy_floor: 0.,
            energy_restore: 15.,
            energy_restore_idle: 25.,
            energy_drain_sprint: 10.,
            energy_min_sprint: 20.,
            energy_cost_jump: 0.,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 25.,
            jump_height: 2.5,
            jump_forward_speed_factor: 1.3,
            jump_strafe_speed_factor: 1.,
            roll_speed_factor: 1.5,
            roll_frames: 0,
            recover_from_tumble: false,
            getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
        };
        let class_name = StrId(1);
        let mut infantry = Infantry {
            power_kits: vec![None],
            classes: vec![Class { name: class_name, registry_id: None, tuning, acceleration: None, offhand_guns: Vec::new() }],
            lives: vec![None; 2],
            bot_inputs: bot_input::BotInputs::new(2),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes: vec![None, None],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; 2],
            life_serial: 0,
            tick: 0,
        };
        let mut body = Soldier {
            mover: -1.,
            max_speed: 8.,
            max_strafe: 6.,
            max_jump: 8.,
            max_turn: 2.,
            max_pitch: 0.5,
            accel: 70.,
            pitch_limits: [-0.8, 1.1],
            max_fly_height: 200.,
            grounded: true,
            normal: [0., 1., 0.],
            fuel: 0.5,
            ..Soldier::default()
        };
        let ok = infantry
            .advance(
                &mut movement,
                &mut collision,
                0,
                1,
                class_name,
                1.,
                true,
                true,
                Controls::default(),
                None,
                None,
                &mut body,
                &MoveInput::default(),
                1. / 60.,
                world as usize,
                -1,
            )
            .unwrap();
        assert!(ok);
        assert_eq!(body.fuel, 1.0);
        assert_eq!(movement.mover(body.mover as u32).fuel, 1.0);
    }

    #[test]
    fn outgoing_and_new_attacks_keep_their_respective_entry_ticks() {
        let (old, new) = (combo::StateId(3), combo::StateId(5));
        let entries = [combo::StateEntry {
            state: new,
            eval_time: 0.,
        }];
        assert_eq!(attack_origin(old, &entries, old, 12, 19), 12);
        assert_eq!(attack_origin(old, &entries, new, 12, 19), 19);
    }

    #[test]
    fn a_new_life_requires_its_own_press_or_release() {
        let fire = ComboButton::Fire.bit();
        let mut buttons = LifeButtons::default();
        let mut c = Controls {
            melee_held: fire,
            block: true,
            ..Default::default()
        };
        for _ in 0..3 {
            let cmd = buttons.input(c, 0., 0.);
            assert_eq!((cmd.buttons_pressed, cmd.buttons_held), (0, 0));
        }
        c.block_pressed = true;
        let block = ComboButton::FireSecondary.bit();
        assert_eq!(buttons.input(c, 0., 0.).buttons_held, block);
        buttons.input(Controls::default(), 0., 0.);
        c.block_pressed = false;
        assert_eq!(buttons.input(c, 0., 0.).buttons_held, fire | block);
    }

    #[test]
    fn a_dedicated_guard_tap_and_the_hold_preference_are_independent_of_offhand() {
        let bit = ComboButton::FireSecondary.bit();
        let mut life = LifeButtons::default();
        let offhand = Controls {
            melee_pressed: bit,
            melee_held: bit,
            ..Default::default()
        };
        let stock = life.input(offhand, 0., 0.);
        assert_eq!((stock.buttons_pressed, stock.buttons_held), (0, 0));
        for hold in [false, true] {
            let tap = life.input(
                Controls {
                    block_pressed: true,
                    hold_to_block: hold,
                    ..Default::default()
                },
                0.,
                0.,
            );
            assert_eq!(
                (tap.buttons_pressed, tap.buttons_held, tap.hold_to_block),
                (bit, 0, hold)
            );
            let up = life.input(
                Controls {
                    hold_to_block: hold,
                    ..Default::default()
                },
                0.,
                0.,
            );
            assert_eq!(
                (up.buttons_pressed, up.buttons_held, up.hold_to_block),
                (0, 0, hold)
            );
        }
    }

    #[test]
    fn melee_throw_reloading_matches_post_return_reload_only() {
        let machine = ComboMachine::from_json_value(
            &serde_json::json!({"nameHash":"11111111","states":[{"name":"IDLE","children":[{"key":"Duration","values":[0]}]}]}),
            &WeaponMeleeSpec::default(),
            combo::COMBO_ANIM_FPS,
            |_| None,
        )
        .unwrap();
        let mut hero = Hero::new(0, &machine, 0);
        let mut inf = Infantry {
            power_kits: Vec::new(),
            classes: Vec::new(),
            lives: vec![Some(Life {
                slab: 1,
                class: 0,
                generation: 1.0,
                movement: State { energy: 100.0, exhausted: false, sprint_time: None, roll: None, jump_count: 0, land_cooldown: 0.0, tumble: None, unsupported_time: None },
                allow_fire: true,
                allow_reload: true,
                offhand_pressed: false,
                bot_secondary_held: None,
                guard: None,
                lock_pressed: false,
                power_input: Default::default(),
                energy_before: 0.0,
                power_thrust: 1.0,
                blast_pending: false,
                offhand_gun_salvo: false,
                offhand_gun_anim: None,
            })],
            bot_inputs: bot_input::BotInputs::new(1),
            mailbox: 0,
            player: 0,
            weapons: Vec::new(),
            heroes: vec![None],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: Vec::new(),
            life_serial: 0,
            tick: 10,
        };
        // Windup is firing, not reloading.
        hero.launch_due = Some(25);
        hero.ready_at = 50;
        inf.heroes[0] = Some(hero);
        assert!(!inf.melee_throw_reloading(0, 1.0));
        // In flight is firing, not reloading.
        let h = inf.heroes[0].as_mut().unwrap();
        h.launch_due = None;
        h.flight.phase = combo::SaberThrowPhase::Outbound;
        assert!(!inf.melee_throw_reloading(0, 1.0));
        // Returned and waiting for ready_at is reloading.
        let h = inf.heroes[0].as_mut().unwrap();
        h.flight = Default::default();
        assert!(inf.melee_throw_reloading(0, 1.0));
        // Past ready_at is idle again.
        inf.tick = 50;
        assert!(!inf.melee_throw_reloading(0, 1.0));
    }

    #[test]
    fn bot_offhand_pressed_tracks_rising_edges_per_life_while_humans_use_press_latch() {
        let mut inf = Infantry {
            power_kits: Vec::new(),
            classes: Vec::new(),
            lives: vec![Some(Life {
                slab: 1,
                class: 0,
                generation: 1.0,
                movement: State { energy: 100.0, exhausted: false, sprint_time: None, roll: None, jump_count: 0, land_cooldown: 0.0, tumble: None, unsupported_time: None },
                allow_fire: true,
                allow_reload: true,
                offhand_pressed: false,
                bot_secondary_held: None,
                guard: None,
                lock_pressed: false,
                power_input: Default::default(),
                energy_before: 0.0,
                power_thrust: 1.0,
                blast_pending: false,
                offhand_gun_salvo: false,
                offhand_gun_anim: None,
            })],
            bot_inputs: bot_input::BotInputs::new(1),
            mailbox: 0,
            player: 0,
            weapons: Vec::new(),
            heroes: vec![None],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: Vec::new(),
            life_serial: 0,
            tick: 0,
        };
        // Bot rising edge from initial None -> true fires once, sustained hold does not re-edge,
        // releasing and re-holding produces a fresh edge.
        assert!(inf.offhand_edge(0, 1.0, false, true));
        assert!(!inf.offhand_edge(0, 1.0, false, true));
        assert!(!inf.offhand_edge(0, 1.0, false, false));
        assert!(inf.offhand_edge(0, 1.0, false, true));
        // Human path reads and clears offhand_pressed independently of held level.
        assert!(!inf.offhand_edge(0, 1.0, true, true));
        inf.lives[0].as_mut().unwrap().offhand_pressed = true;
        assert!(inf.offhand_edge(0, 1.0, true, false));
        assert!(!inf.offhand_edge(0, 1.0, true, true));
    }

    #[test]
    fn swbf2_mouse_look_uses_class_control_speed_turn_factors() {
        use crate::gen::{Air, Posture, Unit, UnitBody};
        use crate::movement::soldier::swbf2::RollDirection;
        use crate::players::{
            commands::{apply, Command},
            owner::{Player, Players},
        };

        let mut world = World::new();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let class_name = world.sim.strings.intern_str("test_blade");
        let max_turn = 6.0;
        let mut unit_row = Unit {
            class_name,
            generation: 1.0,
            alive: true,
            human: true,
            ..Unit::default()
        };
        unit_row.soldier = UnitBody::Soldier(Soldier {
            max_turn,
            pitch_limits: [-0.8, 1.1],
            grounded: true,
            ..Soldier::default()
        });
        let unit = crate::units::f3::with_owner_rows(&mut world, |store, owner| {
            owner.build(store, unit_row)
        })
        .reference();
        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 1);
        let key = players.admit(Player::new(0, 0., 4)).unwrap();

        let cs = |turn| ControlSpeed { forward: 1.0, strafe: 1.0, turn };
        let tuning = Tuning {
            stand: cs(1.0),
            crouch: cs(0.85),
            sprint: cs(0.5),
            jump: cs(0.6),
            jet: cs(1.25),
            jetpack: None,
            roll: cs(0.35),
            tumble: cs(0.1),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 20.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            recover_from_tumble: false,
            getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
        };
        let slot = unit.slot as usize;
        let mut lives = vec![None; slot + 1];
        lives[slot] = Some(Life {
            slab: unit.gen,
            class: 0,
            generation: 1.0,
            movement: State::new(&tuning),
            allow_fire: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: 100.0,
            power_thrust: 1.0,
            blast_pending: false,
            allow_reload: true,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        world.units.infantry = Some(Box::new(Infantry {
            power_kits: vec![None],
            classes: vec![Class {
                name: class_name,
                registry_id: Some(1),
                tuning,
                acceleration: None,
                offhand_guns: Vec::new(),
            }],
            lives,
            bot_inputs: bot_input::BotInputs::new(slot + 1),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes: (0..=slot).map(|_| None).collect(),
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; slot + 1],
            life_serial: 0,
            tick: 1,
        }));

        let rate = max_turn / 2048.0;
        let cases: [(Posture, Option<Air>, bool, bool, f64); 8] = [
            (Posture::Stand, None, false, false, 1.0),
            (Posture::Crouch, None, false, false, 0.85),
            (Posture::Stand, None, true, false, 0.5),
            (Posture::Stand, Some(Air::Jump), false, false, 0.6),
            (Posture::Stand, Some(Air::Fall), false, false, 0.6),
            (Posture::Stand, Some(Air::Jet), false, false, 1.25),
            (Posture::Crouch, None, false, true, 0.35),
            (Posture::Stand, Some(Air::Thrown), false, false, 0.1),
        ];
        for (posture, air, sprint, roll, expected) in cases {
            {
                let u = world.sim.store.unit.get_mut(unit).unwrap();
                let UnitBody::Soldier(s) = &mut u.soldier else { unreachable!() };
                s.aim_yaw = 0.0;
                s.posture = posture;
                s.air = air;
                let life = world.units.infantry.as_mut().unwrap().lives[slot].as_mut().unwrap();
                life.movement.sprint_time = sprint.then_some(0.1);
                life.movement.roll = roll.then_some(Roll { direction: RollDirection::Forward, elapsed: 0.1 });
            }
            apply(&mut world, &mut players, key, unit, &Command::default(), &[[16.0, 0.0]], 1., 1., 0.);
            let UnitBody::Soldier(s) = &world.sim.store.unit.get(unit).unwrap().soldier else { unreachable!() };
            let got = -s.aim_yaw / (16.0 * rate);
            assert!((got - expected).abs() < 1e-12, "state ({posture:?},{air:?},{sprint},{roll}): got {got} != {expected}");
        }

        // 2004 battles (no `units.infantry`) still use Soldier::mode_turn() (0.5 in jump, 1.0 in jet).
        world.units.infantry = None;
        for (air, expected) in [(Some(Air::Jump), 0.5), (Some(Air::Jet), 1.0)] {
            {
                let u = world.sim.store.unit.get_mut(unit).unwrap();
                let UnitBody::Soldier(s) = &mut u.soldier else { unreachable!() };
                s.aim_yaw = 0.0;
                s.posture = Posture::Stand;
                s.air = air;
            }
            apply(&mut world, &mut players, key, unit, &Command::default(), &[[16.0, 0.0]], 1., 1., 0.);
            let UnitBody::Soldier(s) = &world.sim.store.unit.get(unit).unwrap().soldier else { unreachable!() };
            let got = -s.aim_yaw / (16.0 * rate);
            assert!((got - expected).abs() < 1e-12, "2004 air {air:?}: got {got} != {expected}");
        }
    }

    #[test]
    fn swbf2_mouse_look_scales_by_active_combo_turn_factor_through_player_commands() {
        use crate::gen::{Air, Posture, Unit, UnitBody};
        use crate::players::{
            commands::{apply, Command},
            owner::{Player, Players},
        };
        use serde_json::json;

        let machine = ComboMachine::from_json_value(
            &json!({
                "nameHash": "0000cafe",
                "states": [
                    { "name": "IDLE", "children": [{ "key": "Duration", "values": [0] }] },
                    { "name": "SLASH", "children": [
                        { "key": "Duration", "values": [0.40] },
                        { "key": "TurnFactor", "values": [0.5] }
                    ]},
                    { "name": "DASH", "children": [
                        { "key": "Duration", "values": [0.40] },
                        { "key": "TurnFactor", "values": [0.3] }
                    ]},
                    { "name": "RECOVER", "children": [
                        { "key": "Duration", "values": [0.30] },
                        { "key": "TurnFactor", "values": [0.1] }
                    ]},
                    { "name": "LOCK_TURN", "children": [
                        { "key": "Duration", "values": [0.30] },
                        { "key": "InputLock", "values": ["Turn"] }
                    ]}
                ]
            }),
            &WeaponMeleeSpec::default(),
            combo::COMBO_ANIM_FPS,
            |_| None,
        )
        .unwrap();

        let mut world = World::new();
        crate::units::f3::f3_init_in(&mut world, 0, 0., 0);
        let class_name = world.sim.strings.intern_str("test_duelist");
        let max_turn = 5.0;
        let mut unit_row = Unit {
            class_name,
            generation: 1.0,
            alive: true,
            human: true,
            ..Unit::default()
        };
        unit_row.soldier = UnitBody::Soldier(Soldier {
            max_turn,
            pitch_limits: [-0.8, 1.1],
            grounded: true,
            ..Soldier::default()
        });
        let unit = crate::units::f3::with_owner_rows(&mut world, |store, owner| {
            owner.build(store, unit_row)
        })
        .reference();
        let mut players = Players::new();
        players.activate(&mut world.sim.strings, 1);
        let key = players.admit(Player::new(0, 0., 4)).unwrap();

        let cs = |turn| ControlSpeed { forward: 1.0, strafe: 1.0, turn };
        let tuning = Tuning {
            stand: cs(1.0),
            crouch: cs(1.0),
            sprint: cs(0.5),
            jump: cs(0.6),
            jet: cs(1.25),
            jetpack: None,
            roll: cs(0.35),
            tumble: cs(0.1),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 20.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            recover_from_tumble: false,
            getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
        };
        let slot = unit.slot as usize;
        let mut lives = vec![None; slot + 1];
        lives[slot] = Some(Life {
            slab: unit.gen,
            class: 0,
            generation: 1.0,
            movement: State::new(&tuning),
            allow_fire: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: 100.0,
            power_thrust: 1.0,
            blast_pending: false,
            allow_reload: true,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        let mut heroes: Vec<Option<Hero>> = (0..=slot).map(|_| None).collect();
        heroes[slot] = Some(Hero::new(0, &machine, 1));
        let state_hashes = machine.state_names.iter().map(|s| combo::zero_fnv1a_lower(s)).collect();
        world.units.infantry = Some(Box::new(Infantry {
            power_kits: vec![None],
            classes: vec![Class {
                name: class_name,
                registry_id: Some(1),
                tuning,
                acceleration: None,
                offhand_guns: Vec::new(),
            }],
            lives,
            bot_inputs: bot_input::BotInputs::new(slot + 1),
            mailbox: 0,
            player: 0,
            weapons: vec![Some(MeleeWeapon {
                weapon: 1,
                state_hashes,
                machine,
                spec: WeaponMeleeSpec::default(),
                pose: BladePose::empty(),
                offhand: None,
                effects: Default::default(),
            })],
            heroes,
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; slot + 1],
            life_serial: 0,
            tick: 1,
        }));

        let rate = max_turn / 2048.0;
        for (state_idx, air, expected) in [
            (0u16, None, 1.0),
            (1, None, 0.5),
            (2, None, 0.3),
            (3, None, 0.1),
            (4, None, 0.0),
            (1, Some(Air::Jump), 0.6 * 0.5),
        ] {
            {
                let u = world.sim.store.unit.get_mut(unit).unwrap();
                let UnitBody::Soldier(s) = &mut u.soldier else { unreachable!() };
                s.aim_yaw = 0.0;
                s.posture = Posture::Stand;
                s.air = air;
                let hero = world.units.infantry.as_mut().unwrap().heroes[slot].as_mut().unwrap();
                hero.state.state = combo::StateId(state_idx);
                hero.state.state_time = 0.05;
            }
            apply(&mut world, &mut players, key, unit, &Command::default(), &[[20.0, 0.0]], 1., 1., 0.);
            let UnitBody::Soldier(s) = &world.sim.store.unit.get(unit).unwrap().soldier else { unreachable!() };
            let got = -s.aim_yaw / (20.0 * rate);
            assert!(
                (got - expected).abs() < 1e-12,
                "combo state {state_idx} air {air:?}: got {got} != {expected}"
            );
        }
    }

    #[test]
    fn swbf2_turn_predictor_matches_simulation_across_postures_and_combos() {
        use crate::collision::CollisionOwner;
        use crate::gen::{Air, MoveInput, Posture};
        use bfe_content::{
            bvh::{build, Positions},
            collision::{triangle_bounds, MapCollision, TriangleGrid},
        };
        use bfe_family::GameFamily;
        use serde_json::json;

        const DT: f64 = 1.0 / 60.0;

        // Upper deck y=0 (z in [-40, 22]) with a drop to lower deck y=-18 (z in [22, 60])
        // so the scripted soldier exercises sprint ramp, roll, jump, ledge fall, and landing.
        let floor = vec![
            -40.0_f32, 0.0, -40.0, 40.0, 0.0, -40.0, 40.0, 0.0, 22.0, -40.0, 0.0, 22.0,
            -40.0, -18.0, 22.0, 40.0, -18.0, 22.0, 40.0, -18.0, 60.0, -40.0, -18.0, 60.0,
        ];
        let positions = vec![
            -6.0_f32, -100.0, -6.0, 6.0, -100.0, -6.0, 6.0, -100.0, 6.0, -6.0, -100.0, 6.0,
        ];
        let b_indices = vec![0u32, 2, 1, 0, 3, 2];
        let t_indices = vec![0u32, 2, 1, 0, 3, 2, 4, 6, 5, 4, 7, 6];
        let buildings = build(Positions::F32(&positions), &b_indices);
        let terrain = build(Positions::F32(&floor), &t_indices);
        let map = MapCollision {
            tri_bounds: triangle_bounds(&positions, &b_indices),
            grid: Some(TriangleGrid::new(&floor, &t_indices)),
            positions: positions.into(),
            indices: b_indices.into(),
            polygons: vec![0, 1].into(),
            masks: Vec::new().into(),
            has_terrain: true,
            terrain_positions: floor.into(),
            terrain_indices: t_indices.into(),
            kill_y: -200.0,
        };
        let mut collision_owner = CollisionOwner::new();
        let world_id = collision_owner
            .native_install(&map, &buildings, Some(&terrain))
            .unwrap()
            .world as usize;
        let mut collision = collision_owner.state;

        let mut movement = MovementOwner::new();
        movement.select_family(GameFamily::Swbf2).unwrap();

        let machine = ComboMachine::from_json_value(
            &json!({
                "nameHash": "0000d00d",
                "states": [
                    { "name": "IDLE", "children": [
                        { "key": "Duration", "values": [0] },
                        { "key": "Transition", "values": ["SLASH"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["Fire", "Press"] }] }
                        ]},
                        { "key": "Transition", "values": ["LOCK_TURN"], "children": [
                            { "key": "If", "children": [{ "key": "Button", "values": ["FireSecondary", "Press"] }] }
                        ]}
                    ]},
                    { "name": "SLASH", "children": [
                        { "key": "Duration", "values": [0.20] },
                        { "key": "TurnFactor", "values": [0.5] },
                        { "key": "Transition", "values": ["RECOVER"] }
                    ]},
                    { "name": "RECOVER", "children": [
                        { "key": "Duration", "values": [0.20] },
                        { "key": "TurnFactor", "values": [0.1] },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]},
                    { "name": "LOCK_TURN", "children": [
                        { "key": "Duration", "values": [0.30] },
                        { "key": "InputLock", "values": [0.15, "Turn"] },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]}
                ]
            }),
            &WeaponMeleeSpec::default(),
            combo::COMBO_ANIM_FPS,
            |_| None,
        )
        .unwrap();

        let speed = |forward, strafe, turn| ControlSpeed { forward, strafe, turn };
        let tuning = Tuning {
            stand: speed(1.0, 1.0, 1.0),
            crouch: speed(0.5, 0.5, 0.85),
            sprint: speed(1.75, 0.4, 0.5),
            jump: speed(0.04, 0.04, 0.6),
            jet: speed(0.8, 0.65, 1.25),
            jetpack: None,
            roll: speed(0.0, 0.0, 0.35),
            tumble: speed(0.0, 0.0, 0.1),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 12.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.25,
            jump_strafe_speed_factor: 0.75,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
            recover_from_tumble: true,
        };

        let mut world = World::new();
        let class_name = world.sim.strings.intern_str("sim_predictor_hero");
        let state_hashes = machine
            .state_names
            .iter()
            .map(|s| combo::zero_fnv1a_lower(s))
            .collect();
        let mut infantry = Infantry {
            power_kits: vec![None],
            classes: vec![Class {
                name: class_name,
                registry_id: Some(1),
                tuning,
                acceleration: Some(40.0),
                offhand_guns: vec![],
            }],
            lives: vec![None; 2],
            bot_inputs: bot_input::BotInputs::new(2),
            mailbox: 0,
            player: 0,
            weapons: vec![Some(MeleeWeapon {
                weapon: 1,
                effects: Default::default(),
                state_hashes,
                machine: machine.clone(),
                spec: WeaponMeleeSpec::default(),
                pose: BladePose::empty(),
                offhand: None,
            })],
            heroes: vec![None, None],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; 2],
            life_serial: 0,
            tick: 0,
        };

        let mut body = Soldier {
            max_speed: 6.0,
            max_strafe: 5.0,
            max_jump: 5.0,
            max_turn: 6.0,
            max_pitch: 6.0,
            accel: 40.0,
            dive_time: 0.6,
            pitch_limits: [-0.8, 1.1],
            grounded: true,
            normal: [0.0, 1.0, 0.0],
            mover: -1.0,
            ..Soldier::default()
        };
        let model = TurnModel {
            tuning,
            max_speed: body.max_speed,
            machine: Some(machine),
        };
        let fire = ComboButton::Fire.bit();

        // Build scripted command schedule covering stand, crouch, walk + sprint
        // start from rest, roll, jump -> fall -> landing, and combo states with
        // TurnFactor + timed InputLock("Turn").
        let mut script: Vec<(&'static str, MoveInput, Controls)> = Vec::new();
        let mut push = |phase: &'static str, count: u32, m: MoveInput, c: Controls| {
            for _ in 0..count {
                script.push((phase, m.clone(), c));
            }
        };
        // Bootstrap + stand idle.
        push("stand", 5, MoveInput::default(), Controls::default());
        // Toggle crouch, stay crouched, toggle back to stand.
        push("crouch_enter", 1, MoveInput { crouch: true, ..MoveInput::default() }, Controls::default());
        push("crouch_hold", 6, MoveInput::default(), Controls::default());
        push("crouch_exit", 1, MoveInput { crouch: true, ..MoveInput::default() }, Controls::default());
        push("stand_after_crouch", 3, MoveInput::default(), Controls::default());
        // Sprint start from rest: hold forward + sprint while velocity ramps past 0.9 * max_speed.
        push(
            "sprint_from_rest",
            30,
            MoveInput { forward: 1.0, ..MoveInput::default() },
            Controls { sprint: true, ..Controls::default() },
        );
        // Whip the view round while sprinting (a look between ticks): the
        // speed along the new facing ends the sprint; then face forward again.
        push(
            "sprint_whip",
            3,
            MoveInput { forward: 1.0, ..MoveInput::default() },
            Controls { sprint: true, ..Controls::default() },
        );
        push(
            "sprint_unwhip",
            2,
            MoveInput { forward: 1.0, ..MoveInput::default() },
            Controls { sprint: true, ..Controls::default() },
        );
        // Roll forward while moving, then let roll finish.
        push(
            "roll_press",
            1,
            MoveInput { forward: 1.0, ..MoveInput::default() },
            Controls { roll: true, ..Controls::default() },
        );
        push("roll_run", 36, MoveInput { forward: 1.0, ..MoveInput::default() }, Controls::default());
        // Stop briefly so jump launches cleanly on flat floor.
        push("pre_jump", 6, MoveInput::default(), Controls::default());
        // Jump, fly through apex and fall back onto the collision floor, then stand.
        push("jump_press", 1, MoveInput { jump: true, ..MoveInput::default() }, Controls::default());
        push("jump_air_and_land", 70, MoveInput::default(), Controls::default());
        // Combo: Fire -> SLASH (0.5) -> RECOVER (0.1) -> IDLE (1.0).
        push(
            "combo_slash",
            1,
            MoveInput::default(),
            Controls { melee_pressed: fire, melee_held: fire, ..Controls::default() },
        );
        push("combo_follow", 32, MoveInput::default(), Controls::default());
        // Timed InputLock("Turn"): FireSecondary -> LOCK_TURN (0.0 for 0.15 s, then 1.0) -> IDLE.
        push(
            "combo_lock_turn",
            1,
            MoveInput::default(),
            Controls { block_pressed: true, block: true, ..Controls::default() },
        );
        push("combo_lock_follow", 26, MoveInput::default(), Controls::default());
        // Walk off the z=22 ledge into Air::Fall, drop onto the lower deck, and land.
        push("ledge_fall", 200, MoveInput { forward: 1.0, ..MoveInput::default() }, Controls::default());
        push("lower_stand", 10, MoveInput::default(), Controls::default());

        let mut seen_stand = 0u32;
        let mut seen_crouch = 0u32;
        let mut seen_pre_sprint_walk = 0u32;
        let mut seen_sprint = 0u32;
        let mut seen_roll = 0u32;
        let mut seen_jump = 0u32;
        let mut seen_fall = 0u32;
        let mut seen_landed = false;
        let mut seen_slash = 0u32;
        let mut seen_recover = 0u32;
        let mut seen_lock_zero = 0u32;
        let mut seen_lock_released = 0u32;

        // Tick 0 bootstraps Life + Sample in Infantry.
        infantry.tick = 1;
        infantry
            .advance(
                &mut movement,
                &mut collision,
                1,
                1,
                class_name,
                1.0,
                true,
                true,
                Controls::default(),
                None,
                None,
                &mut body,
                &MoveInput::default(),
                DT,
                world_id,
                -1,
            )
            .unwrap();

        // The preview predicts from a correction two ticks old: keep each tick's
        // starting body and Sample with its command to replay that lag below.
        let mut history: Vec<(Posture, Option<Air>, [f64; 3], f64, f64, Sample, MoveInput, Controls)> = Vec::new();
        let mut lagged_mismatch: Vec<usize> = Vec::new();
        let mut first_sim_sprint: Option<usize> = None;
        let mut seen_whip_stop = false;
        for (step_idx, (phase, mv, controls)) in script.iter().enumerate() {
            let entering = step_idx == 0 || script[step_idx - 1].0 != *phase;
            if entering && *phase == "sprint_whip" {
                assert!(infantry.sample.unwrap().sprint, "sprinting before the whip");
                body.aim_yaw += 2.2;
            }
            if entering && *phase == "sprint_unwhip" {
                body.aim_yaw -= 2.2;
            }
            let sample_before = infantry.sample.expect("sample after bootstrap");
            let sim_scale_before = infantry
                .mouse_turn_scale(1, class_name, 1.0, &body)
                .expect("sim mouse_turn_scale");
            if *phase == "sprint_from_rest" {
                if sample_before.sprint && first_sim_sprint.is_none() {
                    first_sim_sprint = Some(step_idx);
                }
                if step_idx >= 2 {
                    let (posture, air, velocity, lock, _, sample, _, _) = &history[step_idx - 2];
                    let mut lagged = TurnPredictor::from_sample(*posture, *air, *velocity, *lock, sample);
                    if step_idx >= 3 {
                        let before = history[step_idx - 3].2;
                        lagged.velocity_change_xz = [velocity[0] - before[0], velocity[2] - before[2]];
                    }
                    for back in [2, 1] {
                        let (_, _, _, _, yaw, _, m, c) = &history[step_idx - back];
                        lagged.step(&model, m.forward, m.right, m.jump, m.crouch, *c, *yaw, DT);
                    }
                    if (lagged.scale(&model) - sim_scale_before).abs() > 1e-12 {
                        lagged_mismatch.push(step_idx);
                    }
                }
            }
            history.push((body.posture, body.air, body.velocity, body.lock, body.aim_yaw, sample_before, mv.clone(), *controls));
            let was_air = body.air;
            let mut pred = TurnPredictor::from_sample(
                body.posture,
                body.air,
                body.velocity,
                body.lock,
                &sample_before,
            );
            let pred_scale_before = pred.scale(&model);
            assert!(
                (pred_scale_before - sim_scale_before).abs() < 1e-12,
                "tick {step_idx} ({phase}) before-step scale mismatch: pred={pred_scale_before} sim={sim_scale_before} (posture={:?} air={:?})",
                body.posture, body.air
            );

            infantry.tick = infantry.tick.saturating_add(1);
            infantry
                .advance(
                    &mut movement,
                    &mut collision,
                    1,
                    1,
                    class_name,
                    1.0,
                    true,
                    true,
                    *controls,
                    None,
                    None,
                    &mut body,
                    mv,
                    DT,
                    world_id,
                    -1,
                )
                .unwrap();
            pred.step(
                &model,
                mv.forward,
                mv.right,
                mv.jump,
                mv.crouch,
                *controls,
                history[step_idx].4,
                DT,
            );

            let sim_scale_after = infantry
                .mouse_turn_scale(1, class_name, 1.0, &body)
                .expect("sim mouse_turn_scale after");
            let terrain_air_edge = (was_air.is_some() && body.air.is_none())
                || (was_air.is_none() && body.air == Some(Air::Fall));
            if was_air.is_some() && body.air.is_none() {
                seen_landed = true;
            }
            if terrain_air_edge {
                // Touchdown and walking off a ledge come from world terrain
                // collision; verify the next correction's from_sample matches.
                let sample_after = infantry.sample.unwrap();
                let post_edge = TurnPredictor::from_sample(
                    body.posture,
                    body.air,
                    body.velocity,
                    body.lock,
                    &sample_after,
                );
                assert!(
                    (post_edge.scale(&model) - sim_scale_after).abs() < 1e-12,
                    "tick {step_idx} ({phase}) terrain-edge scale mismatch: pred={} sim={sim_scale_after}",
                    post_edge.scale(&model)
                );
            } else {
                let pred_scale_after = pred.scale(&model);
                assert!(
                    (pred_scale_after - sim_scale_after).abs() < 1e-12,
                    "tick {step_idx} ({phase}) after-step scale mismatch: pred={pred_scale_after} sim={sim_scale_after} (posture={:?} air={:?} sprint={} roll={})",
                    body.posture,
                    body.air,
                    infantry.sample.unwrap().sprint,
                    infantry.sample.unwrap().roll.is_some()
                );
            }

            let sample_after = infantry.sample.unwrap();
            match *phase {
                "stand" if (sim_scale_after - 1.0).abs() < 1e-12 => seen_stand += 1,
                "crouch_hold" if body.posture == Posture::Crouch && (sim_scale_after - 0.85).abs() < 1e-12 => seen_crouch += 1,
                "sprint_from_rest" => {
                    if !sample_after.sprint && (sim_scale_after - 1.0).abs() < 1e-12 {
                        seen_pre_sprint_walk += 1;
                    } else if sample_after.sprint && (sim_scale_after - 0.5).abs() < 1e-12 {
                        seen_sprint += 1;
                    }
                }
                "sprint_whip" if !sample_after.sprint => seen_whip_stop = true,
                "roll_run" if sample_after.roll.is_some() && (sim_scale_after - 0.35).abs() < 1e-12 => seen_roll += 1,
                "jump_air_and_land" => {
                    if body.air == Some(Air::Jump) && (sim_scale_after - 0.6).abs() < 1e-12 {
                        seen_jump += 1;
                    }
                }
                "ledge_fall" => {
                    if body.air == Some(Air::Fall) && (sim_scale_after - 0.6).abs() < 1e-12 {
                        seen_fall += 1;
                    }
                }
                "combo_follow" => {
                    if (sim_scale_after - 0.5).abs() < 1e-12 {
                        seen_slash += 1;
                    } else if (sim_scale_after - 0.1).abs() < 1e-12 {
                        seen_recover += 1;
                    }
                }
                "combo_lock_follow" => {
                    if sim_scale_after == 0.0 {
                        seen_lock_zero += 1;
                    } else if (sim_scale_after - 1.0).abs() < 1e-12 && seen_lock_zero > 0 {
                        seen_lock_released += 1;
                    }
                }
                _ => {}
            }
        }

        assert!(seen_stand > 0, "never observed stand scale");
        assert!(seen_crouch > 0, "never observed crouch scale");
        assert!(seen_pre_sprint_walk >= 3, "never observed pre-sprint speed ramp ({seen_pre_sprint_walk})");
        assert!(seen_sprint >= 5, "never observed active sprint ({seen_sprint})");
        assert!(seen_roll >= 5, "never observed active roll ({seen_roll})");
        assert!(seen_jump >= 5, "never observed Air::Jump ({seen_jump})");
        assert!(seen_fall >= 5, "never observed Air::Fall ({seen_fall})");
        assert!(seen_landed, "soldier never landed back on collision floor");
        assert!(seen_whip_stop, "the whip never ended the sprint");
        assert!(seen_slash >= 5, "never observed combo SLASH TurnFactor=0.5 ({seen_slash})");
        assert!(seen_recover >= 5, "never observed combo RECOVER TurnFactor=0.1 ({seen_recover})");
        assert!(seen_lock_zero >= 5, "never observed InputLock(Turn) 0.0 window ({seen_lock_zero})");
        assert!(seen_lock_released >= 5, "never observed InputLock(Turn) release to 1.0 ({seen_lock_released})");
        // Two ticks behind the simulation, with the last two corrections'
        // velocity change, the prediction meets the sprint's start on time.
        assert!(first_sim_sprint.is_some(), "the simulation sprinted");
        assert!(
            lagged_mismatch.is_empty(),
            "lagged prediction differs from the simulation while sprinting from rest: {lagged_mismatch:?} (sprint from {first_sim_sprint:?})"
        );
    }

    #[test]
    fn turn_predictor_matches_sim_tumble_and_getup_transitions() {
        use crate::collision::CollisionOwner;
        use crate::gen::{Air as GenAir, Posture as GenPosture};
        use crate::movement::soldier::{
            swbf2::{Actions, State as Swbf2State, Step, Tumble, TumblePhase},
            Air as MoveAir, Input as MoveInput, Posture as MovePosture, V3,
        };
        use crate::movement::MovementOwner;
        use bfe_content::{
            bvh::{build, Positions},
            collision::{triangle_bounds, MapCollision, TriangleGrid},
        };

        let floor = vec![
            -200.0f32, 0.0, -200.0, 200.0, 0.0, -200.0, 200.0, 0.0, 200.0, -200.0, 0.0, 200.0,
        ];
        let building = vec![
            -6.0f32, -100.0, -6.0, 6.0, -100.0, -6.0, 6.0, -100.0, 6.0, -6.0, -100.0, 6.0,
        ];
        let indices = vec![0u32, 2, 1, 0, 3, 2];
        let buildings = build(Positions::F32(&building), &indices);
        let terrain = build(Positions::F32(&floor), &indices);
        let map = MapCollision {
            tri_bounds: triangle_bounds(&building, &indices),
            grid: Some(TriangleGrid::new(&floor, &indices)),
            positions: building.into(),
            indices: indices.clone().into(),
            polygons: vec![0, 1].into(),
            masks: Vec::new().into(),
            has_terrain: true,
            terrain_positions: floor.into(),
            terrain_indices: indices.into(),
            kill_y: -200.0,
        };

        let cs = |turn| ControlSpeed { forward: 1.0, strafe: 1.0, turn };
        for recover_from_tumble in [false, true] {
            let tuning = Tuning {
                stand: cs(1.0),
                crouch: cs(0.85),
                sprint: cs(0.5),
                jump: cs(0.6),
                jet: cs(1.25),
                jetpack: None,
                roll: cs(0.35),
                tumble: cs(0.12),
                sprint_accelerate_time: 0.25,
                energy_capacity: 100.0,
                energy_floor: 0.0,
                energy_restore: 10.0,
                energy_restore_idle: 15.0,
                energy_drain_sprint: 10.0,
                energy_min_sprint: 10.0,
                energy_cost_jump: 5.0,
                energy_cost_jump_sprint: None,
                energy_cost_roll: 20.0,
                jump_height: 2.0,
                jump_forward_speed_factor: 1.0,
                jump_strafe_speed_factor: 1.0,
                roll_speed_factor: 1.5,
                roll_frames: 20,
                recover_from_tumble,
                getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
                getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
            };
            let model = TurnModel {
                tuning,
                max_speed: 6.0,
                machine: None,
            };
            let mut col = CollisionOwner::new();
            let world = col.native_install(&map, &buildings, Some(&terrain)).unwrap().world as usize;
            let mut movement = MovementOwner::new();
            movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
            let id = movement.new_mover(world, |m| {
                m.grounded = false;
                m.air = MoveAir::Thrown;
                m.velocity = if recover_from_tumble {
                    V3::new(0.0, 14.0, 4.0)
                } else {
                    V3::new(0.0, 5.5, 13.0)
                };
                m.max_speed = 6.0;
                m.max_strafe = 5.0;
            });
            let mut state = Swbf2State::new(&tuning);
            let v0 = movement.mover(id).velocity;
            state.tumble = Some(Tumble::enter(v0, 0.0, 1));
            let dt = 1.0 / 60.0;

            let mut saw_thrown = false;
            let mut saw_tumble = false;
            let mut saw_landed = false;
            let mut saw_getup = false;
            let mut saw_recover = false;

            for tick in 0..300 {
                let m = movement.mover(id);
                let posture = match m.posture {
                    MovePosture::Stand => GenPosture::Stand,
                    MovePosture::Crouch => GenPosture::Crouch,
                    MovePosture::Prone => GenPosture::Prone,
                };
                let air = match m.air {
                    MoveAir::None => None,
                    MoveAir::Jump => Some(GenAir::Jump),
                    MoveAir::Fall => Some(GenAir::Fall),
                    MoveAir::Thrown => Some(GenAir::Thrown),
                    MoveAir::Jet => Some(GenAir::Jet),
                };
                let sample = Sample {
                    id: 1,
                    generation: 1.0,
                    energy: state.energy,
                    exhausted: state.exhausted,
                    sprint: state.sprint_time.is_some(),
                    sprint_time: state.sprint_time,
                    roll: state.roll,
                    roll_seconds: tuning.roll_seconds(),
                    jump_count: state.jump_count,
                    land_cooldown: state.land_cooldown,
                    tumble: state.tumble.map(|t| t.phase),
                    tumble_state: state.tumble,
                    combo: None,
                    state_time: 0.0,
                    anim_time: 0.0,
                    blades_off: 0,
                    turn_factor: 1.0,
                    swing_turn: 0.0,
                    saber: None,
                    combo_state: None,
                    buttons_armed: 0,
                    saber_locked: false,
                };
                let mut pred = TurnPredictor::from_sample(posture, air, m.velocity.arr(), m.lock, &sample);
                let sim_scale = look_turn_scale(
                    tuning.turn_scales(),
                    posture,
                    air,
                    state.sprint_time.is_some(),
                    state.roll.is_some(),
                    state.is_tumble_look(),
                    1.0,
                );
                assert!(
                    (pred.scale(&model) - sim_scale).abs() < 1e-12,
                    "recover={recover_from_tumble} tick={tick}: pred scale != sim scale"
                );
                match state.tumble.map(|t| t.phase) {
                    Some(TumblePhase::Thrown) => saw_thrown = true,
                    Some(TumblePhase::Tumble) => saw_tumble = true,
                    Some(TumblePhase::Landed) => saw_landed = true,
                    Some(TumblePhase::GetUp) => saw_getup = true,
                    Some(TumblePhase::Recover) => saw_recover = true,
                    None => {}
                }
                let phase_before = state.tumble.map(|t| t.phase);
                pred.step(&model, 0.0, 0.0, false, false, Controls::default(), 0.0, dt);
                movement
                    .advance_profiled(
                        &mut col.state,
                        id,
                        &MoveInput::default(),
                        dt,
                        Some(Step {
                            actions: Actions::default(),
                            tuning: &tuning,
                            state: &mut state,
                            override_: None,
                        }),
                    )
                    .unwrap();
                let m_after = movement.mover(id);
                let phase_after = state.tumble.map(|t| t.phase);
                // Every non-terrain-contact transition must match TurnPredictor on the exact same tick.
                let terrain_contact = matches!(
                    (phase_before, phase_after),
                    (Some(TumblePhase::Thrown), Some(TumblePhase::Tumble | TumblePhase::Landed))
                        | (Some(TumblePhase::Recover), None)
                );
                if !terrain_contact && (phase_before.is_some() || phase_after.is_some()) {
                    assert_eq!(
                        pred.movement.tumble.map(|t| t.phase),
                        phase_after,
                        "recover={recover_from_tumble} tick={tick}: phase mismatch ({phase_before:?} -> {phase_after:?})"
                    );
                    assert_eq!(
                        pred.lock > 0.0,
                        m_after.lock > 0.0,
                        "recover={recover_from_tumble} tick={tick}: lock mismatch pred={} sim={}",
                        pred.lock,
                        m_after.lock
                    );
                    if m_after.lock > 0.0 {
                        assert!(
                            (pred.lock - m_after.lock).abs() < 1e-9,
                            "recover={recover_from_tumble} tick={tick}: lock drift pred={} sim={}",
                            pred.lock,
                            m_after.lock
                        );
                    }
                }
                if state.tumble.is_none() && m_after.grounded {
                    break;
                }
            }
            assert!(saw_thrown);
            if recover_from_tumble {
                assert!(saw_recover);
            } else {
                assert!(saw_tumble && saw_landed && saw_getup);
            }
        }
    }

    fn flat_turn_tuning() -> Tuning {
        let row = |turn| ControlSpeed { forward: 1.0, strafe: 1.0, turn };
        Tuning {
            stand: row(1.0),
            crouch: row(0.7),
            sprint: row(0.4),
            jump: row(0.55),
            jet: row(0.9),
            jetpack: None,
            roll: row(0.3),
            tumble: row(0.2),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 12.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            getup_front_seconds: 1.5,
            getup_back_seconds: 1.25,
            recover_from_tumble: true,
        }
    }

    fn flat_turn_sample(tuning: &Tuning) -> Sample {
        Sample {
            id: 1,
            generation: 1.0,
            energy: tuning.energy_capacity,
            exhausted: false,
            sprint: false,
            sprint_time: None,
            roll: None,
            roll_seconds: tuning.roll_seconds(),
            jump_count: 0,
            land_cooldown: 0.0,
            tumble: None,
            tumble_state: None,
            combo: None,
            state_time: 0.0,
            anim_time: 0.0,
            blades_off: 0,
            turn_factor: 1.0,
            swing_turn: 0.0,
            saber: None,
            combo_state: None,
            buttons_armed: 0,
            saber_locked: false,
        }
    }

    #[test]
    fn look_turn_scale_gives_a_thrown_or_tumbling_soldier_the_tumble_column_whatever_the_combo() {
        use crate::gen::{Air, Posture};
        let scales = flat_turn_tuning().turn_scales();
        // Tumbling flag alone, thrown air alone, and both: the combo factor never reaches them.
        assert_eq!(look_turn_scale(scales, Posture::Crouch, None, false, false, true, 0.5), scales.tumble);
        assert_eq!(look_turn_scale(scales, Posture::Stand, Some(Air::Thrown), false, false, false, 0.5), scales.tumble);
        assert_eq!(look_turn_scale(scales, Posture::Stand, Some(Air::Thrown), true, true, true, 0.25), scales.tumble);
        // Any other state: its own column times the combo factor.
        assert_eq!(look_turn_scale(scales, Posture::Stand, None, false, false, false, 0.5), 0.5 * scales.stand);
        assert_eq!(look_turn_scale(scales, Posture::Crouch, None, false, false, false, 0.5), 0.5 * scales.crouch);
        assert_eq!(look_turn_scale(scales, Posture::Stand, Some(Air::Fall), false, false, false, 0.5), 0.5 * scales.jump);
        assert_eq!(look_turn_scale(scales, Posture::Stand, None, true, false, false, 0.5), 0.5 * scales.sprint);
        assert_eq!(look_turn_scale(scales, Posture::Stand, None, false, true, false, 0.5), 0.5 * scales.roll);
    }

    #[test]
    fn turn_predictor_holds_a_roll_back_until_the_landing_lock_has_run_out() {
        use crate::gen::Posture;
        let tuning = flat_turn_tuning();
        let model = TurnModel { tuning, max_speed: 6.0, machine: None };
        let mut sample = flat_turn_sample(&tuning);
        // Three ticks of lock left: the roll press is refused on each of them.
        sample.land_cooldown = 3.0 / 60.0 - 1e-12;
        let mut pred = TurnPredictor::from_sample(Posture::Stand, None, [0.0, 0.0, 3.0], 0.0, &sample);
        let press = Controls { roll: true, ..Controls::default() };
        let dt = 1.0 / 60.0;
        for tick in 0..3 {
            pred.step(&model, 1.0, 0.0, false, false, press, 0.0, dt);
            assert!(pred.movement.roll.is_none(), "roll started under the landing lock at tick {tick}");
            assert_eq!(pred.scale(&model), tuning.stand.turn);
        }
        assert_eq!(pred.movement.land_cooldown, 0.0, "a lock a rounding error short of zero must end");
        pred.step(&model, 1.0, 0.0, false, false, press, 0.0, dt);
        assert!(pred.movement.roll.is_some());
        assert_eq!(pred.scale(&model), tuning.roll.turn);
    }

    #[test]
    fn turn_predictor_ends_a_look_lock_that_a_tick_leaves_a_rounding_error_short_of_zero() {
        use crate::gen::Posture;
        let tuning = flat_turn_tuning();
        let model = TurnModel { tuning, max_speed: 6.0, machine: None };
        let sample = flat_turn_sample(&tuning);
        let dt = 1.0 / 60.0;
        let mut pred = TurnPredictor::from_sample(Posture::Stand, None, [0.0; 3], dt + 1e-12, &sample);
        assert!(pred.lock > 0.0);
        pred.step(&model, 0.0, 0.0, false, false, Controls::default(), 0.0, dt);
        assert_eq!(pred.lock, 0.0);
    }
}
