//! A borrowed presentation sample from a Battlefront II battle, shared by local and online play.

/// Previous and current fixed-tick observations of the same subject.
#[derive(serde::Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub previous: f32,
    pub current: f32,
}

impl Sample {
    pub fn at(self, fraction: f32) -> f32 {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0., 1.)
        } else {
            0.
        };
        self.previous + (self.current - self.previous) * fraction
    }

    /// Interpolate a heading in radians across its wraparound.
    pub fn angle_at(self, fraction: f32) -> f32 {
        use std::f32::consts::{PI, TAU};
        let delta = (self.current - self.previous + PI).rem_euclid(TAU) - PI;
        Sample {
            previous: 0.,
            current: delta,
        }
        .at(fraction)
            + self.previous
    }

    pub const fn steady(value: f32) -> Self {
        Self {
            previous: value,
            current: value,
        }
    }
}

#[derive(serde::Deserialize, Clone, Copy, Debug, Default)]
pub struct Gauge {
    pub value: Sample,
    pub maximum: f32,
}

impl Gauge {
    pub fn fill(self, fraction: f32) -> f32 {
        if self.maximum > 0. {
            (self.value.at(fraction) / self.maximum).clamp(0., 1.)
        } else {
            0.
        }
    }
}

/// Coordinates in the map image or the drawing viewport, with (0,0) at top left.
pub type Point = [Sample; 2];

