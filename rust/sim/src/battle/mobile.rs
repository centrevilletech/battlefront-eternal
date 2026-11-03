//! Command vehicles as command posts (src/mobile-posts.js): a vehicle of the
//! command classes (ODF base commandwalker: the AT-AT and the AT-TE;
//! commandhover: the MTT and the hovernaut) is a post while it lives, one
//! post a vehicle pad for the battle, named by the pad (transport-<slot>).
//! Its owner is its vehicle's team (an occupant's makes it his, and it stays
//! his); it is where its vehicle is; its ValueBleed counts for its owner;
//! it never changes hands by capture; when its vehicle dies it is lost.

use super::world::BattleWorld;

/// The class defaults for the AI's attack and defence weights.
pub const DEFAULT_VALUES: [f64; 6] = [100.0; 6];

/// A command class's post data as ai-mission.json `commandClasses` gives
/// it (the original game's class parser).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassRecord {
    pub atk: Option<[f64; 6]>,
    pub def: Option<[f64; 6]>,
    pub value_bleed: Option<f64>,
    /// Spawn points [x, y, z, yawDeg] (missing numbers 0).
    pub spawn_points: Vec<[f64; 4]>,
}

/// What a post takes from its hull's class (classData).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassData {
    pub atk: [f64; 6],
    pub def: [f64; 6],
    pub value_bleed: f64,
    pub points: Vec<[f64; 4]>,
}

/// classData(def, record) (mobile-posts.js:52): the mission's record, else
/// what the vehicle class says (its ValueBleed parsed as an integer, NaN
/// when it is none).
pub fn class_data(value_bleed_setting: f64, record: Option<&ClassRecord>) -> ClassData {
    if let Some(r) = record {
        return ClassData {
            atk: r.atk.unwrap_or(DEFAULT_VALUES),
            def: r.def.unwrap_or(DEFAULT_VALUES),
            value_bleed: r.value_bleed.unwrap_or(100.0),
            points: r.spawn_points.clone(),
        };
    }
    let bleed = value_bleed_setting;
    ClassData { atk: DEFAULT_VALUES, def: DEFAULT_VALUES, value_bleed: if bleed.is_finite() { bleed } else { 100.0 }, points: Vec::new() }
}

/// A hull on a pad as a command post reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hull {
    /// The vehicle (its place in the battle's list of hulls).
    pub id: u32,
    pub alive: bool,
    /// Of a command class (isCommandClass(v.def)).
    pub command: bool,
    /// Its registry entry's generation (aiTarget?.generation ?? 0).
    pub generation: f64,
    /// v.team, v.controllingTeam (|0 applied).
    pub team: f64,
    pub controlling_team: f64,
    /// Someone holds a seat (a lease not released).
    pub seated: bool,
    pub yaw: f64,
    pub name: String,
    /// v.label || v.name.
    pub label: String,
    /// v.size ? v.size.length() / 2 : v.radius ?? 0.
    pub radius: f64,
    /// v.speed ?? 0.
    pub speed: f64,
    /// parseInt(def.settings.ValueBleed) (NaN when none).
    pub value_bleed_setting: f64,
    /// ai-mission.json commandClasses[v.name].
    pub record: Option<ClassRecord>,
}

/// A pad's hulls: its variants' (in the Map's order) and its own vehicle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pad {
    pub variants: Vec<Hull>,
    pub vehicle: Option<Hull>,
}

/// What a post said of itself at the last events() (MobilePosts.state).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reported {
    pub active: bool,
    pub team: f64,
    pub vehicle: Option<u32>,
    pub life: Option<f64>,
}

/// The AI's view of the post (post.ai): its class's weights, ValueBleed and
/// spawn points.
#[derive(Clone, Debug, PartialEq)]
pub struct MobileAi {
    pub atk: [f64; 6],
    pub def: [f64; 6],
    pub value_bleed: f64,
    pub points: Vec<[f64; 4]>,
    pub class_spawn_points: f64,
}

impl Default for MobileAi {
    fn default() -> Self {
        MobileAi { atk: DEFAULT_VALUES, def: DEFAULT_VALUES, value_bleed: 100.0, points: Vec::new(), class_spawn_points: 0.0 }
    }
}

