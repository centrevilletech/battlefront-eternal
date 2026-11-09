//! Mission relationships and viewer facts for the shared HUD radar.
use super::receiver::Receiver;
use bfe_content_native::BattleContent;
use bfe_presentation::network_layout::net_record as R;
use bfe_render::hud_model::{
    self,
    layout::{HudFrame as H, HudTeam as T},
};
use bfe_sim::{
    battle::spawning::{self, Arg, Call},
    store::Strings,
};

pub struct Teams {
    relations: [[f64; 8]; 8],
    locals: [bool; 8],
}
impl Teams {
    pub fn new(content: &BattleContent) -> Self {
        // Use the simulation's mission parser so authored alliances and local
        // factions agree with the battle, including asymmetric relationships.
        let (_, calls) = bfe_sim::world::admit::rows::ai::mission_calls(&content.content.battle);
        let calls: Vec<_> = calls
            .iter()
            .map(|call| Call {
                name: call
                    .get("name")
                    .and_then(bfe_content::Value::as_str)
                    .unwrap_or("")
                    .into(),
                args: call
                    .get("args")
                    .and_then(bfe_content::Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .map(|v| match v {
                        bfe_content::Value::Null => Arg::Null,
                        bfe_content::Value::Bool(v) => Arg::Bool(*v),
                        bfe_content::Value::Number(v) => Arg::Num(*v),
                        bfe_content::Value::String(v) => Arg::Str(v.to_string()),
                        _ => Arg::Other,
                    })
                    .collect(),
                faction: call.get("faction").and_then(bfe_content::Value::as_f64),
                mask: call.get("mask").and_then(bfe_content::Value::as_f64),
            })
            .collect();
        let mut teams = spawning::make_teams(0.);
        spawning::apply_mission(&mut teams, &calls, &|_| false, &mut Strings::new());
        Self {
            relations: std::array::from_fn(|a| {
                std::array::from_fn(|b| teams[a].relations[b] as f64)
            }),
            locals: std::array::from_fn(|t| teams[t].faction == bfe_sim::battle::names::LOCALS),
        }
    }
    pub fn relation(&self, viewer: u8, team: usize) -> f64 {
        self.relations
            .get(viewer as usize)
            .and_then(|row| row.get(team))
            .copied()
            .unwrap_or(0.)
    }
    pub fn color_relation(&self, viewer: u8, team: usize) -> f64 {
        if team == 0 {
            0.
        } else if team != viewer as usize && self.locals.get(team).copied().unwrap_or(false) {
            3.
        } else {
            match self.relation(viewer, team) {
                v if v > 0. => 1.,
                v if v < 0. => 2.,
                _ => 0.,
            }
        }
    }
    pub fn publish(&self, input: &mut [f64], receiver: &Receiver, tickets: &[[f64; 4]; 8]) {
        // Marker handles use snapshot slots, not wire entity ids. Resolve the
        // viewer in that same namespace to exclude its duplicate troop arrow.
        let viewer = receiver.active_slots().find(|(_, row)| {
            row[R::ID] == receiver.own.id && row[R::GENERATION] == receiver.own.generation
        });
        input[H::VIEWER] = viewer.map_or(0., |(slot, _)| (slot + 1) as f64);
        input[H::VIEWER_TEAM] = receiver.own.team as f64;
        input[H::HAS_VIEWER] = viewer.is_some() as u8 as f64;
        for team in 0..hud_model::MAX_TEAMS {
            let at = hud_model::TEAMS + team * T::WORDS;
            input[at + T::RELATION] = self.relation(receiver.own.team, team);
            input[at + T::LOCALS] = self.locals.get(team).copied().unwrap_or(false) as u8 as f64;
            input[at + T::TICKETS] = tickets.get(team).map_or(f64::INFINITY, |row| row[0]);
            // Let RadarUnits count living soldiers from its current unit bank.
            input[at + T::LIVING] = f64::NAN;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewer_teams_preserve_alliances_locals_and_ticket_visibility() {
        let mut relations = [[-1.; 8]; 8];
        for (team, row) in relations.iter_mut().enumerate() {
            row[0] = 0.;
            row[team] = 1.;
        }
        relations[0].fill(0.);
        relations[2][3] = 1.;
        let teams = Teams {
            relations,
            locals: [false, false, false, true, false, false, false, false],
        };
        assert_eq!(teams.color_relation(2, 0), 0.);
        assert_eq!(teams.color_relation(2, 1), 2.);
        assert_eq!(teams.color_relation(2, 2), 1.);
        assert_eq!(teams.color_relation(2, 3), 3.);
        assert_eq!(teams.relation(2, 3), 1.);
        assert_eq!(teams.color_relation(3, 3), 1.);
        let receiver = Receiver::new(serde_json::Value::Null);
        let mut input = vec![0.; hud_model::INPUT_LEN];
        teams.publish(&mut input, &receiver, &[[20., 30., 0., 0.]; 8]);
        assert_eq!(input[hud_model::TEAMS + T::WORDS + T::TICKETS], 20.);
        assert!(input[hud_model::TEAMS + T::WORDS + T::LIVING].is_nan());
    }
}
