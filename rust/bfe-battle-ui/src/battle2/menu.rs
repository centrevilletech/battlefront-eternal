//! The 2005 battle's own boards: the roster held over play and the result at the
//! end. Deploying is the spawn launcher's (`crate::spawn`), shared by both games.
use super::{BattleView, Phase};
use crate::ScoreboardPort;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Hit {
    #[default]
    None,
    Done,
    Earlier,
    Later,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Roster(bool),
    Previous,
    Next,
    Confirm,
    Back,
    Hit(Hit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    None,
    Done,
}

impl Response {
    pub fn send(self, score: &mut dyn ScoreboardPort) {
        if self == Self::Done && score.can_restart() {
            score.restart();
        }
    }
}

#[derive(Default)]
pub struct Menu {
    pub board: bool,
    pub board_offset: usize,
    pub(super) board_rows: usize,
}

impl Menu {
    pub(super) fn last_offset(&self, v: &BattleView<'_>) -> usize {
        v.teams
            .iter()
            .map(|t| v.scores.iter().filter(|r| r.team == t.id).count())
            .max()
            .unwrap_or(0)
            .saturating_sub(self.board_rows.max(1))
    }

    pub fn act(&mut self, v: &BattleView<'_>, action: Action) -> Response {
        if let Action::Roster(held) = action {
            self.board = held && matches!(v.phase, Phase::Playing);
            self.board_offset = 0;
            return Response::None;
        }
        if (v.phase == Phase::Playing && self.board) || matches!(v.phase, Phase::Ended(_)) {
            match action {
                Action::Next | Action::Hit(Hit::Later) => {
                    self.board_offset = self.board_offset.saturating_add(1).min(self.last_offset(v))
                }
                Action::Previous | Action::Hit(Hit::Earlier) => {
                    self.board_offset = self.board_offset.saturating_sub(1)
                }
                Action::Back => self.board = false,
                Action::Confirm | Action::Hit(Hit::Done) => {
                    self.board = false;
                    if matches!(v.phase, Phase::Ended(_)) {
                        return Response::Done;
                    }
                }
                _ => {}
            }
        }
        Response::None
    }
}
