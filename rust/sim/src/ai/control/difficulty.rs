//! Classic difficulty and its Enhanced flavor
//!: the tier's skills, the four aim curves of
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

/// [player, enemy] skill by tier for 2004 battles; any other tier
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

/// [player, enemy] skill by tier for 2005 battles.
fn base_skills_swbf2(tier: f64) -> (f64, f64) {
    if tier == EASY {
        (6.0, 4.0)
    } else if tier == HARD {
        (11.0, 17.0)
    } else if tier == MEDIUM {
        (11.0, 14.0)
    } else {
        (11.0, 14.0)
    }
}

/// Each curve's values at skill 1, 14 and 20.
pub const ANCHORS_A: [f64; 3] = [0.8, 1.9, 20.0];
pub const ANCHORS_D: [f64; 3] = [0.8, 1.9, 2.4];
pub const ANCHORS_B: [f64; 3] = [3.0, 1.6, 1.0];
pub const ANCHORS_S: [f64; 3] = [2.0, 1.0, 0.0];

/// The danger notifier's delay rows.
pub const DANGER_ROWS: [[f64; 2]; 3] = [[1.1, 2.1], [0.6, 1.3], [0.0, 0.8]];

/// (`value | 0`, then clamped).
pub fn clamp_skill(value: f64) -> f64 {
    v8math::min(v8math::max(v8math::to_int32(value) as f64, SKILL_MIN), SKILL_MAX)
}

/// A curve at an integer skill.
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

/// Skill aim curves for 2005 battles: cubic acceleration and linear damping/spread over 1..=20.
pub fn skill_curves_swbf2(skill: f64) -> Curves {
    let k = clamp_skill(skill);
    let t = f32((k - 1.0) / 19.0);
    let t3 = f32(f32(t * t) * t);
    let lin = |lo: f64, hi: f64| f32(f32(lo) + f32(t * f32(f32(hi) - f32(lo))));
    Curves {
        a: f32(f32(0.8) + f32(t3 * f32(f32(14.0) - f32(0.8)))),
        d: lin(0.8, 2.4),
        b: lin(3.0, 1.0),
        s: lin(2.0, 0.0),
    }
}

/// A mission script's number argument. Only finite
/// script numbers reach here (Number(value) || 0 in the JavaScript:
/// NaN and +-0 become 0, which the caller spells).
fn lua_int(value: f64) -> f64 {
    v8math::trunc(f32(value))
}

/// The 2004 game's float range draw: lo itself and
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

/// ClassicDifficulty's options.
#[derive(Clone, Debug)]
pub struct DifficultyOptions {
    pub tier: f64,
    pub profile_tier: f64,
    pub player_team: Option<f64>,
    pub second_player_team: Option<f64>,
    pub network: bool,
    pub override_: bool,
    pub skills: (f64, f64),
    pub swbf2: bool,
}

