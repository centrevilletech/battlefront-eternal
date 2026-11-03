//! Classic difficulty (difficulty.js) and its Enhanced flavor
//! (enhanced/difficulty.js): the tier's skills, the four aim curves of
//! each skill in float32 one operation at a time, the mission setters,
//! and the scheduler numbers each flavor runs.

use super::world::Draws;

pub const EASY: f64 = 1.0;
pub const MEDIUM: f64 = 2.0;
pub const HARD: f64 = 3.0;
pub const SKILL_MIN: f64 = 1.0;
pub const SKILL_MAX: f64 = 20.0;

fn f32(x: f64) -> f64 {
    v8math::fround(x)
}

/// [player, enemy] skill by tier (difficulty.js:35); any other tier
/// falls through to 4/14. Tier keys are the numbers 1/2/3 exactly.
fn base_skills(tier: f64) -> (f64, f64) {
    if tier == EASY {
        (6.0, 4.0)
    } else if tier == HARD {
        (4.0, 17.0)
    } else if tier == MEDIUM {
        (4.0, 14.0)
    } else {
        (4.0, 14.0)
    }
}

/// Each curve's values at skill 1, 14 and 20 (difficulty.js:39).
pub const ANCHORS_A: [f64; 3] = [0.8, 1.9, 20.0];
pub const ANCHORS_D: [f64; 3] = [0.8, 1.9, 2.4];
pub const ANCHORS_B: [f64; 3] = [3.0, 1.6, 1.0];
pub const ANCHORS_S: [f64; 3] = [2.0, 1.0, 0.0];

/// The danger notifier's delay rows (difficulty.js:49).
pub const DANGER_ROWS: [[f64; 2]; 3] = [[1.1, 2.1], [0.6, 1.3], [0.0, 0.8]];

/// difficulty.js:55 (`value | 0`, then clamped).
pub fn clamp_skill(value: f64) -> f64 {
    v8math::min(v8math::max(v8math::to_int32(value) as f64, SKILL_MIN), SKILL_MAX)
}

