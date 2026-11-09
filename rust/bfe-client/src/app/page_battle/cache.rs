//! Retain text geometry until its model facts change. Animation and the live
//! map keep updating without formatting labels or rebuilding UTF-16 each frame.
use super::super::battle::LiveSpawnPort;
use bfe_battle_ui::{
    scoreboard::{ClassicScoreboardView, ScoreboardModel, ScoreboardPort},
    spawn::{SpawnHitTarget, SpawnModel, SpawnState},
};
use bfe_render::interface::Batch;
use std::hash::{Hash, Hasher};
struct SpawnKey {
    team: u8,
    class: usize,
    post: Option<String>,
    tickets: [i32; 2],
    size: [i32; 2],
    theme: String,
    turned: bool,
    state: SpawnState,
    switch: bool,
    battle: bfe_shell::page::SpawnBattleStatus,
}
pub(in crate::app) struct Cache {
    pub panels: Batch,
    pub hits: Vec<([f64; 4], SpawnHitTarget)>,
    pub scoreboard: Batch,
    spawn: Option<SpawnKey>,
    score: u64,
}
impl Cache {
    pub fn new() -> Self {
        Self {
            panels: Batch::new(),
            hits: Vec::with_capacity(64),
            scoreboard: Batch::new(),
            spawn: None,
            score: 0,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_changed(
        &mut self,
        model: &SpawnModel,
        port: &LiveSpawnPort,
        size: [i32; 2],
        theme: &str,
        turned: bool,
        battle: bfe_shell::page::SpawnBattleStatus,
    ) -> bool {
        let tickets = [port.teams[0].reinforcements, port.teams[1].reinforcements];
        let switch = port.human.can_switch_teams;
        if self.spawn.as_ref().is_some_and(|old| {
            old.team == model.team
                && old.class == model.class_index
                && old.post == model.post
                && old.tickets == tickets
                && old.size == size
                && old.theme == theme
                && old.turned == turned
                && old.state == model.state
                && old.switch == switch
                && old.battle == battle
        }) {
            return false;
        }
        self.spawn = Some(SpawnKey {
            team: model.team,
            class: model.class_index,
            post: model.post.clone(),
            tickets,
            size,
            theme: theme.into(),
            turned,
            state: model.state,
            switch,
            battle,
        });
        true
    }
    #[allow(clippy::too_many_arguments)]
    pub fn score_changed(
        &mut self,
        model: &ScoreboardModel,
        view: &ClassicScoreboardView,
        port: &dyn ScoreboardPort,
        size: [i32; 2],
        theme: &str,
        hover: Option<usize>,
    ) -> bool {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        hover.hash(&mut hash);
        (
            model.state as u8,
            model.page as u8,
            model.mode as u8,
            model.reverse,
            model.team,
            model.side,
            model.selected,
            model.first,
            model.shown,
        )
            .hash(&mut hash);
        (size, theme, port.network(), port.can_restart()).hash(&mut hash);
        model.hover.map(|h| (h.side, h.index)).hash(&mut hash);
        for title in &model.titles {
            if let Some(title) = title {
                (&title.name, title.reinforcements).hash(&mut hash);
            }
        }
        for rows in &model.lists {
            rows.len().hash(&mut hash);
            for row in rows {
                (
                    row.slot, &row.name, row.kills, row.deaths, row.points, row.human, row.local,
                    row.ping,
                )
                    .hash(&mut hash);
            }
        }
        for rows in [&view.details_page.content, &view.awards_page.content] {
            for row in rows {
                (row.label, &row.value).hash(&mut hash);
            }
        }
        if let Some(subject) = &model.subject {
            (&subject.name, subject.side, subject.slot).hash(&mut hash);
        }
        let next = hash.finish();
        let changed = self.score != next;
        self.score = next;
        changed
    }
}