/// The post's desc: { name, displayName, valueBleed, mobile: true }.
#[derive(Clone, Debug, PartialEq)]
pub struct MobileDesc {
    pub name: String,
    pub display_name: String,
    pub value_bleed: f64,
}

impl Default for MobileDesc {
    fn default() -> Self {
        MobileDesc { name: String::new(), display_name: String::new(), value_bleed: 100.0 }
    }
}

/// One vehicle pad's post (MobilePost).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobilePost {
    /// The pad (its place in the vehicles' slots).
    pub slot: usize,
    pub label: String,
    pub team: f64,
    pub active: bool,
    /// The hull of its current or last life.
    pub vehicle: Option<u32>,
    /// That hull's registry generation.
    pub life: Option<f64>,
    pub yaw: f64,
    pub radius: f64,
    pub speed: f64,
    pub last_position: [f64; 3],
    pub desc: MobileDesc,
    pub ai: MobileAi,
    pub reported: Reported,
}

impl MobilePost {
    /// hull() (mobile-posts.js:132): the pad's live command vehicle.
    fn hull(pad: &Pad) -> Option<&Hull> {
        let mut found = None;
        for v in &pad.variants {
            if v.alive && v.command {
                found = Some(v);
            }
        }
        if found.is_none() {
            if let Some(v) = pad.vehicle.as_ref().filter(|v| v.alive && v.command) {
                found = Some(v);
            }
        }
        found
    }

    /// refresh() (mobile-posts.js:139).
    pub fn refresh(&mut self, world: &mut dyn BattleWorld) {
        let pad = world.pad(self.slot);
        let Some(v) = Self::hull(&pad) else {
            if let Some(old) = self.vehicle {
                self.last_position = world.vehicle_position(old);
            }
            self.active = false;
            return;
        };
        let life = v.generation;
        if Some(v.id) != self.vehicle || Some(life) != self.life {
            // A new hull on the pad: its class's post data, and its team.
            self.vehicle = Some(v.id);
            self.life = Some(life);
            self.team = v.team;
            let data = class_data(v.value_bleed_setting, v.record.as_ref());
            self.ai.atk = data.atk;
            self.ai.def = data.def;
            self.ai.value_bleed = data.value_bleed;
            self.ai.class_spawn_points = data.points.len() as f64;
            self.ai.points = data.points;
            self.label = v.label.clone();
            self.desc = MobileDesc { name: v.name.clone(), display_name: self.label.clone(), value_bleed: data.value_bleed };
            self.radius = v.radius;
            self.speed = v.speed;
        }
        // Whoever takes a seat makes it his team's; it stays his after.
        if v.seated {
            self.team = v.controlling_team;
        }
        self.yaw = v.yaw;
        self.active = true;
    }

    fn state(&self) -> Reported {
        Reported { active: self.active, team: self.team, vehicle: self.vehicle, life: self.life }
    }
}

/// A command vehicle's post lost (its vehicle died, or another took the
/// pad) or changed hands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MobileEvent {
    Lost { post: usize, vehicle: Option<u32>, from: f64 },
    Owner { post: usize, from: f64, to: f64 },
}

/// Every pad whose classes include a command vehicle, in pad order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobilePosts {
    pub posts: Vec<MobilePost>,
}

impl MobilePosts {
    /// refresh() (mobile-posts.js:183).
    pub fn refresh(&mut self, world: &mut dyn BattleWorld) {
        for post in &mut self.posts {
            post.refresh(world);
        }
    }

    /// events() (mobile-posts.js:191).
    pub fn events(&mut self, world: &mut dyn BattleWorld) -> Vec<MobileEvent> {
        let mut out = Vec::new();
        for (i, post) in self.posts.iter_mut().enumerate() {
            let was = post.reported;
            post.refresh(world);
            let replaced = post.vehicle != was.vehicle || post.life != was.life;
            if was.active && (!post.active || replaced) {
                out.push(MobileEvent::Lost { post: i, vehicle: was.vehicle, from: was.team });
            } else if was.active && post.active && was.team != post.team {
                out.push(MobileEvent::Owner { post: i, from: was.team, to: post.team });
            }
            post.reported = post.state();
        }
        out
    }
}
