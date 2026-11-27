//! Team waves and personal readiness for a 2005 mission.
use super::*;

#[derive(Debug)]
pub(crate) struct Waves {
    tick: u32,
    next_wave: u32,
    period: u32,
    stagger: u32,
    next_slot: [u32; TEAM_COUNT],
    network: bool,
    class_limits: [Vec<usize>; TEAM_COUNT],
}

impl Waves {
    pub(crate) fn new(
        period: f64,
        stagger: f64,
        network: bool,
        teams: &std::collections::BTreeMap<i32, crate::battle::script::TeamSetup>,
    ) -> Self {
        let class_limits = std::array::from_fn(|i| {
            teams
                .get(&(i as i32))
                .map(|t| {
                    t.classes
                        .iter()
                        .map(|c| c.max_units.map_or(usize::MAX, |n| n.max(0) as usize))
                        .collect()
                })
                .unwrap_or_default()
        });
        Self {
            tick: 0,
            next_wave: 6,
            period: ticks(if network { 15.0 } else { period }),
            stagger: ticks(stagger),
            next_slot: [0; TEAM_COUNT],
            network,
            class_limits,
        }
    }

    pub(crate) fn advance(&mut self, s: &mut Store) {
        // The clock stops at its bound (2.2 years of play): a tick that did not happen brings no wave.
        let before = self.tick;
        self.tick = self.tick.saturating_add(1);
        if self.tick == before {
            return;
        }
        if self.tick >= self.next_wave {
            s.spawning.wave += 1.0;
            self.next_wave = self.tick.saturating_add(self.period);
            self.next_slot.fill(self.tick.saturating_add(self.stagger));
        }
        s.spawning.wave_countdown = self.next_wave.saturating_sub(self.tick) as f64 / 60.0;
    }

    /// The ticks from now until a Character without a body may have one, if he
    /// is ready by then: his own wait after a death and, in a network battle,
    /// the spawn wave after it and his team's next slot in it. Zero once he
    /// may; none for a place that waits for nobody.
    pub(crate) fn opens_in(&self, s: &Store, c: &Character) -> Option<u32> {
        let spawn = c.spawn.as_ref().filter(|spawn| c.unit.is_none() && spawn.timer.is_finite())?;
        let own = jsrt::js_round(spawn.timer.max(0.0) * 60.0) as u32;
        if c.human && !self.network {
            return Some(own);
        }
        let next = self.next_wave.saturating_sub(self.tick);
        // The wait that ends on the tick of a wave has missed that wave.
        let waves = if own > 0 {
            if own < next { 0 } else { (own - next) / self.period + 1 }
        } else {
            let ahead = num(spawn.wave) - s.spawning.wave;
            if ahead <= 0.0 {
                let slot = self.next_slot.get(c.team as usize).copied().unwrap_or(0);
                return Some(slot.saturating_sub(self.tick));
            }
            (ahead as u32).saturating_sub(1)
        };
        Some(next.saturating_add(waves.saturating_mul(self.period)).saturating_add(self.stagger))
    }