/// A curve at an integer skill (difficulty.js:61).
pub fn curve(anchors: [f64; 3], skill: f64) -> f64 {
    let below = skill < 14.0;
    let lo = f32(if below { anchors[0] } else { anchors[1] });
    let hi = f32(if below { anchors[1] } else { anchors[2] });
    let t = f32(if below { (skill - 1.0) / 13.0 } else { (skill - 14.0) / 6.0 });
    f32(lo + f32(t * f32(hi - lo)))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Curves {
    pub a: f64,
    pub d: f64,
    pub b: f64,
    pub s: f64,
}

pub fn skill_curves(skill: f64) -> Curves {
    Curves { a: curve(ANCHORS_A, skill), d: curve(ANCHORS_D, skill), b: curve(ANCHORS_B, skill), s: curve(ANCHORS_S, skill) }
}

/// A mission script's number argument (difficulty.js:80). Only finite
/// script numbers reach here (Number(value) || 0 in the JavaScript:
/// NaN and +-0 become 0, which the caller spells).
fn lua_int(value: f64) -> f64 {
    v8math::trunc(f32(value))
}

/// The 2004 game's float range draw (difficulty.js:86): lo itself and
/// no draw when the ends are equal. `draw` produces the stream's
/// uniform.
pub fn draw_range(draw: &mut dyn FnMut() -> f64, lo: f64, hi: f64) -> f64 {
    let lo = f32(lo);
    let hi = f32(hi);
    if lo == hi {
        return lo;
    }
    let u = draw();
    f32(f32(u * f32(hi - lo)) + lo)
}

/// ClassicDifficulty's options (difficulty.js:113).
#[derive(Clone, Debug)]
pub struct DifficultyOptions {
    pub tier: f64,
    pub profile_tier: f64,
    pub player_team: Option<f64>,
    pub second_player_team: Option<f64>,
    pub network: bool,
    pub override_: bool,
    pub skills: (f64, f64),
}

impl Default for DifficultyOptions {
    fn default() -> Self {
        DifficultyOptions { tier: MEDIUM, profile_tier: MEDIUM, player_team: None, second_player_team: None, network: false, override_: false, skills: (4.0, 14.0) }
    }
}

/// ClassicDifficulty, or EnhancedDifficulty when `enhanced` (it
/// overrides init/apply/advancedTraining and the schedule).
#[derive(Clone, Debug)]
pub struct Difficulty {
    pub tier: f64,
    pub profile_tier: f64,
    pub player_team: Option<f64>,
    pub second_player_team: Option<f64>,
    pub network: bool,
    pub override_: bool,
    pub player_skill: f64,
    pub enemy_skill: f64,
    pub player: Curves,
    pub enemy: Curves,
    pub enhanced: bool,
}

impl Difficulty {
    pub fn classic(options: DifficultyOptions) -> Self {
        let mut d = Difficulty {
            tier: options.tier,
            profile_tier: options.profile_tier,
            player_team: options.player_team,
            second_player_team: options.second_player_team,
            network: options.network,
            override_: options.override_,
            player_skill: clamp_skill(options.skills.0),
            enemy_skill: clamp_skill(options.skills.1),
            player: Curves::default(),
            enemy: Curves::default(),
            enhanced: false,
        };
        d.init(0.0, 0.0);
        d
    }

    pub fn enhanced(tier: f64, player_team: Option<f64>, network: bool) -> Self {
        let mut d = Difficulty {
            tier,
            profile_tier: tier,
            player_team,
            second_player_team: None,
            network,
            override_: false,
            player_skill: clamp_skill(4.0),
            enemy_skill: clamp_skill(14.0),
            player: Curves::default(),
            enemy: Curves::default(),
            enhanced: true,
        };
        d.init(0.0, 0.0);
        d
    }

    /// difficulty.js:137 (Enhanced: enhanced/difficulty.js init).
    pub fn init(&mut self, player_offset: f64, enemy_offset: f64) {
        if self.enhanced {
            let skill = clamp_skill(enhanced_skill(self.tier));
            self.player_skill = skill;
            self.enemy_skill = skill;
            self.refresh();
            return;
        }
        if !self.override_ {
            let (player, enemy) = base_skills(self.tier);
            self.player_skill = clamp_skill(player + v8math::to_int32(player_offset) as f64);
            self.enemy_skill = clamp_skill(enemy + v8math::to_int32(enemy_offset) as f64);
        }
        self.refresh();
    }

    pub fn advanced_training(&mut self) {
        if self.enhanced {
            return;
        }
        self.init(4.0, 0.0);
    }

    pub fn refresh(&mut self) {
        self.enemy = skill_curves(self.enemy_skill);
        self.player = skill_curves(self.player_skill);
    }

    pub fn set_player(&mut self, value: f64) {
        self.player_skill = clamp_skill(lua_int(value));
        self.refresh();
    }

    pub fn set_enemy(&mut self, value: f64) {
        self.enemy_skill = clamp_skill(lua_int(value));
        self.refresh();
    }

    pub fn add_player(&mut self, tier: f64, value: f64) {
        if self.profile_tier != tier {
            return;
        }
        self.player_skill = clamp_skill(self.player_skill + lua_int(value));
        self.refresh();
    }

    pub fn add_enemy(&mut self, tier: f64, value: f64) {
        if self.profile_tier != tier {
            return;
        }
        self.enemy_skill = clamp_skill(self.enemy_skill + lua_int(value));
        self.refresh();
    }

    /// Mission calls in source order (difficulty.js:181). Values are
    /// the calls' first arguments as numbers (Number(value) || 0 is
    /// the caller's, in lua_int's input here).
    pub fn apply(&mut self, calls: &[MissionCall]) {
        if self.enhanced {
            return;
        }
        for call in calls {
            let Some(&value) = call.args.first() else { continue };
            match call.name.as_str() {
                "SetPlayerTeamDifficulty" => self.set_player(value),
                "SetEnemyTeamDifficulty" => self.set_enemy(value),
                "SetPlayerTeamDifficultyEasy" => self.add_player(EASY, value),
                "SetPlayerTeamDifficultyMedium" => self.add_player(MEDIUM, value),
                "SetPlayerTeamDifficultyHard" => self.add_player(HARD, value),
                "SetEnemyTeamDifficultyEasy" => self.add_enemy(EASY, value),
                "SetEnemyTeamDifficultyMedium" => self.add_enemy(MEDIUM, value),
                "SetEnemyTeamDifficultyHard" => self.add_enemy(HARD, value),
                _ => {}
            }
        }
    }

    /// difficulty.js:193.
    pub fn is_player_team(&self, team: f64) -> bool {
        if self.network || self.player_team.is_none() {
            return false;
        }
        if let Some(second) = self.second_player_team {
            if Some(second) != self.player_team {
                return false;
            }
        }
        Some(team) == self.player_team
    }

    pub fn skill(&self, team: f64) -> f64 {
        if self.is_player_team(team) { self.player_skill } else { self.enemy_skill }
    }

    pub fn curves(&self, team: f64) -> Curves {
        if self.is_player_team(team) { self.player } else { self.enemy }
    }

    pub fn hard(&self) -> bool {
        self.tier == HARD
    }

    pub fn cycle(&self) -> f64 {
        if self.enhanced {
            return enhanced_schedule(self.tier).0;
        }
        if self.hard() { 3.0 } else { 15.0 }
    }

    pub fn los_budget(&self) -> f64 {
        if self.enhanced {
            return enhanced_schedule(self.tier).1;
        }
        if self.hard() { 12.0 } else { 6.0 }
    }

    pub fn vehicle_bucket(&self) -> f64 {
        if self.hard() { 10.0 } else { 2.0 }
    }

    pub fn danger_row(&self) -> [f64; 2] {
        let k = self.enemy_skill;
        DANGER_ROWS[if k < 17.0 { if k < 14.0 { 0 } else { 1 } } else { 2 }]
    }

    pub fn danger_delay(&self, draws: &mut dyn Draws) -> f64 {
        let [lo, hi] = self.danger_row();
        draw_range(&mut || draws.a_uniform(), lo, hi)
    }
}

/// One mission script call (ai-mission.json): its name and its numeric
/// arguments as the wrappers read them (Number(value) || 0 applied by
/// the caller that hands them here; NaN becomes 0 there).
#[derive(Clone, Debug, PartialEq)]
pub struct MissionCall {
    pub name: String,
    pub args: Vec<f64>,
    /// String arguments in the same positions (only the dense setters
    /// read them as strings; elsewhere the numeric reading applies).
    pub str_args: Vec<String>,
}

/// Every team's skill by tier, Enhanced (enhanced/difficulty.js).
pub fn enhanced_skill(tier: f64) -> f64 {
    if tier == EASY {
        8.0
    } else if tier == HARD {
        17.0
    } else {
        14.0
    }
}

/// (cycle, losBudget) by tier, Enhanced.
pub fn enhanced_schedule(tier: f64) -> (f64, f64) {
    if tier == EASY {
        (15.0, 6.0)
    } else if tier == HARD {
        (6.0, 12.0)
    } else {
        (10.0, 9.0)
    }
}