#[derive(serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subject {
    pub id: u32,
    pub generation: u32,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Caption<'a> {
    /// A localization path or its eight-digit hexadecimal string ID.
    Key(&'a str),
    /// A player name or already localized authority text.
    Literal(&'a str),
}

#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    HeroAssault,
    SpaceAssault,
    Other,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Winner(u8),
    Draw,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Spawn,
    Playing,
    Ended(Outcome),
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Trooper,
    RosterHero,
    AwardedHero,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    Pilot,
    Gunner,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    None,
    Acquiring,
    Locked,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Weapon,
    Saber,
    Power,
}
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    All,
    Team(u8),
    Player(u32),
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Team<'a> {
    pub id: u8,
    pub name: Caption<'a>,
    pub icon: &'a str,
    pub score: i32,
    pub target: Option<u32>,
    pub reinforcements: Option<i32>,
    pub show_score: bool,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Slot<'a> {
    pub kind: SlotKind,
    pub name: Caption<'a>,
    pub icon: &'a str,
    /// Last change of equipment in this slot, in battle ticks.
    #[serde(default)]
    pub changed_at: Option<u32>,
    /// None for an unlimited weapon, saber or power.
    pub rounds: Option<u32>,
    pub reserve: Option<u32>,
    pub charge: Option<Gauge>,
    pub ready_at: Option<u32>,
    #[serde(default)]
    pub clip: Option<Gauge>,
    #[serde(default)]
    pub heat: Option<Gauge>,
    #[serde(default)]
    pub overheated: bool,
    #[serde(default)]
    pub refire: Option<Gauge>,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Body<'a> {
    pub subject: Subject,
    pub role: Role,
    pub health: Gauge,
    pub stamina: Gauge,
    pub blocking: bool,
    pub map: Point,
    pub yaw: Sample,
    /// Currently selected primary and secondary equipment.
    pub slots: [Option<Slot<'a>>; 2],
    pub reticle: &'a str,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
pub struct MapPose {
    pub map: Point,
    pub yaw: Sample,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Vehicle<'a> {
    pub subject: Subject,
    pub seat: Seat,
    /// Position and heading of the occupied vehicle on the map.
    #[serde(default)]
    pub map_pose: Option<MapPose>,
    pub hull: Gauge,
    pub speed: Gauge,
    pub boost: Option<Gauge>,
    pub slots: [Option<Slot<'a>>; 2],
    /// Localized action text. The pilot's current key is supplied by the client.
    pub landing: Option<Caption<'a>>,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Target<'a> {
    pub subject: Subject,
    pub team: u8,
    pub name: Caption<'a>,
    pub health: Option<Gauge>,
    pub lock: Lock,
    pub progress: Gauge,
    /// Projected left, top, right, bottom in viewport fractions. None behind the camera.
    pub bracket: Option<[Sample; 4]>,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Contact<'a> {
    pub subject: Subject,
    pub team: u8,
    pub map: Point,
    pub icon: &'a str,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct System<'a> {
    pub subject: Subject,
    pub team: u8,
    pub name: Caption<'a>,
    /// The picture of its mark in the Space Assault group.
    pub icon: &'a str,
    pub hull: Gauge,
    pub shield: Option<Gauge>,
    pub destroyed: bool,
    /// Where the mission puts its mark on its team's ship picture (fractions across and down).
    #[serde(default)]
    pub hud: [f32; 2],
    /// Whether the mission shows its mark (a linked system's other parts show none).
    #[serde(default)]
    pub marker: bool,
    /// Its team's ship picture and the shield picture drawn over it.
    #[serde(default)]
    pub ship: &'a str,
    #[serde(default)]
    pub shield_art: &'a str,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Objective<'a> {
    pub id: u32,
    pub audience: Audience,
    pub text: Caption<'a>,
    pub complete: bool,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Timer<'a> {
    pub name: Caption<'a>,
    pub audience: Audience,
    pub due: u32,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Message<'a> {
    Notice(Caption<'a>),
    Kill {
        killer: &'a str,
        victim: &'a str,
        weapon: &'a str,
        /// Display name supplied by the weapon definition when the language lacks its key.
        #[serde(default)]
        weapon_name: Option<&'a str>,
    },
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Event<'a> {
    pub sequence: u32,
    pub until_tick: u32,
    pub audience: Audience,
    pub message: Message<'a>,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct ScoreRow<'a> {
    pub player: u32,
    pub team: u8,
    pub name: &'a str,
    pub score: i32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub ping: Option<u16>,
}

#[derive(serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AimRelation {
    #[default]
    None,
    Neutral,
    Friendly,
    Enemy,
}

#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitMarker {
    pub tick: u32,
    pub critical: bool,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Aim<'a> {
    /// Projected reticle position in viewport fractions `(0..1, 0..1)` with `(0, 0)` at top-left.
    pub position: Option<[f32; 2]>,
    pub visible: bool,
    pub relation: AimRelation,
    pub hit: Option<HitMarker>,
    pub scope: Option<&'a str>,
}

impl Default for Aim<'_> {
    fn default() -> Self {
        Self {
            position: None,
            visible: true,
            relation: AimRelation::None,
            hit: None,
            scope: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BattleView<'a> {
    pub tick: u32,
    pub phase: Phase,
    pub mode: Mode,
    pub player: u32,
    pub team: u8,
    pub map_name: Caption<'a>,
    pub map_image: &'a str,
    pub teams: [Team<'a>; 2],
    pub body: Option<Body<'a>>,
    pub vehicle: Option<Vehicle<'a>>,
    pub aim: Aim<'a>,
    pub target: Option<Target<'a>>,
    pub contacts: &'a [Contact<'a>],
    pub systems: &'a [System<'a>],
    pub objectives: &'a [Objective<'a>],
    pub timers: &'a [Timer<'a>],
    pub events: &'a [Event<'a>],
    pub scores: &'a [ScoreRow<'a>],
    /// What the player controls is outside the battlefield: his leaving countdown runs.
    pub leaving: bool,
}

impl Audience {
    pub fn includes(self, player: u32, team: u8) -> bool {
        match self {
            Self::All => true,
            Self::Team(id) => team == id,
            Self::Player(id) => player == id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_between_ticks_without_changing_the_observation() {
        let energy = Gauge {
            value: Sample {
                previous: -3.,
                current: 9.,
            },
            maximum: 12.,
        };
        assert_eq!(energy.fill(0.), 0.);
        assert_eq!(energy.fill(0.5), 0.25);
        assert_eq!(energy.fill(4.), 0.75);
        assert_eq!(energy.value.current, 9.);
        assert!(!Audience::Player(15).includes(14, 15));
        assert!(Audience::Team(15).includes(14, 15));
    }
}