impl Default for DifficultyOptions {
    fn default() -> Self {
        DifficultyOptions { tier: MEDIUM, profile_tier: MEDIUM, player_team: None, second_player_team: None, network: false, override_: false, skills: (4.0, 14.0), swbf2: false }
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
    pub swbf2: bool,
    /// The game's AI auto balance offsets (player side, enemy side), set each AI update in a
    /// 2005 Classic battle (`auto_balance`) and added to the side's skill; 0 otherwise.
    pub balance: (f64, f64),
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
            swbf2: options.swbf2,
            balance: (0.0, 0.0),
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
            swbf2: false,
            balance: (0.0, 0.0),
        };
        d.init(0.0, 0.0);
        d
    }

    /// (Enhanced: enhanced/difficulty.js init).
    pub fn init(&mut self, player_offset: f64, enemy_offset: f64) {
        if self.enhanced {
            let skill = clamp_skill(enhanced_skill(self.tier));
            self.player_skill = skill;
            self.enemy_skill = skill;
            self.refresh();
            return;
        }
        if !self.override_ {
            let (player, enemy) = if self.swbf2 { base_skills_swbf2(self.tier) } else { base_skills(self.tier) };
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
        let curves = if self.swbf2 { skill_curves_swbf2 } else { skill_curves };
        let (player, enemy) = self.effective();
        self.enemy = curves(enemy);
        self.player = curves(player);
    }

    /// The sides' skills with the auto balance offsets, clamped as the game clamps the sum.
    pub fn effective(&self) -> (f64, f64) {
        if self.balance == (0.0, 0.0) {
            return (self.player_skill, self.enemy_skill);
        }
        (clamp_skill(self.player_skill + self.balance.0), clamp_skill(self.enemy_skill + self.balance.1))
    }

    /// Set this AI update's auto balance offsets (the game zeroes and recomputes them each update).
    pub fn set_balance(&mut self, player: f64, enemy: f64) {
        if self.balance != (player, enemy) {
            self.balance = (player, enemy);
            self.refresh();
        }
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

    /// Mission calls in source order. Values are
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
        let (player, enemy) = self.effective();
        if self.is_player_team(team) { player } else { enemy }
    }

    pub fn opposing_skill(&self, team: f64) -> f64 {
        let (player, enemy) = self.effective();
        if self.network || self.player_team.is_none() || self.is_player_team(team) {
            enemy
        } else {
            player
        }
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

/// Every team's skill by tier, Enhanced.
/// What the game's AI auto balance reads at one AI update (00439470, 004393b0, 00439320).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BalanceInputs {
    /// The mission's SetFlagGameplayType name ("1flag", "2flag", "campaign" matter).
    pub flag_mode: FlagMode,
    /// SpaceAssaultEnable was called.
    pub space_assault: bool,
    /// Teams 1 and 2: live reinforcement counts and their maxima (i32::MAX when unlimited).
    pub counts: [i32; 2],
    pub maxima: [i32; 2],
    /// Teams 1 and 2: points.
    pub points: [i32; 2],
    /// The local player's team (1 or 2); the other is the enemy.
    pub player_team: usize,
}

/// SetFlagGameplayType as the game maps it (hashes of "1flag", "2flag" and "campaign").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagMode { Other, OneFlag, TwoFlag, Campaign }

impl FlagMode {
    pub fn named(name: &str) -> Self {
        if name.eq_ignore_ascii_case("1flag") {
            FlagMode::OneFlag
        } else if name.eq_ignore_ascii_case("2flag") {
            FlagMode::TwoFlag
        } else if name.eq_ignore_ascii_case("campaign") {
            FlagMode::Campaign
        } else {
            FlagMode::Other
        }
    }
}

/// The game's AI auto balance at one AI update: (player-side offset, enemy-side offset). Mode:
/// flag games ("1flag", "2flag") compare points with no threshold, step 1; "campaign" is off;
/// space assault compares points, threshold 20, step 10; otherwise reinforcements when both
/// teams have under 1,000, threshold 15, step 10; else off. A bias of -20 (the enemy's count
/// under a quarter of its maximum) or -10 (under a half) is subtracted from the player team's
/// value; past the threshold the enemy offset is (difference - signed threshold) / step,
/// truncated, the player offset its negative, each within -4..4.
pub fn auto_balance(i: &BalanceInputs) -> (i32, i32) {
    let mode = match i.flag_mode {
        FlagMode::OneFlag | FlagMode::TwoFlag => 0,
        FlagMode::Campaign => 3,
        FlagMode::Other if i.space_assault => 1,
        FlagMode::Other if i.counts[0] < 1000 && i.counts[1] < 1000 => 2,
        FlagMode::Other => 3,
    };
    let (mine_at, other_at) = if i.player_team == 2 { (1, 0) } else { (0, 1) };
    let (values, threshold, step) = match mode {
        0 => (i.points, 0, 1),
        1 => (i.points, 20, 10),
        2 => (i.counts, 15, 10),
        _ => return (0, 0),
    };
    let (count, max) = (i.counts[other_at] as f32, i.maxima[other_at] as f32);
    let bias = if max * 0.25 > count { -20 } else if count < max * 0.5 { -10 } else { 0 };
    let e = values[mine_at].saturating_sub(bias).saturating_sub(values[other_at]);
    if e.saturating_abs() <= threshold {
        return (0, 0);
    }
    let signed = if e < 1 { -threshold } else { threshold };
    let enemy = e.saturating_sub(signed) / step;
    (enemy.saturating_neg().clamp(-4, 4), enemy.clamp(-4, 4))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_games_auto_balance_reads_its_mode_threshold_step_and_clamp() {
        let space = |player: i32, enemy: i32| BalanceInputs {
            flag_mode: FlagMode::Other, space_assault: true, counts: [i32::MAX; 2], maxima: [i32::MAX; 2],
            points: [player, enemy], player_team: 1,
        };
        assert_eq!(auto_balance(&space(7, 17)), (0, 0), "a 10-point gap is under the threshold of 20");
        assert_eq!(auto_balance(&space(7, 47)), (2, -2), "the side behind gains");
        assert_eq!(auto_balance(&space(7, 77)), (4, -4), "at most 4");
        assert_eq!(auto_balance(&space(60, 10)), (-3, 3), "the side ahead loses");
        let mut flag = space(3, 1);
        flag.space_assault = false;
        flag.flag_mode = FlagMode::named("1flag");
        assert_eq!(auto_balance(&flag), (-2, 2), "flag games: no threshold, step 1");
        flag.flag_mode = FlagMode::named("Campaign");
        assert_eq!(auto_balance(&flag), (0, 0));
        let mut hero = space(0, 100);
        hero.space_assault = false;
        assert_eq!(auto_balance(&hero), (0, 0), "unlimited reinforcements and no flag type: off");
        let tickets = |count: [i32; 2]| BalanceInputs {
            flag_mode: FlagMode::Other, space_assault: false, counts: count, maxima: [600, 900],
            points: [0, 0], player_team: 2,
        };
        assert_eq!(auto_balance(&tickets([500, 520])), (0, 0), "(20 - 15) / 10 truncates to 0");
        assert_eq!(auto_balance(&tickets([500, 560])), (-4, 4), "team 2 ahead by 60: (60 - 15) / 10 = 4");
        assert_eq!(auto_balance(&tickets([100, 70])), (0, 0), "the enemy under a quarter of its maximum: 70 + 20 - 100 is within 15");
        assert_eq!(auto_balance(&tickets([400, 360])), (2, -2), "no bias at 400 of 600: (-40 + 15) / 10 truncates to -2");
    }

    #[test]
    fn auto_balance_offsets_add_to_each_sides_skill_within_one_to_twenty() {
        let mut d = Difficulty::classic(DifficultyOptions { tier: MEDIUM, profile_tier: MEDIUM, player_team: Some(1.0), swbf2: true, ..Default::default() });
        let (player, enemy) = (d.skill(1.0), d.skill(2.0));
        d.set_balance(4.0, -4.0);
        assert_eq!((d.skill(1.0), d.skill(2.0)), (player + 4.0, enemy - 4.0));
        assert_eq!(d.opposing_skill(1.0), enemy - 4.0);
        d.set_balance(40.0, -40.0);
        assert_eq!((d.skill(1.0), d.skill(2.0)), (20.0, 1.0));
        d.set_balance(0.0, 0.0);
        assert_eq!((d.skill(1.0), d.skill(2.0)), (player, enemy));
    }

    #[test]
    fn family_base_skills_keep_2004_unchanged_and_raise_2005_player_team() {
        for (tier, classic, swbf2) in [
            (EASY, (6.0, 4.0), (6.0, 4.0)),
            (MEDIUM, (4.0, 14.0), (11.0, 14.0)),
            (HARD, (4.0, 17.0), (11.0, 17.0)),
        ] {
            let d2004 = Difficulty::classic(DifficultyOptions { tier, profile_tier: tier, ..Default::default() });
            assert_eq!((d2004.player_skill, d2004.enemy_skill), classic);
            let d2005 = Difficulty::classic(DifficultyOptions { tier, profile_tier: tier, swbf2: true, ..Default::default() });
            assert_eq!((d2005.player_skill, d2005.enemy_skill), swbf2);
        }
        let mut with_offset = Difficulty::classic(DifficultyOptions { tier: MEDIUM, profile_tier: MEDIUM, swbf2: true, ..Default::default() });
        with_offset.apply(&[MissionCall { name: "SetPlayerTeamDifficultyMedium".into(), args: vec![2.0], str_args: vec![] }]);
        assert_eq!((with_offset.player_skill, with_offset.enemy_skill), (13.0, 14.0));
        let c1 = skill_curves_swbf2(1.0);
        let c20 = skill_curves_swbf2(20.0);
        assert_eq!((c1.a, c1.d, c1.b, c1.s), (f32(0.8), f32(0.8), f32(3.0), f32(2.0)));
        assert_eq!((c20.a, c20.d, c20.b, c20.s), (f32(14.0), f32(2.4), f32(1.0), f32(0.0)));
        let cm = skill_curves_swbf2(11.0);
        assert!(cm.a > 2.5 && cm.a < 3.0);
        let mut teamed = Difficulty::classic(DifficultyOptions { tier: MEDIUM, profile_tier: MEDIUM, player_team: Some(1.0), swbf2: true, ..Default::default() });
        assert_eq!(teamed.opposing_skill(1.0), 14.0);
        assert_eq!(teamed.opposing_skill(2.0), 11.0);
        teamed.player_team = None;
        assert_eq!(teamed.opposing_skill(2.0), 14.0);
    }
}
