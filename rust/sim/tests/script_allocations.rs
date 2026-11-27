//! Counting-allocator verification that steady-state script ticks scoring kills
//! or advancing a visible HUD timer perform zero heap allocations.

use bfe_family::GameFamily;
use bfe_sim::battle::rules::events::{
    Event, EventDriven, EventQueue, EventRules, Outcome, Reinforcements, RuleActions, RuleError,
    RuleResult,
};
use bfe_sim::battle::script::{ScriptChain, ScriptChunk, ScriptRules, ScriptRulesConfig};
use bfe_sim::gen::{Character, F3UnitKey};
use bfe_sim::store::{Ref, StrId};

#[global_allocator]
static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

fn count_allocations(f: impl FnOnce()) -> u32 {
    bfe_test_alloc::measure_thread(f).1
}

const FIXTURE_ARENA_ROSTER: &[u8] = include_bytes!("lua/01_arena_roster.luac");
const FIXTURE_ARENA_MATCH: &[u8] = include_bytes!("lua/01_arena_match.luac");
const FIXTURE_FINITE_TICKETS: &[u8] = include_bytes!("lua/02_finite_tickets_timers.luac");

struct SteadyActions {
    scores: [u32; 4],
    reinf: [Reinforcements; 4],
    score_limit: Option<u32>,
    c1: Ref<Character>,
    c2: Ref<Character>,
    outcome: Option<Outcome>,
}

impl SteadyActions {
    fn new(score_limit: Option<u32>) -> Self {
        Self {
            scores: [0; 4],
            reinf: [Reinforcements::Unlimited; 4],
            score_limit,
            c1: Ref::new(1, 1),
            c2: Ref::new(2, 1),
            outcome: None,
        }
    }
}

impl RuleActions for SteadyActions {
    fn score(&self, team: f64) -> Option<u32> {
        self.scores.get(team as usize).copied()
    }
    fn set_team_points(&mut self, team: f64, points: u32) -> RuleResult<()> {
        let slot = self.scores.get_mut(team as usize).ok_or(RuleError::InvalidValue)?;
        *slot = points;
        Ok(())
    }
    fn add_team_points(&mut self, team: f64, delta: i32) -> RuleResult<u32> {
        let slot = self.scores.get_mut(team as usize).ok_or(RuleError::InvalidValue)?;
        *slot = slot.saturating_add_signed(delta);
        Ok(*slot)
    }
    fn score_limit(&self) -> Option<u32> {
        self.score_limit
    }
    fn reinforcements(&self, team: f64) -> RuleResult<Reinforcements> {
        self.reinf
            .get(team as usize)
            .copied()
            .ok_or(RuleError::InvalidValue)
    }
    fn set_reinforcements(&mut self, team: f64, count: Reinforcements) -> RuleResult<()> {
        let slot = self.reinf.get_mut(team as usize).ok_or(RuleError::InvalidValue)?;
        *slot = count;
        Ok(())
    }
    fn add_reinforcements(&mut self, team: f64, delta: i32) -> RuleResult<Reinforcements> {
        let cur = self.reinforcements(team)?;
        let next = match cur {
            Reinforcements::Unlimited => Reinforcements::Unlimited,
            Reinforcements::Finite(n) => Reinforcements::Finite(n.saturating_add_signed(delta)),
        };
        self.set_reinforcements(team, next)?;
        Ok(next)
    }
    fn finish(&mut self, outcome: Outcome) -> RuleResult<()> {
        self.outcome = Some(outcome);
        Ok(())
    }
    fn character_team(&self, character: Ref<Character>) -> RuleResult<f64> {
        if character == self.c1 {
            Ok(1.0)
        } else if character == self.c2 {
            Ok(2.0)
        } else {
            Err(RuleError::StaleHandle)
        }
    }
    fn character_is_human(&self, character: Ref<Character>) -> RuleResult<bool> {
        Ok(character == self.c1)
    }
    fn start_timer(&mut self, _: StrId, _: f64) -> bool {
        true
    }
    fn stop_timer(&mut self, _: StrId) -> bool {
        true
    }
}

