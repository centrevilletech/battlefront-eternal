//! The 2005 feeder of the one spawn launcher. It fills the launcher's port from
//! a mission's observation and the replicated HUD metadata. A local battle and
//! an online one keep the same observation on their receiver, so both take
//! this path. The teams, rosters and posts are built once; each overlay frame
//! only updates what the mission changes (who may switch, which classes are
//! open, who holds each post, the countdown, the counts).
use super::Live;
use crate::{
    app::battle::LiveSpawnPort,
    local::mission_view::Observation,
    online::hud::{Hud, HudOwnSummary},
};
use bfe_battle_ui::spawn::{SpawnClassEntry, SpawnPostEntry};
use bfe_shell::page::MAX_CLASSES;

/// The 2005 game's own line for a class that has reached its limit.
const FULL_LINE: &str = "d3cd201e";

/// What the launcher keeps of a 2005 mission between frames.
#[derive(Default)]
pub(in crate::app::battle) struct Feed {
    admitted: bool,
    objectives: (u32, u32),
}

fn phase_of(alive: bool, own: &HudOwnSummary) -> &'static str {
    if alive {
        "alive"
    } else if own.valid {
        if own.ended {
            "ended"
        } else {
            match own.phase {
                0 => "unspawned",
                1 => "alive",
                2 => "dying",
                _ => "awaiting",
            }
        }
    } else {
        "awaiting"
    }
}

/// Whether a class can be chosen, from what the authority publishes to a local
/// player and an online one alike: on the player's own team, the player's
/// summary lists it (an empty list lists all). The 2005 game limits a human only
/// in a class with points to unlock, so a class the bots hold stays open.
pub(super) fn class_available(own_team: bool, listed: &[u16], registry: Option<u16>) -> bool {
    !own_team || listed.is_empty() || registry.is_some_and(|id| listed.contains(&id))
}

/// Marks each class of a team's roster open or closed for the player: listed
/// for him, and with room in the mission (`room`, by roster row, as its
/// authority applies it: a local world's own, an online one's replicated
/// record; a row it does not say has room). The launcher offers only what the
/// authority would accept.
fn open_classes(roster: &mut [SpawnClassEntry], ids: &[usize], own_team: bool, listed: &[u16], room: &[bool]) {
    for (row, (entry, id)) in roster.iter_mut().zip(ids).enumerate() {
        entry.eligible = class_available(own_team, listed, u16::try_from(*id).ok()) && room.get(row).copied().unwrap_or(true);
    }
}

impl Live {
    /// Fills the launcher's port for this frame. `own` is the receiver's team
    /// and whether the player's body is alive.
    pub(in crate::app::battle) fn feed_launcher(
        &mut self,
        port: &mut LiveSpawnPort,
        observed: &Observation,
        hud: &Hud,
        (own_team, own_alive): (u8, bool),
    ) {
        if !self.feed.admitted {
            self.admit_launcher(port, observed, hud);
        }
        let summary = &hud.own_summary;
        let team = if own_team != 0 { own_team } else { summary.team };
        if (1..=2).contains(&team) {
            port.human.team = team;
        }
        let phase = phase_of(own_alive, summary);
        if port.human.phase != phase {
            phase.clone_into(&mut port.human.phase);
        }
        port.human.can_switch_teams = observed.choose_team;
        port.human.over = observed.score.outcome.is_some();
        port.spawn_eligibility = *summary;
        if summary.valid {
            let side = usize::from(port.human.team == 2);
            if let Some(index) = port.class_registry_ids[side]
                .iter()
                .position(|id| *id == usize::from(summary.class_id))
            {
                port.human.class_index = index;
            }
        }
        let listed = &summary.classes[..summary.class_count.min(summary.classes.len())];
        for side in 0..2 {
            let own_side = side + 1 == usize::from(port.human.team);
            open_classes(&mut port.rosters[side], &port.class_registry_ids[side], own_side, listed, &observed.class_room[side]);
            let points = observed.show_points[side] || observed.reinforcements[side].is_none();
            port.teams[side].reinforcements = match observed.reinforcements[side] {
                Some(count) if !points => count,
                _ => observed.score.teams[side] as i32,
            };
            port.mission.points[side] = points.then_some(observed.score.target.unwrap_or(0));
        }
        for (post, meta) in port.posts.iter_mut().zip(&hud.post_meta) {
            post.owner = hud.post_capture(&meta.name).map_or(meta.team, |row| row.0 as u8);
            post.eligible = meta.active && !observed.posts_hidden;
        }
        port.mission.deploy_wait = observed
            .wave
            .filter(|_| !observed.deploy_ready)
            .map(|due| due.saturating_sub(observed.tick) as f64 / 60.);
        port.mission.places = true;
        port.mission.unnamed_posts = true;
        let signature = (
            u32::try_from(observed.objectives.len()).unwrap_or(u32::MAX),
            observed.objectives.iter().fold(0u32, |a, o| a.wrapping_mul(31).wrapping_add(o.id)),
        );
        if signature != self.feed.objectives {
            self.feed.objectives = signature;
            for (side, facts) in port.teams.iter_mut().enumerate() {
                facts.objectives.clear();
                for o in observed.objectives.iter().filter(|o| o.team <= 0 || o.team == side as i32 + 1) {
                    let text = self.screens.catalog.localized(&o.key).unwrap_or(&o.key);
                    facts.objectives.push(text.trim().to_uppercase());
                }
            }
        }
    }