    pub(crate) fn update<W: SpawningWorld + ?Sized>(
        &mut self,
        s: &mut Store,
        strings: &dyn ContextText<W>,
        world: &mut W,
        random: &mut dyn ContextDraws<W>,
        scratch: &mut Scratch,
    ) {
        for i in 0..s.spawning.characters.len() {
            let r = s.spawning.characters[i];
            if ch(s, r).unit.is_some() {
                continue;
            }
            let time = sp(ch(s, r)).timer;
            // A parked place never counts down: it waits for a player to take it.
            if !time.is_finite() {
                continue;
            }
            if time > 0.0 {
                let remaining = jsrt::js_round(time * 60.0) as u32;
                let remaining = remaining.saturating_sub(1);
                sp_mut(ch_mut(s, r)).timer = remaining as f64 / 60.0;
                if remaining != 0 {
                    continue;
                }
                prepare_with_scratch(s, r, strings, world, random, scratch);
            }
            let c = ch(s, r);
            let t = c.team as usize;
            let human = c.human;
            if t >= TEAM_COUNT
                || !sp(c).ready
                || spawn_tickets(s, t as f64, world) <= team(&s.spawning, t as f64).living
            {
                continue;
            }
            if (!human || self.network)
                && (num(sp(c).wave) > s.spawning.wave || self.next_slot[t] > self.tick)
            {
                continue;
            }
            let post = choose_post_with_scratch(s, r, world, random, scratch);
            change_post(s, r, post);
            if post.is_none() {
                continue;
            }
            // A human keeps the class chosen on the spawn screen. A bot takes the
            // class the 2005 game's picker gives it, or waits when none has room.
            let chosen = ch(s, r)
                .class_name
                .and_then(|n| s.spawning.teams[t].classes.iter().position(|c| c.name == n));
            if human {
                if chosen.is_none_or(|i| i >= CLASS_CAPACITY) {
                    continue;
                }
            } else {
                let alive = alive_by_class(s, t);
                let Some(i) = self.bot_class(s, t, &alive, random, world) else {
                    continue;
                };
                if chosen != Some(i) {
                    set_class(s, r, i as f64, strings, world);
                }
            }
            let post = post.unwrap();
            let team = t as f64;
            let allied = team != owner_of(posts::team_of(s, post).0);
            let point = spawn_point(s, post, allied, world, random, scratch);
            let class = ch(s, r).class_name;
            let unit = world.spawn_unit(
                s,
                r,
                class,
                post,
                [point[0], point[1], point[2]],
                Some(point[3]),
            );
            if unit.is_none() {
                continue;
            }
            finish_spawn(s, r, team, post, unit, world);
            if !human || self.network {
                self.next_slot[t] = self.tick.saturating_add(self.stagger);
            }
        }
    }
}
/// How many members of the team's roster live in each class.
fn alive_by_class(s: &Store, t: usize) -> [usize; CLASS_CAPACITY] {
    let roster = &s.spawning.teams[t];
    let mut alive = [0usize; CLASS_CAPACITY];
    for other in &roster.characters {
        let member = ch(s, *other);
        if let Some(body) = member.unit.and_then(|r| s.unit.get(r)).filter(|u| u.alive) {
            if let Some(index) = roster.classes.iter().position(|k| k.name == body.class_name).filter(|&i| i < CLASS_CAPACITY) {
                alive[index] += 1;
            }
        }
    }
    alive
}

/// A character a player holds: human, with a body or a running clock (a
/// parked place waits for a player and holds none).
fn player_held(c: &crate::gen::Character) -> bool {
    c.human && (c.unit.is_some() || sp(c).timer.is_finite())
}

impl Waves {
    /// Whether a human may take the class now. The 2005 game limits a human
    /// only in a class with points to unlock (then by those points and the
    /// class's maximum); no prepared class carries any, so a human may take
    /// every class the team lists, whatever the bots hold.
    pub(crate) fn has_room(&self, s: &Store, team: usize, class: StrId) -> bool {
        team < TEAM_COUNT
            && s.spawning.teams[team].classes.iter().take(CLASS_CAPACITY).any(|k| k.name == class)
    }

    /// The class a bot of team `t` takes, as the 2005 game picks one: going
    /// through the team's classes in a shuffled order, one under its minimum,
    /// else one under its maximum, among the classes [`bot_may_take`] allows.
    /// None when no class has room.
    fn bot_class<W: ?Sized>(
        &self,
        s: &Store,
        t: usize,
        alive: &[usize; CLASS_CAPACITY],
        random: &mut dyn ContextDraws<W>,
        world: &mut W,
    ) -> Option<usize> {
        let roster = &s.spawning.teams[t];
        let n = roster.classes.len().min(CLASS_CAPACITY);
        if n == 0 {
            return None;
        }
        let holds = holds_places(self.tick, roster.characters.iter().any(|&c| player_held(ch(s, c))));
        let mut order = [0usize; CLASS_CAPACITY];
        for (i, slot) in order.iter_mut().enumerate().take(n) {
            *slot = i;
        }
        for k in (1..n).rev() {
            let j = random.int_in(world, 0.0, k as f64) as usize;
            order.swap(k, j.min(k));
        }
        let max = |i: usize| self.class_limits[t].get(i).copied().unwrap_or(usize::MAX);
        let open = |i: usize| bot_may_take(roster.classes[i].hero, holds, alive[i], max(i));
        let order = &order[..n];
        order
            .iter()
            .copied()
            .find(|&i| open(i) && (alive[i] as f64) < roster.classes[i].target)
            .or_else(|| order.iter().copied().find(|&i| open(i) && alive[i] < max(i)))
    }
}