#[test]
fn warm_scoring_ticks_and_visible_hud_timer_perform_zero_allocations() {
    // 1. Warmed kill-scoring ticks on 01_arena_match with live telemetry attached.
    let chain = ScriptChain::new(
        "01_arena_match",
        vec![
            ScriptChunk {
                name: "01_arena_match".to_string(),
                bytes: FIXTURE_ARENA_MATCH.into(),
            },
            ScriptChunk {
                name: "01_arena_roster".to_string(),
                bytes: FIXTURE_ARENA_ROSTER.into(),
            },
        ],
    );
    let mut cfg = ScriptRulesConfig::strict(77);
    cfg.assault_score_limit = Some(1000);
    let mut rules = ScriptRules::new(chain, cfg).unwrap();
    let telemetry = rules.attach_telemetry();
    let mut actions = SteadyActions::new(Some(1000));
    let empty = EventQueue::new(4);

    // Boot tick runs load + ScriptPostLoad.
    rules.tick(0.0, &empty, &mut actions);

    let mut kill_batch = EventQueue::new(4);
    kill_batch.push(Event::UnitKilled {
        unit: F3UnitKey {
            slot: 2,
            generation: 1,
            life: 1.0,
        },
        victim: Some(actions.c2),
        victim_team: 2.0,
        killer: Some(actions.c1),
        killer_team: Some(1.0),
        relation: -1.0,
    });
    // Warm up character registration on kill 1.
    rules.tick(1.0 / 60.0, &kill_batch, &mut actions);
    assert_eq!(telemetry.read().scores[2], 1);

    // Measure 20 consecutive warmed scoring ticks: MUST allocate 0 times!
    let kill_allocs = count_allocations(|| {
        for _ in 0..20 {
            rules.tick(1.0 / 60.0, &kill_batch, &mut actions);
        }
    });
    assert_eq!(telemetry.read().scores[2], 21);
    assert_eq!(
        kill_allocs, 0,
        "warm kill-scoring ticks allocated {kill_allocs} times (expected 0)"
    );

    // 2. Warmed visible HUD timer ticks on 02_finite_tickets_timers (100 ticks).
    let timer_chain = ScriptChain::new(
        "02_finite_tickets_timers",
        vec![ScriptChunk {
            name: "02_finite_tickets_timers".to_string(),
            bytes: FIXTURE_FINITE_TICKETS.into(),
        }],
    );
    let mut timer_rules =
        ScriptRules::new(timer_chain, ScriptRulesConfig::strict(78)).unwrap();
    let timer_tel = timer_rules.attach_telemetry();
    let mut timer_actions = SteadyActions::new(None);
    timer_actions.reinf[1] = Reinforcements::Finite(3);
    timer_actions.reinf[2] = Reinforcements::Finite(2);

    // Boot tick starts phase_clock (0.5s) and shows it on HUD.
    timer_rules.tick(0.0, &empty, &mut timer_actions);
    assert!(timer_tel.read().hud_timer_id.is_some());
    assert!(timer_tel.read().hud_timer_running);

    // 100 ticks of dt = 0.004s (0.40s total, timer stays visible & running): MUST allocate 0 times!
    let timer_allocs = count_allocations(|| {
        for _ in 0..100 {
            timer_rules.tick(0.004, &empty, &mut timer_actions);
        }
    });
    assert!(timer_tel.read().hud_timer_running);
    assert!(timer_tel.read().hud_timer_remaining > 0.05);
    assert_eq!(
        timer_allocs, 0,
        "100 ticks of a visible running HUD timer allocated {timer_allocs} times (expected 0)"
    );

    // Keep unused imports clean (verify EventDriven & GameFamily stay in scope if needed).
    let _ = (GameFamily::Swbf2, core::any::type_name::<EventDriven>());
}