    /// Builds the teams, rosters and posts from the mission's admission.
    fn admit_launcher(&mut self, port: &mut LiveSpawnPort, observed: &Observation, hud: &Hud) {
        self.feed.admitted = true;
        self.prepare_team_labels(observed);
        port.mission.closed_note = self
            .screens
            .catalog
            .localized(FULL_LINE)
            .unwrap_or("")
            .trim()
            .to_uppercase();
        let labels = self.team_labels.clone().unwrap_or_default();
        for (side, admitted) in observed.admission.teams.iter().enumerate() {
            let roster = &mut port.rosters[side];
            let ids = &mut port.class_registry_ids[side];
            roster.clear();
            ids.clear();
            for class in admitted.classes.iter().take(MAX_CLASSES) {
                let meta = hud.class_meta.iter().find(|c| c.class_name == class.as_ref());
                let registry = meta.map_or(0, |m| m.registry_id);
                let name = u16::try_from(registry)
                    .ok()
                    .and_then(|id| self.names.class(id))
                    .or(meta.map(|m| m.display_name.as_str()))
                    .unwrap_or(class);
                roster.push(SpawnClassEntry {
                    class_name: class.to_string(),
                    display_name: name.to_owned(),
                    weapons: meta.map(|m| m.weapons.clone()).unwrap_or_default(),
                    eligible: true,
                });
                ids.push(registry);
            }
            let facts = &mut port.teams[side];
            facts.name.clone_from(&labels[side]);
            facts.logo.clone_from(&admitted.icon);
            if let Some(meta) = hud.class_meta.iter().find(|c| admitted.classes.first().is_some_and(|f| c.class_name == f.as_ref())) {
                facts.side.clone_from(&meta.side);
            }
        }
        // The 2005 game's spawn screen names none of these posts (its map's
        // legend reads only "Command Post"), and the prepared strings hold no
        // name for them: a post shows no name, never its mission label.
        port.posts.clear();
        port.post_ids.clear();
        for meta in &hud.post_meta {
            port.posts.push(SpawnPostEntry {
                name: meta.name.clone(),
                display_name: String::new(),
                position: meta.position,
                owner: meta.team,
                eligible: meta.active,
                vehicles: Vec::new(),
            });
            port.post_ids.push(meta.id);
        }
        port.post_vehicles.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_follows_the_players_list_on_the_players_team_only() {
        assert!(class_available(false, &[9], Some(3)));
        assert!(class_available(true, &[], None));
        assert!(class_available(true, &[3, 4], Some(4)));
        assert!(!class_available(true, &[3, 4], Some(5)));
        assert!(!class_available(true, &[3, 4], None));
    }

    #[test]
    fn the_published_list_reaches_every_row_of_the_roster() {
        let entry = |name: &str| SpawnClassEntry { class_name: name.into(), eligible: false, ..Default::default() };
        let mut roster = vec![entry("oak"), entry("fir"), entry("yew")];
        open_classes(&mut roster, &[11, 12, 13], true, &[13, 11], &[]);
        assert_eq!(roster.iter().map(|c| c.eligible).collect::<Vec<_>>(), [true, false, true]);
        open_classes(&mut roster, &[11, 12, 13], false, &[13], &[]);
        assert!(roster.iter().all(|c| c.eligible));
    }

    // The mission's room closes a class the player's list offers, on either
    // team, and never opens one the list does not; a row the room does not
    // name keeps what the list says.
    #[test]
    fn a_class_without_room_in_the_mission_is_not_offered() {
        let entry = |name: &str| SpawnClassEntry { class_name: name.into(), eligible: false, ..Default::default() };
        let mut roster = vec![entry("oak"), entry("fir"), entry("yew")];
        open_classes(&mut roster, &[11, 12, 13], true, &[11, 13], &[true, true, false]);
        assert_eq!(roster.iter().map(|c| c.eligible).collect::<Vec<_>>(), [true, false, false]);
        open_classes(&mut roster, &[11, 12, 13], false, &[], &[false, true]);
        assert_eq!(roster.iter().map(|c| c.eligible).collect::<Vec<_>>(), [false, true, true]);
    }

    #[test]
    fn the_phase_follows_the_body_and_the_summary() {
        let mut own = HudOwnSummary::default();
        assert_eq!(phase_of(true, &own), "alive");
        assert_eq!(phase_of(false, &own), "awaiting");
        own.valid = true;
        for (phase, name) in [(0, "unspawned"), (1, "alive"), (2, "dying"), (3, "awaiting")] {
            own.phase = phase;
            assert_eq!(phase_of(false, &own), name);
        }
        own.ended = true;
        assert_eq!(phase_of(false, &own), "ended");
    }
}