/// How long from the battle's start every team's bots leave places free: the
/// 2005 game's picker holds them while its battle clock reads under a minute.
const OPENING_SECONDS: f64 = 60.0;

/// Whether a team's bots leave one place in each class for a player: on every
/// team through the battle's first minute (the mission's own tick, which
/// starts with the battle and stands still while the game is paused), and for
/// as long as a player is on the team.
fn holds_places(tick: u32, player_on_team: bool) -> bool {
    player_on_team || tick < ticks(OPENING_SECONDS)
}

/// Whether a bot may take a class before its minimum and maximum are counted:
/// never a hero class (left to the hero award), and one short of the class's
/// maximum while the team holds places.
fn bot_may_take(hero: bool, holds: bool, alive: usize, max: usize) -> bool {
    !hero && (!holds || alive < max.saturating_sub(1))
}

fn ticks(seconds: f64) -> u32 {
    (jsrt::js_round(seconds * 60.0) as u32).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waves(tick: u32, next_wave: u32) -> Waves {
        let mut waves = Waves::new(20.0, 2.0, false, &std::collections::BTreeMap::new());
        waves.tick = tick;
        waves.next_wave = next_wave;
        waves
    }

    #[test]
    fn a_player_on_the_team_keeps_a_place_in_each_class() {
        assert!(holds_places(90_000, true));
        assert!(!holds_places(90_000, false));
        assert!(bot_may_take(false, false, 4, 5), "without a held place a bot fills to the maximum");
        assert!(!bot_may_take(false, true, 4, 5), "a held place stops a bot one short of the maximum");
        assert!(bot_may_take(false, true, 3, 5));
        assert!(!bot_may_take(false, true, 0, 1), "a class of one is the player's while places are held");
        assert!(!bot_may_take(true, false, 0, 9), "a hero class is left to the award");
    }

    #[test]
    fn every_team_keeps_a_place_in_each_class_through_the_first_minute() {
        let minute = ticks(60.0);
        assert!(holds_places(0, false), "a team without a player holds places when the battle starts");
        assert!(holds_places(minute - 1, false), "and on the last tick of that minute");
        assert!(!holds_places(minute, false), "and no longer once a minute has gone");
        assert!(holds_places(minute, true), "a player on the team keeps holding them");
        assert!(!bot_may_take(false, holds_places(minute / 2, false), 1, 2), "a class of two takes one bot in that minute");
        assert!(bot_may_take(false, holds_places(minute, false), 1, 2), "and its second after it");
    }

    #[test]
    fn a_wave_comes_every_period_below_the_bound() {
        let mut s = Store::default();
        let mut waves = waves(0, 6);
        let period = waves.period;
        for _ in 0..6 {
            waves.advance(&mut s);
        }
        assert_eq!(s.spawning.wave, 1.0);
        assert_eq!(waves.next_wave, 6 + period);
        assert_eq!(s.spawning.wave_countdown, f64::from(period) / 60.0);
    }

    #[test]
    fn the_wave_clock_saturates_and_stops_at_the_bound() {
        let mut s = Store::default();
        let mut waves = waves(u32::MAX - 3, u32::MAX - 1);
        for _ in 0..4 {
            waves.advance(&mut s);
        }
        assert_eq!(waves.tick, u32::MAX);
        assert!(waves.next_wave >= waves.tick - 1, "the next wave is not left behind by a wrapped sum");
        let at_bound = s.spawning.wave;
        assert!((1.0..=2.0).contains(&at_bound), "the waves due before the bound came once each");
        for _ in 0..1000 {
            waves.advance(&mut s);
        }
        assert_eq!(s.spawning.wave, at_bound, "a stopped clock brings no more waves");
        assert!(s.spawning.wave_countdown.is_finite() && s.spawning.wave_countdown >= 0.0);
    }

    use crate::battle::script::{state::UnitClassSetup, TeamSetup};
    use crate::store::Strings;

    fn waiting(s: &mut Store, timer: f64, wave: f64) -> Ref<Character> {
        s.character.insert(Character { team: 1.0, human: true, spawn: Some(Spawn { timer, wave: Some(wave), ..Default::default() }), ..Default::default() })
    }

    // The tick a waiting player is told stays the same until it comes: the
    // opening slot of the wave that follows his own wait.
    #[test]
    fn a_wait_ends_with_the_first_slot_of_the_wave_after_it() {
        for fell in [7u32, 400, 665, 666, 667, 906, 1100] {
            let (mut s, mut waves) = (Store::default(), Waves::new(10.0, 0.5, true, &Default::default()));
            while waves.tick < fell { waves.advance(&mut s); }
            let player = waiting(&mut s, 4.0, 0.0);
            let wave = (0..).map(|k| 6 + 900 * k).find(|wave| *wave > fell + 240).unwrap();
            let due = |waves: &Waves, s: &Store| waves.opens_in(s, ch(s, player)).map(|ticks| waves.tick + ticks);
            for left in (0..240).rev() {
                assert_eq!(due(&waves, &s), Some(wave + 30), "fell at {fell}, at {}", waves.tick);
                waves.advance(&mut s);
                sp_mut(ch_mut(&mut s, player)).timer = f64::from(left) / 60.0;
            }
            // His wait over, he is prepared for the next wave, whenever it comes.
            let next = s.spawning.wave + 1.0;
            sp_mut(ch_mut(&mut s, player)).wave = Some(next);
            while waves.tick < wave + 30 {
                assert_eq!(due(&waves, &s), Some(wave + 30), "fell at {fell}, at {}", waves.tick);
                waves.advance(&mut s);
            }
            assert_eq!(waves.opens_in(&s, ch(&s, player)), Some(0));
            // A teammate who deploys takes a slot before him.
            waves.next_slot[1] = waves.tick + 30;
            assert_eq!(waves.opens_in(&s, ch(&s, player)), Some(30));
            // A place kept for nobody waits for nothing, and a local battle's
            // player for his own four seconds only.
            let parked = waiting(&mut s, f64::INFINITY, 0.0);
            assert_eq!(waves.opens_in(&s, ch(&s, parked)), None);
            let (fallen, local) = (waiting(&mut s, 4.0, 9.0), Waves::new(10.0, 0.5, false, &Default::default()));
            assert_eq!(local.opens_in(&s, ch(&s, fallen)), Some(240));
        }
    }

    // A human may take every class his team lists, whatever the others hold;
    // a class the team does not list, or a team past the count, has none.
    #[test]
    fn a_human_has_room_in_every_listed_class() {
        let (mut s, mut strings) = (Store::default(), Strings::default());
        let [scout, guard, other] = ["scout", "guard", "other"].map(|name| strings.intern_str(name));
        s.spawning.teams = make_teams(8.0);
        s.spawning.teams[1].classes = [scout, guard].map(|name| SpawnClass { name, target: 1.0, hero: false }).to_vec();
        let mut setup = TeamSetup::new(1);
        setup.classes = [("scout", Some(1)), ("guard", None)].map(|(name, max_units)| UnitClassSetup { class_name: name.into(), min_units: 1, max_units }).to_vec();
        let waves = Waves::new(10.0, 0.5, true, &[(1, setup)].into());
        let body = s.unit.insert(Unit { alive: true, class_name: scout, ..Default::default() });
        let member = s.character.insert(Character { team: 1.0, unit: Some(body), ..Default::default() });
        s.spawning.teams[1].characters.push(member);
        assert!(waves.has_room(&s, 1, scout) && waves.has_room(&s, 1, guard));
        assert!(!waves.has_room(&s, 1, other) && !waves.has_room(&s, 9, scout));
    }

    #[test]
    fn alive_ignores_classes_past_class_capacity() {
        let (mut s, mut strings) = (Store::default(), Strings::default());
        s.spawning.teams = make_teams(8.0);
        for i in 0..=CLASS_CAPACITY {
            let name = strings.intern_str(&format!("class_{i}"));
            s.spawning.teams[1].classes.push(SpawnClass { name, target: 1.0, hero: false });
        }
        let overflow_class = s.spawning.teams[1].classes[CLASS_CAPACITY].name;
        let body = s.unit.insert(Unit { alive: true, class_name: overflow_class, ..Default::default() });
        let member = s.character.insert(Character { team: 1.0, unit: Some(body), ..Default::default() });
        s.spawning.teams[1].characters.push(member);
        let counts = alive_by_class(&s, 1);
        assert_eq!(counts, [0; CLASS_CAPACITY]);
    }
}
