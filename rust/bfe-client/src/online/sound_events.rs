//! GameAudio event facts, kept separate from the audio runtime's playback
//! gates. Network health and cosmetic publications never advance gameplay.
use audio_control::layout::{emitter as e, event as v, event_kind as k};
// SoundControl.event hurt/death names the qualified actor. Reusing a receiver
// slot for a respawn must not turn its initial health into a wound event.
pub fn damage(before: &[f64], after: &[f64], own: bool) -> Option<[f64; v::STRIDE]> {
    if before[e::KIND] == 0.
        || before[e::GENERATION] != after[e::GENERATION]
        || before[e::ALIVE] == 0.
    {
        return None;
    }
    let amount = before[e::HEALTH] - after[e::HEALTH];
    let dead = after[e::ALIVE] == 0.;
    if amount <= 0. && !dead {
        return None;
    }
    let mut out = [0.; v::STRIDE];
    out[v::KIND] = match (own, dead) {
        (true, true) => k::DEATH,
        (true, false) => k::HURT,
        (false, true) => k::ACTOR_DEATH,
        (false, false) => k::ACTOR_HURT,
    } as f64;
    out[v::OWNER] = after[e::ID];
    out[v::CLASS] = after[e::CLASS];
    out[v::AMOUNT] = amount.max(0.);
    // SoundControl.event passes the actor's remaining health to the
    // soldierVoice wound threshold, rather than the pre-hit amount.
    out[v::HEALTH] = after[e::HEALTH];
    Some(out)
}
// NetworkCombatViews terminal dispatch emits splash for water (surface 4),
// otherwise impact. Definition ids were resolved to sound classes at load.
pub fn impact(source: &bfe_sim::wire::cosmetics::Compact, class: usize) -> [f64; v::STRIDE] {
    let mut out = [0.; v::STRIDE];
    out[v::KIND] = if source.surface == 4. {
        k::SPLASH
    } else {
        k::IMPACT
    } as f64;
    out[v::CLASS] = class as f64;
    out[v::PLACED] = 1.;
    out[v::X..v::X + 3].copy_from_slice(&source.view.position.unwrap_or_default());
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn health_edges_ignore_healing_and_new_generations() {
        let mut a = [0.; e::STRIDE];
        a[e::KIND] = 1.;
        a[e::ID] = 2.;
        a[e::CLASS] = 3.;
        a[e::GENERATION] = 1.;
        a[e::ALIVE] = 1.;
        a[e::HEALTH] = 300.;
        let mut b = a;
        b[e::HEALTH] = 180.;
        let hit = damage(&a, &b, true).unwrap();
        assert_eq!(hit[v::KIND], k::HURT as f64);
        assert_eq!(hit[v::AMOUNT], 120.);
        assert_eq!(hit[v::HEALTH], 180.);
        assert!(damage(&b, &a, true).is_none());
        b[e::ALIVE] = 0.;
        assert_eq!(
            damage(&a, &b, false).unwrap()[v::KIND],
            k::ACTOR_DEATH as f64
        );
        b[e::GENERATION] = 2.;
        assert!(damage(&a, &b, true).is_none());
    }
    #[test]
    fn terminal_uses_world_position_and_water_sound() {
        let mut source = bfe_sim::wire::cosmetics::Compact::default();
        source.surface = 4.;
        source.view.position = Some([1., 2., 3.]);
        let row = impact(&source, 7);
        assert_eq!(row[v::KIND], k::SPLASH as f64);
        assert_eq!(row[v::CLASS], 7.);
        assert_eq!(&row[v::X..v::X + 3], &[1., 2., 3.]);
    }
}
