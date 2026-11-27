//! Numeric movement, weapon and combo choices, resolved at character admission.
use super::{array, field, Character};
use bfe_content::{zero_hash, Value};
use bfe_pose::character::stance::{layers, stance_chain, RollSlot};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Forward,
    Backward,
    Left,
    Right,
}
const DIRECTIONS: [Direction; 4] = [
    Direction::Forward,
    Direction::Backward,
    Direction::Left,
    Direction::Right,
];
impl Direction {
    fn opposite(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
    pub fn from_velocity(velocity: [f64; 3], yaw: f64) -> Self {
        let (sin, cos) = yaw.sin_cos();
        // The movement kernel faces +Z at yaw zero. Right is facing cross up.
        let forward = velocity[0] * sin + velocity[2] * cos;
        let right = velocity[2] * sin - velocity[0] * cos;
        if right.abs() > forward.abs() {
            if right < 0. {
                Self::Left
            } else {
                Self::Right
            }
        } else if forward < 0. {
            Self::Backward
        } else {
            Self::Forward
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Air {
    #[default]
    Ground,
    Jump,
    Fall,
    Jet,
    Land,
    Thrown,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TumblePhase {
    #[default]
    None,
    Thrown,
    Tumble,
    Landed,
    GetUp,
    Recover,
    RestDead,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TumblePose {
    pub phase: TumblePhase,
    pub front: bool,
    pub bounce: bool,
    pub fly_dir: u8,
    pub elapsed: f64,
    pub angle: f64,
    pub heading: f64,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    #[default]
    None,
    Fire,
    FireSecondary,
    FireSecondary2,
    Throw,
    Reload,
    Hit,
    Death,
}
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub tick: u32,
    pub discontinuity: bool,
    pub position: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    /// Direction towards the source of a hit in body space, when published.
    pub impact: Option<Direction>,
    pub hard_landing: bool,
    pub seat: Option<(u32, u32, usize)>,
    pub seat_clip: Option<usize>,
    pub seat_time: f64,
    pub choke_clip: Option<usize>,
    pub choke_time: f64,
    pub velocity: [f64; 3],
    pub crouch: bool,
    pub sprint: bool,
    pub roll: Option<Direction>,
    /// The duration of the active roll in the movement profile.
    pub roll_seconds: Option<f64>,
    pub air: Air,
    pub action: Action,
    pub weapon: usize,
    pub combo: Option<usize>,
    pub elapsed: f64,
    pub phase_time: Option<f64>,
    pub anim_time: f64,
    pub blades_off: u8,
    /// Yaw the combo state turns the swing and body by, relative to `yaw`
    /// (radians, positive turns left).
    pub swing_turn: f64,
    pub reaction: u8,
    /// Monotonic jump/double-jump counter for restarting airborne jump clips.
    pub jump_count: u32,
    pub contact_clip: Option<usize>,
    /// The soldier holds its weapon ready: idle, turn, walk and run play
    /// their alert rows where the stance has them.
    pub alert: bool,
    #[serde(skip)]
    pub tumble: Option<TumblePose>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Idle,
    Walk,
    Run,
    Sprint,
    Roll,
    Jump,
    NeutralJump,
    Fall,
    Jet,
    Land,
    HardLand,
    NeutralHit,
    NeutralDeath,
    Fire,
    FireSecondary,
    FireSecondary2,
    Reload,
    Hit,
    Death,
}
const MOTIONS: [Motion; 19] = [
    Motion::Idle,
    Motion::Walk,
    Motion::Run,
    Motion::Sprint,
    Motion::Roll,
    Motion::Jump,
    Motion::NeutralJump,
    Motion::Fall,
    Motion::Jet,
    Motion::Land,
    Motion::HardLand,
    Motion::NeutralHit,
    Motion::NeutralDeath,
    Motion::Fire,
    Motion::FireSecondary,
    Motion::FireSecondary2,
    Motion::Reload,
    Motion::Hit,
    Motion::Death,
];
const PROVISIONAL_BLEND: f64 = 0.1;
const PROVISIONAL_RUN_FRACTION: f64 = 0.6;
fn number(v: &Value, k: &str) -> Option<f64> {
    v.get(k)
        .and_then(|n| n.as_f64().or_else(|| n.as_str()?.parse().ok()))
        .filter(|n| n.is_finite())
}
fn prop<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).or_else(|| {
        array(v, "properties")
            .iter()
            .filter_map(Value::as_array)
            .find_map(|a| {
                (a.first()?.as_str()?.eq_ignore_ascii_case(k))
                    .then(|| a.get(1)?.as_str())
                    .flatten()
            })
    })
}
fn weapon_stance(w: &Value) -> Option<&str> {
    prop(w, "AnimationBank")
        .or_else(|| {
            let bank = field(array(w, "comboBindings").first()?, "bank");
            bank.rsplit('_').next()
        })
        .filter(|s| !s.is_empty())
}
fn names(m: Motion, d: Direction, crouch: bool, alert: bool) -> Vec<String> {
    let stance = match (crouch, alert) {
        (false, false) => "stand",
        (true, false) => "crouch",
        (false, true) => "standalert",
        (true, true) => "crouchalert",
    };
    let upright = if alert { "standalert" } else { "stand" };
    let dir = d.suffix();
    match m {
        Motion::Idle => vec![format!("{stance}_idle_emote"), format!("{stance}_idle")],
        Motion::Walk | Motion::Run => {
            let g = if m == Motion::Walk { "walk" } else { "run" };
            let other = if m == Motion::Walk { "run" } else { "walk" };
            let mut forms = vec![
                format!("{stance}_{g}{dir}"),
                format!("{stance}_{other}{dir}"),
            ];
            if crouch {
                forms.extend([format!("{upright}_{g}{dir}"), format!("{upright}_{other}{dir}")]);
            }
            forms
        }
        Motion::Sprint => vec!["sprint".into()],
        Motion::Roll => Vec::new(),
        Motion::Jump => vec![format!("jump_{dir}"), "jump".into()],
        Motion::NeutralJump => vec!["jump".into()],
        Motion::Fall => vec!["fall".into()],
        Motion::Jet => vec!["jetpack_hover".into(), "fall".into()],
        Motion::Land => vec!["landsoft".into(), "landhard".into()],
        Motion::HardLand => vec!["landhard".into(), "landsoft".into()],
        Motion::NeutralHit => vec![
            format!("{stance}_hit"),
            "hit".into(),
            "stand_hitfront".into(),
        ],
        Motion::NeutralDeath => vec![
            "die".into(),
            format!("{stance}_death"),
            "stand_death_backward".into(),
            "stand_deadhero".into(),
            "thrown_landbacksoft".into(),
            "thrown_landfrontsoft".into(),
        ],
        Motion::Fire => vec![format!("{stance}_shoot")],
        Motion::FireSecondary => {
            let mut forms = vec![format!("{stance}_shoot_secondary")];
            if crouch {
                forms.push("stand_shoot_secondary".into());
            }
            forms
        }
        Motion::FireSecondary2 => {
            let mut forms = vec![format!("{stance}_shoot_secondary2")];
            if crouch {
                forms.push("stand_shoot_secondary2".into());
            }
            forms
        }
        Motion::Reload => vec![format!("{stance}_reload")],
        Motion::Hit => {
            let side = match d {
                Direction::Forward => "front",
                Direction::Backward => "back",
                _ => dir,
            };
            vec![
                format!("{stance}_hit{side}"),
                format!("{stance}_hit_{dir}"),
                format!("stand_hit{side}"),
                format!("{stance}_hit"),
            ]
        }
        Motion::Death => vec![
            format!("{stance}_death_{dir}"),
            "stand_deadhero".into(),
            "stand_death_backward".into(),
            "die".into(),
            // Skeletons without a dedicated death use their own grounded fall.
            "thrown_landbacksoft".into(),
            "thrown_landfrontsoft".into(),
        ],
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AimType {
    None,
    Head,
    #[default]
    Torso,
    FullBody,
}

impl AimType {
    /// Target `[torso, head]` aim pitch weights for this mode.
    pub fn weights(self) -> [f64; 2] {
        match self {
            Self::Torso => [1., 0.],
            Self::Head => [0., 1.],
            Self::None | Self::FullBody => [0., 0.],
        }
    }

    /// Whether this mode locks lower-body yaw to the aim direction.
    pub fn locks_body_yaw(self) -> bool {
        matches!(self, Self::FullBody)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub base: Option<usize>,
    pub lower: Option<usize>,
    pub upper: Option<usize>,
    pub blend_in: f64,
    pub blend_out: f64,
    pub repeats: bool,
    pub attack: bool,
    pub full_body: bool,
    pub aims: bool,
    pub inherited_gait: bool,
    pub turn: f64,
    pub aim_type: AimType,
}
impl Default for Row {
    fn default() -> Self {
        Self {
            base: None,
            lower: None,
            upper: None,
            blend_in: PROVISIONAL_BLEND,
            blend_out: PROVISIONAL_BLEND,
            repeats: false,
            attack: false,
            full_body: false,
            aims: false,
            inherited_gait: false,
            turn: 0.,
            aim_type: AimType::Torso,
        }
    }
}
impl Row {
    fn exists_stride(self) -> bool {
        self.lower.or(self.base).is_some()
    }
    fn exists(self) -> bool {
        self.base.is_some() || self.lower.is_some() || self.upper.is_some()
    }
}
struct Weapon {
    motions: Vec<Row>,
    combo: Vec<(Row, [Option<usize>; 3])>,
    offhand: [Row; 4],
    throw_anim: Row,
    transitions: HashMap<(usize, usize), f64>,
    saber: bool,
    offhand_weapon: bool,
    fire_anim: u8,
    instant_anim: bool,
}
#[derive(Clone, Copy, Default)]
struct TumbleRows {
    flying: [Row; 4],
    flail: Row,
    tumble: [Row; 2],
    bounce: [Row; 2],
    land: [Row; 2],
    getup: [Row; 2],
    rest: [Row; 2],
}
pub struct Tables {
    weapons: Vec<Weapon>,
    tumble: TumbleRows,
    run_speed: f64,
    directional_jumps: bool,
    clip_names: Vec<String>,
}
#[derive(Clone, Copy)]
pub struct Sample {
    pub motion: Row,
    pub action: Option<Row>,
    pub motion_time: f64,
    pub rate: f64,
    pub action_time: f64,
    pub travel_yaw: f64,
    pub backward: bool,
    pub(super) gait_clock: bool,
    pub(super) combo_action: bool,
    pub(super) tumble_turn: Option<f64>,
    /// A hit taken while a combo row plays: the hit row, its time and its weight,
    /// blended over the combo pose and never in place of it.
    pub overlay: Option<(Row, f64, f64)>,
}
/// A hit reaction over a melee combo pose starts at half weight and fades
/// out at 8 per second, so it is gone after 0.125 s.
const HIT_OVERLAY_WEIGHT: f64 = 0.5;
const HIT_OVERLAY_FADE: f64 = 8.;
fn wrapped(angle: f64) -> f64 {
    angle.sin().atan2(angle.cos())
}
/// The mode a block names, or none when it names no mode.
fn aim_type(block: &Value) -> AimType {
    for r in array(block, "children") {
        let key = field(r, "key");
        if key == "AimType" || key == zero_hash::fnv1a_hex("AimType") {
            return match array(r, "values").first().and_then(Value::as_str) {
                Some(v) if v.eq_ignore_ascii_case("Head") => AimType::Head,
                Some(v) if v.eq_ignore_ascii_case("Torso") => AimType::Torso,
                Some(v) if v.eq_ignore_ascii_case("FullBody") => AimType::FullBody,
                _ => AimType::None,
            };
        }
    }
    AimType::None
}
fn tuning(row: &mut Row, block: &Value) {
    row.aim_type = aim_type(block);
    for r in array(block, "children") {
        let key = field(r, "key");
        let val = array(r, "values").first();
        if key == "BlendInTime" || key == zero_hash::fnv1a_hex("BlendInTime") {
            if let Some(n) = val.and_then(Value::as_f64) {
                row.blend_in = n.max(0.);
            }
        }
        if key == "BlendOutTime" || key == zero_hash::fnv1a_hex("BlendOutTime") {
            if let Some(n) = val.and_then(Value::as_f64) {
                row.blend_out = n.max(0.);
            }
        }
        if key == "Loop" || key == zero_hash::fnv1a_hex("Loop") {
            row.repeats = val.is_some_and(|v| v.as_f64() == Some(1.) || v.as_str() == Some("Wrap"));
        }
        if key == "AimType" || key == zero_hash::fnv1a_hex("AimType") {
            row.aims = val
                .and_then(Value::as_str)
                .is_some_and(|v| v.eq_ignore_ascii_case("Torso"));
        }
    }
}
fn blend_targets(
    block: &Value,
    source: usize,
    resolve: impl Fn(&str) -> Option<usize>,
    transitions: &mut HashMap<(usize, usize), f64>,
) {
    for entry in array(block, "children") {
        let key = field(entry, "key");
        if key != "BlendTimeTo" && key != zero_hash::fnv1a_hex("BlendTimeTo") {
            continue;
        }
        let values = array(entry, "values");
        if let (Some(target), Some(seconds)) = (
            values.first().and_then(Value::as_str).and_then(&resolve),
            values.get(1).and_then(Value::as_f64),
        ) {
            if seconds.is_finite() && seconds >= 0. {
                transitions.insert((source, target), seconds);
            }
        }
    }
}
impl Tables {
    pub fn new(c: &Character, def: &Value, sets: &Value, machines: &Value) -> Result<Self, String> {
        Self::with_roll_slot(c, def, sets, machines, None)
    }

    pub(super) fn with_roll_slot(
        c: &Character,
        def: &Value,
        sets: &Value,
        machines: &Value,
        profile: Option<RollSlot<'_>>,
    ) -> Result<Self, String> {
        let mut config = HashMap::new();
        for (_, set) in sets.as_object().into_iter().flat_map(|o| o.entries()) {
            for r in set.as_array().unwrap_or(&[]) {
                if let Some(name) = array(r, "values").first().and_then(Value::as_str) {
                    config.insert(name, r);
                }
            }
        }
        let empty = Value::Null;
        let options = array(def, "weapons");
        // An ability with no stance leaves a saber wielder in the bound saber stance.
        let saber_weapon = options.iter().find(|w| {
            !array(w, "saberHilts").is_empty()
                || weapon_stance(w).is_some_and(|s| s.to_ascii_lowercase().contains("sabr"))
        });
        let hero = number(def, "FleeLikeAHero")
            .or_else(|| prop(def, "FleeLikeAHero")?.parse().ok())
            .is_some_and(|v| v != 0.);
        let mut weapons = Vec::new();
        let mut clip_names = vec![String::new(); c.clips.len()];
        let mut record_name = |id: Option<usize>, label: &str| {
            if let Some(i) = id {
                if let Some(slot) = clip_names.get_mut(i) {
                    if slot.is_empty() && !label.is_empty() {
                        *slot = label.to_string();
                    }
                }
            }
        };
        for w in if options.is_empty() {
            std::slice::from_ref(&empty)
        } else {
            options
        } {
            let equipped = if weapon_stance(w).is_some() {
                w
            } else {
                saber_weapon.unwrap_or(w)
            };
            let binding = array(equipped, "comboBindings").first();
            let stance = weapon_stance(equipped).unwrap_or("rifle");
            let parent = binding.map(|b| field(b, "mode"));
            let chain = stance_chain(stance, parent);
            let saber =
                !array(w, "saberHilts").is_empty() || stance.to_ascii_lowercase().contains("sabr");
            let mut motions = Vec::new();
            // All strings and CRC lookups end here. A display sample only indexes rows.
            let mut cache = HashMap::new();
            // The alert half follows the plain half: only idle, walk and run
            // have alert rows; every other entry repeats its plain row.
            for (alert, posture) in [(false, false), (false, true), (true, false), (true, true)] {
                for m in MOTIONS {
                    for d in DIRECTIONS {
                        let plain = (usize::from(posture) * MOTIONS.len() + m as usize) * 4 + d as usize;
                        if alert && !matches!(m, Motion::Idle | Motion::Walk | Motion::Run) {
                            motions.push(motions[plain]);
                            continue;
                        }
                        let mut forms = names(m, d, posture, alert);
                        if hero && !posture && matches!(m, Motion::Death | Motion::NeutralDeath) {
                            forms.insert(0, "stand_deadhero".into());
                        }
                        let mut row = Row {
                            repeats: matches!(
                                m,
                                Motion::Idle
                                    | Motion::Walk
                                    | Motion::Run
                                    | Motion::Sprint
                                    | Motion::Fall
                                    | Motion::Jet
                            ),
                            ..Row::default()
                        };
                        if saber
                            && matches!(
                                m,
                                Motion::Fire
                                    | Motion::FireSecondary
                                    | Motion::FireSecondary2
                                    | Motion::Reload
                            )
                        {
                            motions.push(row);
                            continue;
                        }
                        if m == Motion::Roll {
                            let slot = profile.unwrap_or(RollSlot::Equipped(c, &chain));
                            if let Some(dive) = slot.resolve(d.suffix()) {
                                [row.base, row.lower, row.upper] =
                                    dive.layers.map(|clip| clip.and_then(|k| k.in_rig(c)));
                                for id in [row.base, row.lower, row.upper] {
                                    record_name(id, &dive.form);
                                }
                                if dive.form == "diveforward" {
                                    row.turn = match d {
                                        Direction::Forward => 0.,
                                        Direction::Backward => std::f64::consts::PI,
                                        Direction::Left => std::f64::consts::FRAC_PI_2,
                                        Direction::Right => -std::f64::consts::FRAC_PI_2,
                                    };
                                }
                                if let Some(block) = config
                                    .get(dive.form.as_str())
                                    .or_else(|| config.get(format!("{}_lower", dive.form).as_str()))
                                {
                                    tuning(&mut row, block);
                                }
                            }
                            motions.push(row);
                            continue;
                        }
                        for form in &forms {
                            let reaction = matches!(m, Motion::NeutralHit | Motion::Hit);
                            let gait = matches!(m, Motion::Walk | Motion::Run | Motion::Sprint);
                            let layers = *cache
                                .entry((form.clone(), reaction, gait))
                                .or_insert_with(|| layers(c, &chain, form, reaction, gait));
                            if gait && row.upper.is_some() && !row.exists_stride() {
                                row.lower = layers[1].or(layers[0]);
                            } else {
                                row.base = layers[0];
                                row.lower = layers[1];
                                row.upper = layers[2];
                            }
                            if gait && !row.exists_stride() {
                                continue;
                            }
                            row.inherited_gait = gait
                                && row.lower.is_some()
                                && row.upper.is_some_and(|i| {
                                    c.clips[i].speed <= 1e-4 || !c.clips[i].moves_legs(&c.rig)
                                });
                            if row.exists() {
                                for id in [row.base, row.lower, row.upper] {
                                    record_name(id, form);
                                }
                                if let Some(block) = config.get(form.as_str()) {
                                    tuning(&mut row, block);
                                } else {
                                    let lower_block = config.get(format!("{form}_lower").as_str());
                                    let upper_block = config.get(format!("{form}_upper").as_str());
                                    if matches!(m, Motion::Fire | Motion::Reload | Motion::FireSecondary | Motion::FireSecondary2) {
                                        if let Some(block) = upper_block.or(lower_block) {
                                            tuning(&mut row, block);
                                        }
                                    } else {
                                        if let Some(block) = lower_block {
                                            tuning(&mut row, block);
                                        }
                                        if let Some(block) = upper_block {
                                            row.aim_type = aim_type(block);
                                        }
                                    }
                                }
                                break;
                            }
                        }
                        if alert && !row.exists() {
                            row = motions[plain];
                        }
                        motions.push(row);
                    }
                }
            }
            let mut combo = Vec::new();
            let mut offhand = [Row::default(); 4];
            let mut throw_anim = Row::default();
            let mut transitions = HashMap::new();
            if let Some(binding) = binding {
                let machine = machines
                    .get(field(binding, "combo"))
                    .ok_or("missing equipped combo machine")?;
                for state in array(machine, "states") {
                    let label = array(state, "values")
                        .first()
                        .and_then(Value::as_str)
                        .ok_or("unnamed combo state")?;
                    let full_body = array(state, "children").iter().any(|v| {
                        field(v, "key") == "AnimatedMove"
                            && array(v, "values").first().and_then(Value::as_f64) != Some(0.)
                    });
                    let mut row = Row {
                        attack: array(state, "children")
                            .iter()
                            .any(|v| field(v, "key") == "Attack"),
                        full_body,
                        ..Row::default()
                    };
                    let mut reactions = [None; 3];
                    for a in binding
                        .get("stateAnimations")
                        .and_then(|a| a.get(label))
                        .and_then(Value::as_array)
                        .unwrap_or(&[])
                    {
                        let name = field(a, "name");
                        let id = c
                            .weapon_clip(binding, name)
                            .map_err(|e| format!("combo state {label}: {e}"))?;
                        record_name(Some(id), name);
                        if field(a, "key") == "DeflectAnimation" {
                            let dir = if name.contains("left") {
                                1
                            } else if name.contains("right") {
                                2
                            } else {
                                0
                            };
                            reactions[dir].get_or_insert(id);
                            continue;
                        }
                        if field(a, "key") != "Animation" {
                            continue;
                        }
                        if name.ends_with("_lower") {
                            row.lower = Some(id);
                        } else if name.ends_with("_upper") {
                            row.upper = Some(id);
                        } else {
                            row.base = Some(id);
                        }
                        if let Some(block) = array(machine, "animations").iter().find(|r| {
                            array(r, "values").first().and_then(Value::as_str) == Some(name)
                        }) {
                            tuning(&mut row, block);
                            blend_targets(
                                block,
                                id,
                                |target| c.weapon_clip(binding, target).ok(),
                                &mut transitions,
                            );
                        }
                        row.repeats = field(a, "playback") == "loop";
                    }
                    combo.push((row, reactions));
                }
                let entries = array(machine, "offhandAnimations");
                for entry in entries {
                    let values = array(entry, "values");
                    let Some(name) = values.first().and_then(Value::as_str) else {
                        continue;
                    };
                    let Ok(id) = c.weapon_clip(binding, name) else {
                        continue;
                    };
                    let mut row = Row::default();
                    if name.ends_with("_lower") {
                        row.lower = Some(id);
                    } else if name.ends_with("_upper") {
                        row.upper = Some(id);
                    } else {
                        row.base = Some(id);
                    }
                    if let Some(block) = array(machine, "animations").iter().find(|r| {
                        array(r, "values").first().and_then(Value::as_str) == Some(name)
                    }) {
                        tuning(&mut row, block);
                    }
                    for other in entries {
                        if array(other, "values").first().and_then(Value::as_str) == Some(name)
                            && !array(other, "children").is_empty()
                        {
                            tuning(&mut row, other);
                        }
                    }
                    tuning(&mut row, entry);
                    // Authored offhand slots cascade in order: FIRE fills 0..4,
                    // FIRE2 overrides 1..4, CHARGE overrides 2..4, RELOAD overrides 3.
                    let start = match values.get(1).and_then(Value::as_str) {
                        Some(s) if s.eq_ignore_ascii_case("FIRE2") => 1,
                        Some(s) if s.eq_ignore_ascii_case("CHARGE") => 2,
                        Some(s) if s.eq_ignore_ascii_case("RELOAD") => 3,
                        _ => 0,
                    };
                    for dst in &mut offhand[start..] {
                        *dst = row;
                    }
                }
                if let Some(entry) = array(machine, "throwAnimations").first() {
                    if let Some(name) = array(entry, "values").first().and_then(Value::as_str) {
                        if let Ok(id) = c.weapon_clip(binding, name) {
                            if name.ends_with("_lower") {
                                throw_anim.lower = Some(id);
                            } else if name.ends_with("_upper") {
                                throw_anim.upper = Some(id);
                            } else {
                                throw_anim.base = Some(id);
                            }
                            if let Some(block) = array(machine, "animations").iter().find(|r| {
                                array(r, "values").first().and_then(Value::as_str) == Some(name)
                            }) {
                                tuning(&mut throw_anim, block);
                            }
                            tuning(&mut throw_anim, entry);
                        }
                    }
                }
            }
            let offhand_weapon = number(w, "OffhandWeapon")
                .or_else(|| prop(w, "OffhandWeapon")?.parse().ok())
                .is_some_and(|v| v != 0.);
            let fire_anim = number(w, "FireAnim")
                .or_else(|| prop(w, "FireAnim")?.parse().ok())
                .map_or(0, |v| v.clamp(0., 3.) as u8);
            let instant_anim = number(w, "InstantPlayFireAnim")
                .or_else(|| prop(w, "InstantPlayFireAnim")?.parse().ok())
                .is_some_and(|v| v != 0.);
            weapons.push(Weapon {
                motions,
                combo,
                offhand,
                throw_anim,
                transitions,
                saber,
                offhand_weapon,
                fire_anim,
                instant_anim,
            });
        }
        let rifle_chain = ["rifle".to_owned()];
        let resolve_tumble = |form: &str, default_repeats: bool| {
            let [base, lower, upper] = layers(c, &rifle_chain, form, false, false);
            let mut row = Row {
                base,
                lower,
                upper,
                repeats: default_repeats,
                ..Row::default()
            };
            if row.exists() {
                if let Some(block) = config.get(form) {
                    tuning(&mut row, block);
                }
            }
            row
        };
        let tumble = TumbleRows {
            flying: [
                resolve_tumble("thrown_flyingfront", false),
                resolve_tumble("thrown_flyingback", false),
                resolve_tumble("thrown_flyingleft", false),
                resolve_tumble("thrown_flyingright", false),
            ],
            flail: resolve_tumble("thrown_flail", true),
            tumble: [
                resolve_tumble("thrown_tumblefront", true),
                resolve_tumble("thrown_tumbleback", true),
            ],
            bounce: [
                resolve_tumble("thrown_bouncefrontsoft", false),
                resolve_tumble("thrown_bouncebacksoft", false),
            ],
            land: [
                resolve_tumble("thrown_landfrontsoft", false),
                resolve_tumble("thrown_landbacksoft", false),
            ],
            getup: [
                resolve_tumble("stand_getupfront", false),
                resolve_tumble("stand_getupback", false),
            ],
            rest: [
                resolve_tumble("thrown_restfrontsoft", false),
                resolve_tumble("thrown_restbacksoft", false),
            ],
        };
        let speed = number(def, "MaxSpeed")
            .or_else(|| prop(def, "MaxSpeed")?.parse().ok())
            .filter(|v| *v > 0.)
            .unwrap_or(1.);
        let directional_jumps = number(def, "UseDirectionalJumps")
            .or_else(|| prop(def, "UseDirectionalJumps")?.parse().ok())
            .is_none_or(|v| v != 0.);
        Ok(Self {
            weapons,
            tumble,
            run_speed: speed,
            directional_jumps,
            clip_names,
        })
    }
    pub fn clip_name(&self, id: usize) -> &str {
        self.clip_names.get(id).map_or("", String::as_str)
    }
    pub(super) fn directional_jumps(&self) -> bool {
        self.directional_jumps
    }
    pub(super) fn aims(&self, weapon: usize) -> bool {
        !self.weapons.get(weapon).unwrap_or(&self.weapons[0]).saber
    }
    pub fn offhand_weapon(&self, weapon: usize) -> bool {
        self.weapons.get(weapon).is_some_and(|w| w.offhand_weapon)
    }
    pub fn fire_anim(&self, weapon: usize) -> u8 {
        self.weapons.get(weapon).map_or(0, |w| w.fire_anim)
    }
    pub fn instant_anim(&self, weapon: usize) -> bool {
        self.weapons.get(weapon).is_some_and(|w| w.instant_anim)
    }
    pub fn action_duration(&self, c: &Character, weapon: usize, crouch: bool, action: Action) -> f64 {
        let probe = State {
            weapon,
            crouch,
            action,
            ..Default::default()
        };
        let row = self.sample(c, &probe, 0.).action;
        row.into_iter()
            .flat_map(|r| [r.base, r.lower, r.upper])
            .flatten()
            .map(|i| c.clips[i].duration())
            .fold(0., f64::max)
    }
    pub(super) fn blend_time(&self, weapon: usize, old: Row, new: Row) -> f64 {
        let transitions = &self
            .weapons
            .get(weapon)
            .unwrap_or(&self.weapons[0])
            .transitions;
        for from in [old.upper, old.base, old.lower].into_iter().flatten() {
            for to in [new.upper, new.base, new.lower].into_iter().flatten() {
                if let Some(time) = transitions.get(&(from, to)) {
                    return *time;
                }
            }
        }
        new.blend_in.min(old.blend_out)
    }
    /// Verify every admitted upper layer, including posture and weapon variants.
    pub fn check_upper_tracks(&self, c: &Character) -> Result<usize, String> {
        let mut total = 0;
        for (weapon, w) in self.weapons.iter().enumerate() {
            for row in w
                .motions
                .iter()
                .chain(w.combo.iter().map(|(row, _)| row))
                .chain(w.offhand.iter())
                .chain(std::iter::once(&w.throw_anim))
            {
                let Some(upper) = row.upper else {
                    continue;
                };
                let lower = row.lower.filter(|&id| id != upper);
                let mask = match lower {
                    Some(_) if row.inherited_gait && c.rig.torso.is_some() => {
                        super::Mask::Upper(&c.rig.upper)
                    }
                    Some(id) => super::Mask::Over(&c.clips[id]),
                    None => super::Mask::All,
                };
                let clip = &c.clips[upper];
                if clip.applied_tracks(mask) == 0 {
                    return Err(format!(
                        "weapon {weapon} upper clip {:08x} has no applicable tracks",
                        clip.crc
                    ));
                }
                total += 1;
            }
        }
        Ok(total)
    }
    pub fn combo_states(&self, weapon: usize) -> usize {
        self.weapons.get(weapon).map_or(0, |w| w.combo.len())
    }
    pub fn sample(&self, c: &Character, s: &State, gait_phase: f64) -> Sample {
        self.sample_after(c, s, gait_phase, false, None, None)
    }
    pub(super) fn sample_after(
        &self,
        c: &Character,
        s: &State,
        gait_phase: f64,
        was_backward: bool,
        latched_jump: Option<Option<Direction>>,
        jump_elapsed: Option<f64>,
    ) -> Sample {
        let w = self.weapons.get(s.weapon).unwrap_or(&self.weapons[0]);
        let speed = s.velocity[0].hypot(s.velocity[2]);
        let dir = s
            .roll
            .unwrap_or_else(|| Direction::from_velocity(s.velocity, s.yaw));
        let half = if s.alert { 2 * MOTIONS.len() * 4 } else { 0 };
        let directed = |m: Motion, direction: Direction| {
            w.motions
                [half + (usize::from(s.crouch) * MOTIONS.len() + m as usize) * 4 + direction as usize]
        };
        let get = |m| directed(m, dir);
        if let Some(id) = s.seat_clip.filter(|&id| id < c.clips.len()) {
            return Sample {
                motion: Row {
                    base: Some(id),
                    ..Row::default()
                },
                motion_time: s.seat_time,
                action: None,
                action_time: 0.,
                rate: 0.,
                travel_yaw: 0.,
                backward: false,
                gait_clock: false,
                combo_action: false,
                tumble_turn: None,
                overlay: None,
            };
        }
        if s.action != Action::Death {
            if let Some(id) = s.choke_clip.filter(|&id| id < c.clips.len()) {
                return Sample {
                    motion: Row {
                        base: Some(id),
                        repeats: true,
                        blend_in: 0.15,
                        blend_out: 0.15,
                        ..Row::default()
                    },
                    motion_time: s.choke_time,
                    action: None,
                    action_time: 0.,
                    rate: 0.,
                    travel_yaw: 0.,
                    backward: false,
                    gait_clock: false,
                    combo_action: false,
                    tumble_turn: None,
                    overlay: None,
                };
            }
        }
        if let Some(t) = s.tumble {
            let side = usize::from(!t.front);
            let pick_thrown = |elapsed: f64, fly_dir: u8| {
                let dir = usize::from(fly_dir.min(3));
                if elapsed <= 0.30 && self.tumble.flying[dir].exists() {
                    self.tumble.flying[dir]
                } else if self.tumble.flail.exists() {
                    self.tumble.flail
                } else if self.tumble.flying[dir].exists() {
                    self.tumble.flying[dir]
                } else {
                    get(Motion::Fall)
                }
            };
            if s.action == Action::Death {
                let row = if self.tumble.rest[side].exists() {
                    Some(self.tumble.rest[side])
                } else if self.tumble.land[side].exists() {
                    Some(self.tumble.land[side])
                } else {
                    None
                };
                if let Some(motion) = row {
                    return Sample {
                        motion,
                        motion_time: t.elapsed,
                        action: None,
                        action_time: 0.,
                        rate: 0.,
                        travel_yaw: 0.,
                        backward: false,
                        gait_clock: false,
                        combo_action: false,
                        tumble_turn: Some(t.heading),
                        overlay: None,
                    };
                }
            } else {
                let chosen = match t.phase {
                    TumblePhase::Thrown => {
                        let motion = pick_thrown(t.elapsed, t.fly_dir);
                        motion.exists().then_some((motion, t.elapsed, 1., false, 0.))
                    }
                    TumblePhase::Tumble => {
                        let motion = if self.tumble.tumble[side].exists() {
                            self.tumble.tumble[side]
                        } else if self.tumble.flail.exists() {
                            self.tumble.flail
                        } else {
                            get(Motion::Fall)
                        };
                        let rate = motion
                            .base
                            .or(motion.lower)
                            .map(|i| c.clips[i].speed * c.clips[i].stride_scale)
                            .filter(|stride| stride.is_finite() && *stride > 1e-4)
                            .map_or(1., |stride| speed / stride);
                        motion.exists().then_some((motion, gait_phase, rate, true, t.heading))
                    }
                    TumblePhase::Landed => {
                        let motion = if t.bounce && self.tumble.bounce[side].exists() {
                            self.tumble.bounce[side]
                        } else if self.tumble.land[side].exists() {
                            self.tumble.land[side]
                        } else {
                            get(Motion::Idle)
                        };
                        motion.exists().then_some((motion, t.elapsed, 0., false, t.heading))
                    }
                    TumblePhase::GetUp => {
                        let motion = if self.tumble.getup[side].exists() {
                            self.tumble.getup[side]
                        } else {
                            get(Motion::Idle)
                        };
                        motion.exists().then_some((motion, t.elapsed, 0., false, t.heading))
                    }
                    TumblePhase::Recover => {
                        let motion = if get(Motion::Fall).exists() {
                            get(Motion::Fall)
                        } else {
                            self.tumble.flail
                        };
                        motion.exists().then_some((motion, t.elapsed, 1., false, 0.))
                    }
                    TumblePhase::RestDead => {
                        let motion = if self.tumble.rest[side].exists() {
                            self.tumble.rest[side]
                        } else {
                            self.tumble.land[side]
                        };
                        motion.exists().then_some((motion, t.elapsed, 0., false, t.heading))
                    }
                    TumblePhase::None => None,
                };
                if let Some((motion, motion_time, rate, gait_clock, heading)) = chosen {
                    return Sample {
                        motion,
                        motion_time,
                        action: None,
                        action_time: 0.,
                        rate,
                        travel_yaw: 0.,
                        backward: false,
                        gait_clock,
                        combo_action: false,
                        tumble_turn: Some(heading),
                        overlay: None,
                    };
                }
            }
        } else if s.air == Air::Thrown && s.action != Action::Death {
            let elapsed = s.phase_time.unwrap_or(s.elapsed);
            let fly_dir = match Direction::from_velocity(s.velocity, s.yaw) {
                Direction::Forward => 0,
                Direction::Backward => 1,
                Direction::Left => 2,
                Direction::Right => 3,
            };
            let motion = if elapsed <= 0.30 && self.tumble.flying[fly_dir].exists() {
                self.tumble.flying[fly_dir]
            } else if self.tumble.flail.exists() {
                self.tumble.flail
            } else {
                get(Motion::Fall)
            };
            if motion.exists() {
                return Sample {
                    motion,
                    motion_time: elapsed,
                    action: None,
                    action_time: 0.,
                    rate: 1.,
                    travel_yaw: 0.,
                    backward: false,
                    gait_clock: false,
                    combo_action: false,
                    tumble_turn: Some(0.),
                    overlay: None,
                };
            }
        }
        let gait = if s.sprint {
            Motion::Sprint
        } else if speed >= self.run_speed * PROVISIONAL_RUN_FRACTION {
            Motion::Run
        } else if speed > 1e-4 {
            Motion::Walk
        } else {
            Motion::Idle
        };
        let mut m = if s.roll.is_some() {
            Motion::Roll
        } else {
            match s.air {
                Air::Jump => Motion::Jump,
                Air::Fall | Air::Thrown => Motion::Fall,
                Air::Jet => Motion::Jet,
                Air::Land if s.hard_landing => Motion::HardLand,
                Air::Land => Motion::Land,
                Air::Ground => gait,
            }
        };
        let mut jump_rollover_time = None;
        let special = if m == Motion::Jump {
            let jump_choice = latched_jump.unwrap_or_else(|| {
                (self.directional_jumps && speed >= 2.0).then_some(dir)
            });
            let jump_row = match jump_choice {
                Some(d) => directed(Motion::Jump, d),
                None => {
                    let neutral = directed(Motion::NeutralJump, Direction::Forward);
                    if neutral.exists() {
                        neutral
                    } else {
                        directed(Motion::Jump, Direction::Forward)
                    }
                }
            };
            if jump_row.exists() {
                let elapsed = jump_elapsed.unwrap_or_else(|| s.phase_time.unwrap_or(s.elapsed));
                let duration = [jump_row.base, jump_row.lower, jump_row.upper]
                    .into_iter()
                    .flatten()
                    .map(|i| c.clips[i].duration())
                    .fold(0., f64::max);
                let fall_row = directed(Motion::Fall, Direction::Forward);
                if duration > 0. && elapsed > duration && fall_row.exists() {
                    m = Motion::Fall;
                    jump_rollover_time = Some(elapsed - duration);
                    Some(fall_row)
                } else {
                    jump_rollover_time = Some(elapsed);
                    Some(jump_row)
                }
            } else {
                None
            }
        } else {
            (m != gait).then(|| get(m)).filter(|r| r.exists())
        };
        let gait_clock = special.is_none();
        let m = if gait_clock { gait } else { m };
        let steering = matches!(m, Motion::Walk | Motion::Run | Motion::Sprint);
        let angle = wrapped(s.velocity[0].atan2(s.velocity[2]) - s.yaw);
        // When the bank lacks lateral cycles, the forward/back choice has a
        // wider sector for the gait already playing.
        let limit = if was_backward { 72_f64 } else { 108_f64 }.to_radians();
        let backward = steering && angle.abs() > limit;
        let axis = if directed(m, Direction::Left).exists()
            && directed(m, Direction::Right).exists()
            && m != Motion::Sprint
        {
            dir
        } else if backward {
            Direction::Backward
        } else {
            Direction::Forward
        };
        let mut motion = special.unwrap_or_else(|| directed(m, axis));
        let mut reverse = false;
        if steering && !motion.exists() {
            motion = directed(m, Direction::Forward);
            reverse = backward;
        }
        if !motion.exists() {
            motion = get(Motion::Idle);
        }
        let mut motion_time = if gait_clock {
            gait_phase
        } else if let Some(t) = jump_rollover_time {
            t
        } else {
            s.phase_time.unwrap_or(s.elapsed)
        };
        if m == Motion::Roll {
            if let (Some(window), Some(id)) = (
                s.roll_seconds.filter(|t| t.is_finite() && *t > 0.),
                motion.base.or(motion.lower).or(motion.upper),
            ) {
                motion_time = (motion_time / window).clamp(0., 1.) * c.clips[id].duration();
            }
        }
        let combo = s
            .combo
            .and_then(|i| w.combo.get(i))
            .map(|&(mut row, reactions)| {
                if let Some(id) = s.contact_clip.filter(|&i| i < c.clips.len()) {
                    row.base = Some(id);
                    row.upper = None;
                    row.lower = None;
                } else if let Some(id) = s
                    .reaction
                    .checked_sub(1)
                    .and_then(|i| reactions.get(i as usize))
                    .copied()
                    .flatten()
                {
                    row.base = Some(id);
                    row.full_body = false;
                }
                row
            })
            .filter(|r| r.exists());
        let offhand = |slot: usize| {
            if w.saber {
                w.offhand[slot]
            } else if slot == 1 {
                get(Motion::FireSecondary2)
            } else {
                get(Motion::FireSecondary)
            }
        };
        let mut combo_action = false;
        let hit = || {
            s.impact
                .map_or_else(|| get(Motion::NeutralHit), |d| directed(Motion::Hit, d))
        };
        let action = match s.action {
            Action::Death => Some(s.impact.map_or_else(
                || get(Motion::NeutralDeath),
                |d| directed(Motion::Death, d.opposite()),
            )),
            // A combo row outranks a hit: the game blends the hit over the swing.
            Action::Hit if combo.is_none() => Some(hit()),
            _ if m == Motion::Roll => None,
            Action::FireSecondary if offhand(0).exists() => Some(offhand(0)),
            Action::FireSecondary2 if offhand(1).exists() => Some(offhand(1)),
            _ if combo.is_some() => {
                combo_action = true;
                combo
            }
            Action::Fire => Some(get(Motion::Fire)),
            Action::FireSecondary => Some(offhand(0)),
            Action::FireSecondary2 => Some(offhand(1)),
            Action::Throw => Some(w.throw_anim),
            Action::Reload => Some(get(Motion::Reload)),
            _ => None,
        }
        .filter(|r| r.exists());
        combo_action &= action.is_some();
        let overlay = (combo_action && s.action == Action::Hit)
            .then(|| (hit(), s.elapsed, HIT_OVERLAY_WEIGHT * (1. - HIT_OVERLAY_FADE * s.elapsed)))
            .filter(|(row, _, weight)| row.exists() && *weight > 0.);
        let stride = motion.lower.or(motion.base).map(|i| &c.clips[i]);
        let rate = stride
            .map(|clip| clip.speed * clip.stride_scale)
            .filter(|stride| steering && stride.is_finite() && *stride > 1e-4)
            .map_or(1., |stride| speed / stride)
            * if reverse { -1. } else { 1. };
        let travel_yaw = if steering {
            let authored = stride.filter(|k| k.speed > 1e-4).map_or_else(
                || match axis {
                    Direction::Backward if !reverse => std::f64::consts::PI,
                    Direction::Left => std::f64::consts::FRAC_PI_2,
                    Direction::Right => -std::f64::consts::FRAC_PI_2,
                    _ => 0.,
                },
                |k| k.travel[0].atan2(k.travel[1]),
            );
            wrapped(angle - authored - if reverse { std::f64::consts::PI } else { 0. })
        } else {
            0.
        };
        Sample {
            motion,
            action,
            motion_time,
            rate,
            travel_yaw,
            backward,
            gait_clock,
            combo_action,
            tumble_turn: None,
            action_time: if combo_action { s.anim_time } else { s.elapsed },
            overlay,
        }
    }
}
#[cfg(test)]
pub(super) mod blend_tests {
    use super::*;
    #[test]
    fn a_named_cut_overrides_positive_blend_out_only_for_its_target() {
        let block = bfe_content::json::parse(
            r#"{"children":[{"key":"BlendTimeTo","values":["settle",0]}]}"#,
        )
        .unwrap();
        let mut transitions = HashMap::new();
        blend_targets(
            &block,
            3,
            |s| (s == "settle").then_some(8),
            &mut transitions,
        );
        let tables = Tables {
            weapons: vec![Weapon {
                motions: vec![],
                combo: vec![],
                offhand: [Row::default(); 4],
                throw_anim: Row::default(),
                transitions,
                saber: false,
                offhand_weapon: false,
                fire_anim: 0,
                instant_anim: false,
            }],
            tumble: TumbleRows::default(),
            run_speed: 1.,
            directional_jumps: true,
            clip_names: vec![],
        };
        let old = Row {
            upper: Some(3),
            blend_out: 0.4,
            ..Row::default()
        };
        for (id, expected) in [(8, 0.), (9, 0.4)] {
            let new = Row {
                base: Some(id),
                blend_in: 0.6,
                ..Row::default()
            };
            assert_eq!(tables.blend_time(0, old, new), expected);
        }
    }

    /// Invented melee rig: a saber stance with an idle, a swing, a hit and casts.
    pub(in crate::animation2) fn melee_fixture() -> (Character, Tables) {
        let (character, tables, _, _) = melee_fixture_parts();
        (character, tables)
    }

    /// The same rig with the class definition and animation sets it was built from.
    fn melee_fixture_parts() -> (Character, Tables, bfe_content::Value, bfe_content::Value) {
        use super::super::Archive;
        use bfe_content::{actor::build_actor, json};
        use serde_json::json as j;
        let mesh = json::parse(
            r#"{"bones":[
                {"name":"root","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"torso","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let bone = zero_hash::crc_hex("torso");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        for (name, y) in [
            ("cedar_sabre_stand_idle", 0.0f32),
            ("cedar_sabre_swing_full", 1.0),
            ("cedar_sabre_stand_hitfront", 5.0),
            ("cedar_sabre_stand_cast_full", 2.0),
            ("cedar_sabre_stand_toss_full", 2.5),
            ("cedar_tool_stand_idle", 0.0),
            ("cedar_tool_stand_shoot_secondary", 3.0),
            ("cedar_tool_stand_shoot_secondary2", 4.0),
        ] {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": 4,
                "tracks": [bone]
            }));
            for f in 0..4 {
                samples.extend([0f32, 0., 0., 1., 0., y + f as f32 * 0.25, 0.]);
            }
        }
        let meta = json::parse(
            &j!({
                "fps": 10,
                "banks": {
                    "cedar": {"clips": clips},
                    "cedar_sabre": {"clips": []}
                }
            })
            .to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(
            &j!({
                "weapons": [
                    {
                        "AnimationBank": "sabre",
                        "comboBindings": [{
                            "combo": "abcd1234",
                            "bank": "cedar_sabre",
                            "mode": "melee",
                            "stateAnimations": {
                                "IDLE": [],
                                "STRIKE": [{"key": "Animation", "name": "swing", "playback": "once"}]
                            },
                            "clipTiming": {
                                "swing": {"bank": "cedar", "crc": zero_hash::crc_hex("cedar_sabre_swing_full"), "frames": 4, "fps": 10},
                                "stand_cast": {"bank": "cedar", "crc": zero_hash::crc_hex("cedar_sabre_stand_cast_full"), "frames": 4, "fps": 10},
                                "stand_toss": {"bank": "cedar", "crc": zero_hash::crc_hex("cedar_sabre_stand_toss_full"), "frames": 4, "fps": 10}
                            }
                        }]
                    },
                    {
                        "AnimationBank": "pistol",
                        "OffhandWeapon": "0"
                    },
                    {
                        "OffhandWeapon": "1",
                        "FireAnim": "1",
                        "InstantPlayFireAnim": "1"
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let machines = json::parse(
            &j!({
                "abcd1234": {
                    "states": [
                        {"values": ["IDLE"], "children": []},
                        {"values": ["STRIKE"], "children": [{"key": "Attack"}]}
                    ],
                    "animations": [],
                    "offhandAnimations": [
                        {
                            "values": ["stand_cast", "FIRE"],
                            "children": [
                                {"key": "Loop", "values": ["FinalFrame"]},
                                {"key": "AimType", "values": ["Torso"]},
                                {"key": "BlendInTime", "values": [0.15]},
                                {"key": "BlendOutTime", "values": [0.2]}
                            ]
                        },
                        {"values": ["stand_cast", "FIRE2"]}
                    ],
                    "throwAnimations": [
                        {"values": ["stand_toss"]}
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        let sets = json::parse(
            &j!({
                "set0": [
                    {
                        "values": ["stand_shoot_secondary2_upper"],
                        "children": [
                            {"key": zero_hash::fnv1a_hex("AimType"), "values": ["Torso"]},
                            {"key": zero_hash::fnv1a_hex("BlendInTime"), "values": [0.15]},
                            {"key": zero_hash::fnv1a_hex("BlendOutTime"), "values": [0.15]}
                        ]
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let character = Character::bind("caster", &def, &["cedar".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&character, &def, &sets, &machines).unwrap();
        (character, tables, def, sets)
    }

    #[test]
    fn combo_and_gun_offhand_animations_resolve_tuning_and_slots() {
        use bfe_content::json;
        use serde_json::json as j;
        let (character, tables, def, sets) = melee_fixture_parts();
        // Idle combo state (no clip) lets offhand power animation play.
        let s = State {
            weapon: 0,
            combo: Some(0),
            action: Action::FireSecondary,
            elapsed: 0.2,
            ..Default::default()
        };
        let sample = tables.sample(&character, &s, 0.);
        let row = sample.action.expect("combo offhand row");
        assert_eq!(row.base, character.named_clip("cedar_sabre_stand_cast_full"));
        assert!(!row.repeats && row.aims);
        assert_eq!((row.blend_in, row.blend_out), (0.15, 0.2));
        assert!(!sample.combo_action);
        assert_eq!(sample.action_time, 0.2);
        // Active offhand power animation takes precedence over an active melee combo state.
        let swinging = State { combo: Some(1), anim_time: 0.1, ..s };
        let swing_sample = tables.sample(&character, &swinging, 0.);
        assert!(!swing_sample.combo_action);
        assert_eq!(swing_sample.action.unwrap().base, character.named_clip("cedar_sabre_stand_cast_full"));
        assert_eq!(swing_sample.action_time, 0.2);
        // Without an active offhand action, the melee combo state plays as usual.
        let pure_swing = State { action: Action::None, ..swinging };
        let pure_sample = tables.sample(&character, &pure_swing, 0.);
        assert!(pure_sample.combo_action);
        assert_eq!(pure_sample.action.unwrap().base, character.named_clip("cedar_sabre_swing_full"));
        assert_eq!(pure_sample.action_time, 0.1);
        // An offhand weapon slot without its own stance inherits the saber offhand clip.
        let alt_slot = State { weapon: 2, combo: Some(0), action: Action::FireSecondary2, elapsed: 0.15, ..Default::default() };
        let alt_sample = tables.sample(&character, &alt_slot, 0.);
        assert_eq!(alt_sample.action.unwrap().base, character.named_clip("cedar_sabre_stand_cast_full"));
        // A melee weapon whose combo machine defines no offhand clips falls back to its combo state.
        let no_offhand_machines = json::parse(
            &j!({
                "abcd1234": {
                    "states": [
                        {"values": ["IDLE"], "children": []},
                        {"values": ["STRIKE"], "children": [{"key": "Attack"}]}
                    ],
                    "animations": [],
                    "offhandAnimations": []
                }
            })
            .to_string(),
        )
        .unwrap();
        let no_offhand_tables = Tables::new(&character, &def, &sets, &no_offhand_machines).unwrap();
        let fallback_sample = no_offhand_tables.sample(&character, &swinging, 0.);
        assert!(fallback_sample.combo_action);
        assert_eq!(fallback_sample.action.unwrap().base, character.named_clip("cedar_sabre_swing_full"));
        // Gun hero secondary2 resolves along pistol -> tool with crouch fallback and upper tuning.
        let gun_off = State { weapon: 1, crouch: true, action: Action::FireSecondary2, elapsed: 0.1, ..Default::default() };
        let gun_sample = tables.sample(&character, &gun_off, 0.);
        let gun_row = gun_sample.action.expect("gun secondary2 row");
        assert_eq!(gun_row.base, character.named_clip("cedar_tool_stand_shoot_secondary2"));
        assert!(gun_row.aims && gun_row.blend_in == 0.15 && gun_row.blend_out == 0.15);
        assert!(tables.offhand_weapon(2) && tables.fire_anim(2) == 1 && tables.instant_anim(2));
        assert!(!tables.offhand_weapon(1));
        // Throw action resolves the combo machine's throwAnimations clip.
        let toss = State {
            weapon: 0,
            combo: Some(0),
            action: Action::Throw,
            elapsed: 0.1,
            ..Default::default()
        };
        let toss_sample = tables.sample(&character, &toss, 0.);
        assert_eq!(
            toss_sample.action.expect("combo throw row").base,
            character.named_clip("cedar_sabre_stand_toss_full")
        );
        assert!(!toss_sample.combo_action);
    }

    #[test]
    fn blend_time_uses_minimum_of_outgoing_and_incoming_when_no_explicit_override() {
        let tables = Tables {
            weapons: vec![Weapon {
                motions: vec![],
                combo: vec![],
                offhand: Default::default(),
                throw_anim: Row::default(),
                transitions: HashMap::new(),
                saber: false,
                offhand_weapon: false,
                fire_anim: 0,
                instant_anim: false,
            }],
            run_speed: 1.,
            directional_jumps: true,
            tumble: Default::default(),
            clip_names: vec![],
        };
        let jump = Row {
            base: Some(1),
            blend_in: 0.01,
            blend_out: 1.0,
            ..Row::default()
        };
        let fall = Row {
            base: Some(2),
            blend_in: 1.0,
            blend_out: 0.01,
            ..Row::default()
        };
        let land = Row {
            base: Some(3),
            blend_in: 0.01,
            blend_out: 0.15,
            ..Row::default()
        };
        assert_eq!(tables.blend_time(0, jump, fall), 1.0);
        assert_eq!(tables.blend_time(0, jump, land), 0.01);
        assert_eq!(tables.blend_time(0, fall, land), 0.01);
    }

    #[test]
    fn thrown_flying_switches_to_flail_after_window_and_tumble_rate_scales_unclamped() {
        use super::super::Archive;
        use bfe_content::{actor::build_actor, json};
        use serde_json::json as j;
        let mesh = json::parse(
            r#"{"bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"torso","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let bone = zero_hash::crc_hex("DummyRoot");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        for (name, dz) in [
            ("elm_rifle_stand_idle", 0.0f32),
            ("elm_rifle_thrown_flyingfront", 0.0),
            ("elm_rifle_thrown_flyingback", 0.0),
            ("elm_rifle_thrown_flyingleft", 0.0),
            ("elm_rifle_thrown_flyingright", 0.0),
            ("elm_rifle_thrown_flail", 0.0),
            ("elm_rifle_thrown_tumblefront", 0.2),
            ("elm_rifle_thrown_restfrontsoft", 0.0),
            ("elm_rifle_thrown_restbacksoft", 0.0),
        ] {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": if dz > 0.0 { 1 } else { 0 },
                "offset": samples.len(),
                "frames": 2,
                "tracks": [bone]
            }));
            // Stride speed for 2 frames at 10 fps (0.1 s): dz / 0.1 = 2.0 m/s for tumblefront.
            samples.extend([0f32, 0., 0., 1., 0., 1., 0.]);
            samples.extend([0f32, 0., 0., 1., 0., 1., dz]);
        }
        let meta = json::parse(&j!({"fps": 10, "banks": {"elm": {"clips": clips}}}).to_string())
            .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(&j!({"MaxSpeed": 5.0, "weapons": [{"AnimationBank": "rifle"}]}).to_string()).unwrap();
        let character = Character::bind("subject", &def, &["elm".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&character, &def, &bfe_content::Value::Null, &bfe_content::Value::Null).unwrap();
        let expected_flying = [
            character.named_clip("elm_rifle_thrown_flyingfront"),
            character.named_clip("elm_rifle_thrown_flyingback"),
            character.named_clip("elm_rifle_thrown_flyingleft"),
            character.named_clip("elm_rifle_thrown_flyingright"),
        ];
        let flail = character.named_clip("elm_rifle_thrown_flail");
        // Within 0.30 s, each of the 4 fly_dir values chooses its distinct thrown_flying clip.
        for (dir, want) in expected_flying.iter().enumerate() {
            let s = State {
                tumble: Some(TumblePose {
                    phase: TumblePhase::Thrown,
                    fly_dir: dir as u8,
                    elapsed: 0.10,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert_eq!(tables.sample(&character, &s, 0.).motion.base, *want, "fly_dir={dir}");
        }
        // Past 0.30 s, switches to thrown_flail.
        let s_flail = State {
            tumble: Some(TumblePose {
                phase: TumblePhase::Thrown,
                fly_dir: 1,
                elapsed: 0.35,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(tables.sample(&character, &s_flail, 0.).motion.base, flail);
        // Death while Thrown selects thrown_rest{front,back}soft (not flying/flail).
        for (front, name) in [(true, "elm_rifle_thrown_restfrontsoft"), (false, "elm_rifle_thrown_restbacksoft")] {
            let s_dead = State {
                action: Action::Death,
                tumble: Some(TumblePose {
                    phase: TumblePhase::Thrown,
                    front,
                    fly_dir: 1,
                    elapsed: 0.10,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert_eq!(tables.sample(&character, &s_dead, 0.).motion.base, character.named_clip(name));
        }
        // Ground tumble playback rate scales proportionally with speed with no 0.5..2.0 clamp.
        let slow = State {
            velocity: [0.4, 0., 0.],
            tumble: Some(TumblePose { phase: TumblePhase::Tumble, front: true, ..Default::default() }),
            ..Default::default()
        };
        let fast = State {
            velocity: [8.0, 0., 0.],
            tumble: Some(TumblePose { phase: TumblePhase::Tumble, front: true, ..Default::default() }),
            ..Default::default()
        };
        let slow_rate = tables.sample(&character, &slow, 0.).rate;
        let fast_rate = tables.sample(&character, &fast, 0.).rate;
        assert!(slow_rate < 0.3 && slow_rate > 0.1, "unclamped slow tumble rate={slow_rate}");
        assert!(fast_rate > 3.5, "unclamped fast tumble rate={fast_rate}");
    }

    #[test]
    fn a_hit_is_blended_over_a_combo_swing_and_fades_out_in_an_eighth_of_a_second() {
        let (character, tables) = melee_fixture();
        let hit = character.named_clip("cedar_sabre_stand_hitfront");
        let swing = character.named_clip("cedar_sabre_swing_full");
        assert!(hit.is_some() && swing.is_some());
        // Outside a combo row a hit plays as before.
        let idle = State { weapon: 0, combo: Some(0), action: Action::Hit, elapsed: 0.05, ..Default::default() };
        let flinch = tables.sample(&character, &idle, 0.);
        assert_eq!(flinch.action.unwrap().base, hit);
        assert!(!flinch.combo_action && flinch.overlay.is_none());
        // In a swing the swing keeps playing and the hit rides over it.
        let struck = State { combo: Some(1), anim_time: 0.1, ..idle };
        let over = tables.sample(&character, &struck, 0.);
        assert!(over.combo_action);
        assert_eq!((over.action.unwrap().base, over.action_time), (swing, 0.1));
        let (row, time, weight) = over.overlay.expect("hit overlay");
        assert_eq!((row.base, time), (hit, 0.05));
        assert!((weight - 0.3).abs() < 1e-12);
        let fresh = tables.sample(&character, &State { elapsed: 0., ..struck }, 0.);
        assert_eq!(fresh.overlay.map(|o| o.2), Some(0.5));
        let late = tables.sample(&character, &State { elapsed: 0.125, ..struck }, 0.);
        assert!(late.combo_action && late.overlay.is_none());
    }
}
