//! Slint model synchronization and action routing for page-look battle screens:
//! spawn screen, scoreboard, and pause menu.

use super::{
    battle::BattleController,
    settings::AppSettingsStore,
    slint_ui::{
        BattleClassModel, BattleModel, BattlePauseActionModel, BattlePinModel,
        BattleScorePlayerModel, BattleStatLineModel, BattleTeamModel, GameUi, ListEntry,
        Theme,
    },
};
use bfe_battle_ui::{
    pause::{PauseHitTarget, PauseHotspot, PauseScreenState},
    scoreboard::{BoardPage, BoardState, ScoreboardAction, ScoreboardHitTarget, ScoreboardPort, SortMode},
    shapes::HudBatchSurface,
    spawn::{SpawnHitTarget, SpawnPort, TeamSwitchSource},
};
use bfe_shell::page::SpawnBattleStatus;
use super::slint_models::sync_rows;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

/// The lists the battle screens repeat over. They live as long as the window does and are handed to the
/// `BattleModel` global once; each sync brings them up to date row by row (slint_models.rs), so the rows
/// keep their hover, held press and double-click state.
pub(super) struct BattleLists {
    classes: Rc<VecModel<BattleClassModel>>,
    objectives: Rc<VecModel<SharedString>>,
    pins: Rc<VecModel<BattlePinModel>>,
    team1_rows: Rc<VecModel<BattleScorePlayerModel>>,
    team2_rows: Rc<VecModel<BattleScorePlayerModel>>,
    detail_lines: Rc<VecModel<BattleStatLineModel>>,
    award_lines: Rc<VecModel<BattleStatLineModel>>,
    pause_rows: Rc<VecModel<BattlePauseActionModel>>,
    pause_players: Rc<VecModel<ListEntry>>,
    // A player row's cells are a model of their own, so the rows are rebuilt only when the names change.
    player_names: RefCell<Vec<String>>,
}

impl BattleLists {
    pub(super) fn new(root: &GameUi) -> Self {
        let bm = root.global::<BattleModel>();
        let lists = Self {
            classes: Rc::default(),
            objectives: Rc::default(),
            pins: Rc::default(),
            team1_rows: Rc::default(),
            team2_rows: Rc::default(),
            detail_lines: Rc::default(),
            award_lines: Rc::default(),
            pause_rows: Rc::default(),
            pause_players: Rc::default(),
            player_names: RefCell::default(),
        };
        bm.set_classes(ModelRc::from(lists.classes.clone()));
        bm.set_objectives(ModelRc::from(lists.objectives.clone()));
        bm.set_pins(ModelRc::from(lists.pins.clone()));
        bm.set_score_team1_rows(ModelRc::from(lists.team1_rows.clone()));
        bm.set_score_team2_rows(ModelRc::from(lists.team2_rows.clone()));
        bm.set_score_detail_lines(ModelRc::from(lists.detail_lines.clone()));
        bm.set_score_award_lines(ModelRc::from(lists.award_lines.clone()));
        bm.set_pause_main_rows(ModelRc::from(lists.pause_rows.clone()));
        bm.set_pause_players(ModelRc::from(lists.pause_players.clone()));
        lists
    }
}

#[derive(Clone, Debug)]
pub(super) enum BattleUiAction {
    SpawnSelectTeam(u8),
    SpawnToggleTeam,
    SpawnSelectClass(usize),
    SpawnSelectPin(String),
    SpawnClicked,
    SpawnFigureDown(f32, f32),
    SpawnFigureMove(f32, f32),
    SpawnFigureUp,
    ScoreSelectPage(String),
    ScoreSortClicked(String),
    ScoreRowClicked(i32, i32),
    ScoreRowDoubleClicked(i32, i32),
    ScoreTeamScrolled(i32, i32),
    ScoreLeftFootClicked,
    ScoreMidFootClicked,
    ScoreRightFootClicked,
    PauseRowHovered(usize),
    PauseRowActivated(usize),
    PauseOptionHovered(usize),
    PauseOptionActivated(usize),
    PausePlayerSelected(usize),
    PauseBackHovered,
    PauseBackActivated,
    PauseConfirmHovered(usize),
    PauseConfirmAccepted,
    PauseConfirmCancelled,
    PauseThemeChanged(usize),
}

pub(super) fn register_battle_callbacks(
    root: &GameUi,
    actions: &Rc<RefCell<Vec<BattleUiAction>>>,
) {
    let bm = root.global::<BattleModel>();
    let push = |a: &Rc<RefCell<Vec<BattleUiAction>>>, act: BattleUiAction| {
        a.borrow_mut().push(act);
    };

    let q = Rc::clone(actions);
    bm.on_select_team(move |team| push(&q, BattleUiAction::SpawnSelectTeam(team.clamp(1, 2) as u8)));
    let q = Rc::clone(actions);
    bm.on_toggle_team(move || push(&q, BattleUiAction::SpawnToggleTeam));
    let q = Rc::clone(actions);
    bm.on_select_class(move |idx| push(&q, BattleUiAction::SpawnSelectClass(idx.max(0) as usize)));
    let q = Rc::clone(actions);
    bm.on_select_pin(move |id| push(&q, BattleUiAction::SpawnSelectPin(id.into())));
    let q = Rc::clone(actions);
    bm.on_spawn_clicked(move || push(&q, BattleUiAction::SpawnClicked));
    let q = Rc::clone(actions);
    bm.on_figure_pointer_down(move |x, y| push(&q, BattleUiAction::SpawnFigureDown(x, y)));
    let q = Rc::clone(actions);
    bm.on_figure_pointer_move(move |x, y| push(&q, BattleUiAction::SpawnFigureMove(x, y)));
    let q = Rc::clone(actions);
    bm.on_figure_pointer_up(move || push(&q, BattleUiAction::SpawnFigureUp));

    let q = Rc::clone(actions);
    bm.on_score_select_page(move |p| push(&q, BattleUiAction::ScoreSelectPage(p.into())));
    let q = Rc::clone(actions);
    bm.on_score_sort_clicked(move |k| push(&q, BattleUiAction::ScoreSortClicked(k.into())));
    let q = Rc::clone(actions);
    bm.on_score_row_clicked(move |slot, tid| push(&q, BattleUiAction::ScoreRowClicked(slot, tid)));
    let q = Rc::clone(actions);
    bm.on_score_row_double_clicked(move |slot, tid| {
        push(&q, BattleUiAction::ScoreRowDoubleClicked(slot, tid));
    });
    let q = Rc::clone(actions);
    bm.on_score_team_scrolled(move |tid, dir| {
        push(&q, BattleUiAction::ScoreTeamScrolled(tid, dir));
    });
    let q = Rc::clone(actions);
    bm.on_score_left_foot_clicked(move || push(&q, BattleUiAction::ScoreLeftFootClicked));
    let q = Rc::clone(actions);
    bm.on_score_mid_foot_clicked(move || push(&q, BattleUiAction::ScoreMidFootClicked));
    let q = Rc::clone(actions);
    bm.on_score_right_foot_clicked(move || push(&q, BattleUiAction::ScoreRightFootClicked));

    let q = Rc::clone(actions);
    bm.on_pause_row_hovered(move |idx| {
        push(&q, BattleUiAction::PauseRowHovered(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_row_activated(move |idx| {
        push(&q, BattleUiAction::PauseRowActivated(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_option_hovered(move |idx| {
        push(&q, BattleUiAction::PauseOptionHovered(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_option_activated(move |idx| {
        push(&q, BattleUiAction::PauseOptionActivated(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_player_selected(move |idx| {
        push(&q, BattleUiAction::PausePlayerSelected(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_back_hovered(move || push(&q, BattleUiAction::PauseBackHovered));
    let q = Rc::clone(actions);
    bm.on_pause_back_activated(move || push(&q, BattleUiAction::PauseBackActivated));
    let q = Rc::clone(actions);
    bm.on_pause_confirm_hovered(move |idx| {
        push(&q, BattleUiAction::PauseConfirmHovered(idx.max(0) as usize));
    });
    let q = Rc::clone(actions);
    bm.on_pause_confirm_accepted(move || push(&q, BattleUiAction::PauseConfirmAccepted));
    let q = Rc::clone(actions);
    bm.on_pause_confirm_cancelled(move || push(&q, BattleUiAction::PauseConfirmCancelled));
    let q = Rc::clone(actions);
    bm.on_pause_theme_changed(move |idx| {
        push(&q, BattleUiAction::PauseThemeChanged(idx.max(0) as usize));
    });
}

pub(super) fn battle_overlay_visible(root: &GameUi) -> bool {
    let bm = root.global::<BattleModel>();
    bm.get_pause_open()
        || bm.get_waiting_prompt_visible()
        || (bm.get_spawn_visible() && bm.get_page_spawn_look())
        || (bm.get_scoreboard_visible() && bm.get_page_score_look())
}

pub(super) fn battle_modal_open(root: &GameUi) -> bool {
    let bm = root.global::<BattleModel>();
    let options = root.global::<super::slint_ui::OptionsModel>();
    bm.get_pause_open()
        && (bm.get_pause_subscreen() == "confirm"
            || (options.get_battle_open() && options.get_modal_open()))
}

pub(super) fn spawn_battle_status(battle: &BattleController) -> SpawnBattleStatus {
    battle.scene.online.as_ref().map_or_else(
        SpawnBattleStatus::default,
        |r| SpawnBattleStatus {
            countdown: r
                .hud
                .countdown
                .is_finite()
                .then_some((r.hud.countdown.floor().max(0.) as u32, r.hud.losing)),
            start: [r.hud.teams[1][1], r.hud.teams[2][1]],
            bleeding: [r.hud.teams[1][3] > 0., r.hud.teams[2][3] > 0.],
        },
    )
}

pub(super) fn sync_battle(root: &GameUi, lists: &BattleLists, battle: &mut BattleController) {
    let (look_idx, look_str) = match battle.pause_page.theme.as_str() {
        "bf1" => (0, "bf1"),
        "bf2" => (1, "bf2"),
        _ => (2, "eternal"),
    };
    root.global::<Theme>().set_menu_theme(look_str.into());

    let bm = root.global::<BattleModel>();
    bm.set_theme_index(look_idx);

    let page_spawn = battle.page_battle.spawn;
    let page_score = battle.page_battle.scoreboard;
    let pause_open = battle.pause_menu.is_open;
    let spawn_visible = page_spawn && battle.spawn_ctrl.is_open() && !pause_open;
    let scoreboard_visible = page_score && battle.scoreboard_ctrl.model.is_open() && !pause_open;
    let waiting_prompt_visible = !pause_open
        && battle
            .pause_menu
            .waiting_time
            .is_some_and(|t| t >= bfe_battle_ui::pause::PROMPT_DELAY);
    let waiting_prompt_opacity = (0.5 + 0.5 * battle.pause_menu.pulse()) as f32;

    bm.set_page_spawn_look(page_spawn);
    bm.set_page_score_look(page_score);
    bm.set_spawn_visible(spawn_visible);
    bm.set_scoreboard_visible(scoreboard_visible);
    bm.set_pause_open(pause_open);
    bm.set_waiting_prompt_visible(waiting_prompt_visible);
    bm.set_waiting_prompt_opacity(waiting_prompt_opacity);
    if !pause_open && bm.get_theme_picker_open() {
        bm.set_theme_picker_open(false);
    }

    if spawn_visible {
        sync_spawn_model(&bm, lists, battle);
    }
    if scoreboard_visible {
        sync_scoreboard_model(&bm, lists, battle);
    }
    if pause_open {
        sync_pause_model(&bm, lists, battle);
    }
}

fn sync_spawn_model(bm: &BattleModel<'_>, lists: &BattleLists, battle: &BattleController) {
    let status = spawn_battle_status(battle);
    let layout = battle.page_battle.layout;
    let w = layout.width;
    let h = layout.height;
    let model = &battle.spawn_ctrl.model;
    let port = &battle.spawn_port;
    let can_switch = model.can_switch_teams(port);

    let build_team = |side: usize| -> BattleTeamModel {
        let team_id = (side + 1) as u8;
        let own = team_id == model.team;
        let interactive = !own && can_switch;
        let bleeding = status.bleeding[side];
        let count = port.teams[side].reinforcements.max(0);
        let start = status.start[side].max(count.max(1) as f64);
        let fill_ratio = (count as f64 / start).clamp(0., 1.) as f32;
        let (badge_mode, badge_text) = if own {
            ("own", "YOUR TEAM")
        } else if can_switch {
            ("switch", "SWITCH TEAM")
        } else {
            ("auto", "TEAMS: AUTO ASSIGN")
        };
        let plate = layout.teams[side];
        let emblem = (h * 0.074).clamp(46., 80.);
        let gap = w * 0.007;
        let logo_x = if side == 0 {
            plate[0] - gap - emblem * 0.5
        } else {
            plate[2] + gap + emblem * 0.5
        };
        let logo_y = (plate[1] + plate[3]) * 0.5;
        BattleTeamModel {
            id: team_id as i32,
            name: port.teams[side].name.to_uppercase().into(),
            tickets: count.to_string().into(),
            fill_ratio,
            own,
            interactive,
            bleeding,
            badge_mode: badge_mode.into(),
            badge_text: badge_text.into(),
            plate_x: plate[0] as f32,
            plate_y: plate[1] as f32,
            plate_w: (plate[2] - plate[0]) as f32,
            plate_h: (plate[3] - plate[1]) as f32,
            logo_x: logo_x as f32,
            logo_y: logo_y as f32,
            logo_size: emblem as f32,
        }
    };

    bm.set_team_one(build_team(0));
    bm.set_team_two(build_team(1));
    bm.set_can_switch_teams(can_switch);
    bm.set_swap_size((h * 0.046).clamp(32., 46.) as f32);
    bm.set_swap_x((w * 0.5) as f32);
    bm.set_swap_y(((layout.teams[0][1] + layout.teams[0][3]) * 0.5) as f32);
    bm.set_switch_hint_y((layout.teams[0][3] + h * 0.005) as f32);

    let pulse_alpha = if ((battle.page_battle.age * 2.0) as u64).is_multiple_of(2) {
        1.0
    } else {
        0.35
    };
    bm.set_pulse_alpha(pulse_alpha);

    if let Some((seconds, losing)) = status.countdown {
        let clock = if seconds >= 60 {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        } else {
            seconds.to_string()
        };
        let label = if losing { "DEFEAT IN" } else { "VICTORY IN" };
        bm.set_countdown_visible(true);
        bm.set_countdown_text(format!("{label} {clock}").into());
        bm.set_countdown_winning(!losing);
        bm.set_countdown_y((layout.roster[1] - h * 0.024) as f32);
    } else {
        bm.set_countdown_visible(false);
        bm.set_countdown_text(SharedString::default());
    }

    bm.set_roster_x(layout.roster[0] as f32);
    bm.set_roster_y(layout.roster[1] as f32);
    bm.set_roster_w((layout.roster[2] - layout.roster[0]) as f32);

    let roster = model.roster(port);
    let classes: Vec<BattleClassModel> = roster
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let picked = i == model.class_index;
            let r = layout.class_rect(i, picked);
            let mut w_slots = [
                SharedString::default(),
                SharedString::default(),
                SharedString::default(),
                SharedString::default(),
            ];
            for (wi, w_name) in entry.weapons.iter().take(4).enumerate() {
                w_slots[wi] = w_name.to_uppercase().into();
            }
            BattleClassModel {
                index: i as i32,
                name: entry.display_name.to_uppercase().into(),
                picked,
                w0: w_slots[0].clone(),
                w1: w_slots[1].clone(),
                w2: w_slots[2].clone(),
                w3: w_slots[3].clone(),
                rect_x: r[0] as f32,
                rect_y: r[1] as f32,
                rect_w: (r[2] - r[0]) as f32,
                rect_h: (r[3] - r[1]) as f32,
            }
        })
        .collect();
    sync_rows(&lists.classes, &classes);

    bm.set_figure_x(layout.figure[0] as f32);
    bm.set_figure_y(layout.figure[1] as f32);
    bm.set_figure_w((layout.figure[2] - layout.figure[0]) as f32);
    bm.set_figure_h((layout.figure[3] - layout.figure[1]) as f32);
    bm.set_figure_dragging(battle.page_battle.dragging());
    bm.set_figure_turned(battle.page_battle.turned());
    bm.set_title_y((layout.figure[3] + h * 0.008) as f32);
    // The caption runs the width of the stage column (the grid's middle track), not just the figure's.
    let column_gap = w * 0.016;
    let stage_x = layout.roster[2] + column_gap;
    bm.set_title_x(stage_x as f32);
    bm.set_title_w((layout.objectives[0] - column_gap - stage_x) as f32);

    let selected_title = roster
        .get(model.class_index)
        .map(|e| e.display_name.to_uppercase())
        .unwrap_or_default();
    bm.set_selected_class_title(selected_title.into());

    let posts = port.posts(model.team);
    if let Some(post) = posts
        .iter()
        .find(|p| model.post.as_deref() == Some(p.name.as_str()))
    {
        let post_label = if post.display_name.trim().is_empty() {
            post.name.to_uppercase()
        } else {
            post.display_name.to_uppercase()
        };
        let vehicles = if post.vehicles.is_empty() {
            "NONE".to_string()
        } else {
            post.vehicles.join(", ").to_uppercase()
        };
        bm.set_selected_post_name(post_label.into());
        bm.set_selected_vehicles(vehicles.into());
    } else {
        bm.set_selected_post_name(SharedString::default());
        bm.set_selected_vehicles("NONE".into());
    }

    let can_spawn = model.post.is_some() && !roster.is_empty();
    bm.set_spawn_x(layout.spawn[0] as f32);
    bm.set_spawn_y(layout.spawn[1] as f32);
    bm.set_spawn_w((layout.spawn[2] - layout.spawn[0]) as f32);
    bm.set_spawn_h((layout.spawn[3] - layout.spawn[1]) as f32);
    bm.set_spawn_label("SPAWN".into());
    bm.set_can_spawn(can_spawn);

    bm.set_objectives_x(layout.objectives[0] as f32);
    bm.set_objectives_y(layout.objectives[1] as f32);
    bm.set_objectives_w((layout.objectives[2] - layout.objectives[0]) as f32);
    bm.set_objectives_h((layout.objectives[3] - layout.objectives[1]) as f32);
    let obj_lines: Vec<SharedString> = port
        .team_facts(model.team)
        .map(|f| {
            f.objectives
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| SharedString::from(s.to_uppercase()))
                .collect()
        })
        .unwrap_or_default();
    sync_rows(&lists.objectives, &obj_lines);

    bm.set_map_x(layout.map[0] as f32);
    bm.set_map_y(layout.map[1] as f32);
    bm.set_map_w((layout.map[2] - layout.map[0]) as f32);
    bm.set_map_h((layout.map[3] - layout.map[1]) as f32);

    let pins: Vec<BattlePinModel> = posts
        .iter()
        .filter(|p| p.owner == model.team && p.eligible)
        .map(|p| {
            let pt = battle
                .page_battle
                .pin_point(p, battle.spawn_ctrl.view.map_proj);
            BattlePinModel {
                id: p.name.as_str().into(),
                x: pt[0] as f32,
                y: pt[1] as f32,
                selected: model.post.as_deref() == Some(p.name.as_str()),
            }
        })
        .collect();
    sync_rows(&lists.pins, &pins);
}

fn sync_scoreboard_model(bm: &BattleModel<'_>, lists: &BattleLists, battle: &mut BattleController) {
    let h = battle.page_size[1].max(1) as f64;
    let shown = ((h * 0.90 - (h * 0.122 + 51. + h * 0.006 + h * 0.047 + (h * 0.038).clamp(28., 40.) + 6.))
        / ((h * 0.034).clamp(26., 38.) + 2.))
        .floor()
        .max(1.) as usize;
    battle.scoreboard_ctrl.model.shown = shown;
    battle.scoreboard_ctrl.model.clamp();

    let model = &battle.scoreboard_ctrl.model;
    let port = &battle.scoreboard_port;

    let page_str = match model.page {
        BoardPage::Table => "table",
        BoardPage::Details => "details",
        BoardPage::Awards => "awards",
    };
    bm.set_score_page(page_str.into());
    bm.set_score_is_end(model.state == BoardState::End);
    let show_ping = port.network() && model.state == BoardState::Peek;
    bm.set_score_show_ping(show_ping);

    let sort_key = match model.mode {
        SortMode::Kills => "kills",
        SortMode::Deaths => "deaths",
        SortMode::Points => "cp",
        SortMode::Name => "name",
    };
    bm.set_score_sort_key(sort_key.into());
    bm.set_score_sort_asc((model.mode == SortMode::Deaths) != model.reverse);

    let own_team = model.team;
    let tone_for = |team_num: i32| -> i32 {
        let rel = port.relation(own_team, team_num);
        if rel > 0 {
            1
        } else if rel < 0 {
            2
        } else {
            0
        }
    };

    let build_team_rows = |side: usize| -> Vec<BattleScorePlayerModel> {
        let list = &model.lists[side - 1];
        list.iter()
            .enumerate()
            .skip(model.first)
            .take(shown)
            .map(|(idx, row)| {
                let (ping_text, ping_bars, ping_tone) = match row.ping {
                    Some(ms) if show_ping => {
                        let bars = if ms < 60 {
                            4
                        } else if ms < 100 {
                            3
                        } else if ms < 160 {
                            2
                        } else {
                            1
                        };
                        let tone = if ms < 100 {
                            1
                        } else if ms < 160 {
                            2
                        } else {
                            3
                        };
                        (ms.to_string(), bars, tone)
                    }
                    _ => (String::new(), 0, 0),
                };
                BattleScorePlayerModel {
                    slot: idx as i32,
                    team_id: side as i32,
                    rank: (idx + 1).to_string().into(),
                    name: row.name.to_uppercase().into(),
                    kills: row.kills.to_string().into(),
                    deaths: row.deaths.to_string().into(),
                    cp: row.points.to_string().into(),
                    ping_text: ping_text.into(),
                    ping_bars,
                    ping_tone,
                    local: row.local,
                    human: row.human && !row.local,
                    selected: model.side == side && model.selected == idx,
                }
            })
            .collect()
    };

    let t1 = model.titles[0].as_ref();
    let t2 = model.titles[1].as_ref();
    bm.set_score_team1_name(t1.map_or("REPUBLIC", |t| t.name.as_str()).to_uppercase().into());
    bm.set_score_team1_tickets(t1.map_or(0, |t| t.reinforcements.max(0)).to_string().into());
    bm.set_score_team1_tone(tone_for(1));
    sync_rows(&lists.team1_rows, &build_team_rows(1));
    bm.set_score_team1_scroll(model.first as i32);
    bm.set_score_team1_total(model.length() as i32);
    bm.set_score_team1_visible(shown as i32);

    bm.set_score_team2_name(t2.map_or("CIS", |t| t.name.as_str()).to_uppercase().into());
    bm.set_score_team2_tickets(t2.map_or(0, |t| t.reinforcements.max(0)).to_string().into());
    bm.set_score_team2_tone(tone_for(2));
    sync_rows(&lists.team2_rows, &build_team_rows(2));
    bm.set_score_team2_scroll(model.first as i32);
    bm.set_score_team2_total(model.length() as i32);
    bm.set_score_team2_visible(shown as i32);

    if model.page == BoardPage::Details {
        if let Some(subject) = &model.subject {
            let side_idx = if subject.side == 2 { 1 } else { 0 };
            let row = model.lists[side_idx]
                .iter()
                .find(|r| r.slot == subject.slot);
            let team_name = model.titles[side_idx]
                .as_ref()
                .map_or("", |t| t.name.as_str())
                .to_uppercase();
            bm.set_score_subject_name(subject.name.to_uppercase().into());
            bm.set_score_subject_team(team_name.into());
            bm.set_score_subject_tone(tone_for(subject.side as i32));
            bm.set_score_subject_kills(row.map_or(0, |r| r.kills).to_string().into());
            bm.set_score_subject_deaths(row.map_or(0, |r| r.deaths).to_string().into());
            bm.set_score_subject_cp(row.map_or(0, |r| r.points).to_string().into());

            let personal = port.personal(subject.slot);
            let detail_lines: Vec<BattleStatLineModel> = personal
                .into_iter()
                .map(|entry| {
                    let trimmed = entry.value.trim();
                    let empty = trimmed.is_empty();
                    BattleStatLineModel {
                        label: entry.label.to_uppercase().into(),
                        value: if empty {
                            "-".into()
                        } else {
                            trimmed.to_uppercase().into()
                        },
                        empty,
                        tag: SharedString::default(),
                        tag_tone: 0,
                    }
                })
                .collect();
            sync_rows(&lists.detail_lines, &detail_lines);
        }
    } else if model.page == BoardPage::Awards {
        let awards = port.awards();
        let award_lines: Vec<BattleStatLineModel> = awards
            .into_iter()
            .map(|award| {
                let trimmed = award.text.trim();
                let empty = trimmed.is_empty();
                let (tag, tag_tone) = match award.team {
                    Some(t) if !empty => {
                        let name = model.titles[usize::from(t == 2)]
                            .as_ref()
                            .map_or("", |th| th.name.as_str())
                            .to_uppercase();
                        (SharedString::from(name), tone_for(t))
                    }
                    _ => (SharedString::default(), 0),
                };
                BattleStatLineModel {
                    label: award.label.to_uppercase().into(),
                    value: if empty {
                        "-".into()
                    } else {
                        trimmed.to_uppercase().into()
                    },
                    empty,
                    tag,
                    tag_tone,
                }
            })
            .collect();
        sync_rows(&lists.award_lines, &award_lines);
    }

    let is_end = model.state == BoardState::End;
    let is_table = model.page == BoardPage::Table;
    let done = is_end && is_table;
    bm.set_score_show_left_foot(!done || port.can_restart());
    bm.set_score_left_foot_label(if done { "DONE" } else { "BACK" }.into());
    bm.set_score_show_mid_foot(is_table);
    bm.set_score_show_right_foot(is_end && is_table);
}

fn sync_pause_model(bm: &BattleModel<'_>, lists: &BattleLists, battle: &mut BattleController) {
    let Some(replicas) = &battle.scene.online else {
        return;
    };
    let surface = HudBatchSurface {
        height: battle.height,
        aspect: battle.aspect,
        batch: &mut battle.scene.bridge.hud_model.batch,
        fonts: &battle.scene.bridge.hud_model.fonts,
        assets: &battle.scene.bridge.hud_model.assets,
        textures: Some(&replicas.hud.names),
        strings: Some(&replicas.hud.strings),
    };
    let rows = battle.pause_menu.visible_rows(&surface);
    let main_rows: Vec<BattlePauseActionModel> = rows
        .iter()
        .enumerate()
        .map(|(i, (id, label))| BattlePauseActionModel {
            id: format!("{id:?}").into(),
            label: label.to_uppercase().into(),
            enabled: true,
            highlighted: i == battle.pause_menu.focused_row && !battle.pause_menu.back_focused,
        })
        .collect();
    sync_rows(&lists.pause_rows, &main_rows);
    bm.set_pause_back_focused(battle.pause_menu.back_focused);

    match battle.pause_menu.top() {
        Some(PauseScreenState::Main) | None => {
            bm.set_pause_subscreen("main".into());
        }
        Some(PauseScreenState::Options { focused_row }) => {
            bm.set_pause_subscreen("options".into());
            bm.set_pause_option_focused(*focused_row as i32);
        }
        Some(PauseScreenState::PlayerList { selected, .. }) => {
            bm.set_pause_subscreen("playerlist".into());
            bm.set_pause_selected_player(*selected as i32);
            let names: Vec<String> = battle
                .pause_menu
                .players
                .iter()
                .map(|p| p.name.clone())
                .collect();
            if *lists.player_names.borrow() != names {
                let players: Vec<ListEntry> = names
                    .iter()
                    .map(|name| ListEntry {
                        badge: SharedString::default(),
                        cells: ModelRc::new(VecModel::from(vec![SharedString::from(name.as_str())])),
                        disabled: false,
                    })
                    .collect();
                lists.pause_players.set_vec(players);
                *lists.player_names.borrow_mut() = names;
            }
        }
        Some(PauseScreenState::Confirm {
            message,
            focused_button,
            ..
        }) => {
            bm.set_pause_subscreen("confirm".into());
            bm.set_pause_confirm_message(message.as_str().into());
            bm.set_pause_confirm_focused_button(*focused_button as i32);
        }
    }
}

pub(super) fn drain_battle_actions(
    actions: &Rc<RefCell<Vec<BattleUiAction>>>,
    battle: &mut BattleController,
    settings: &mut AppSettingsStore,
) {
    let drained: Vec<BattleUiAction> = actions.borrow_mut().drain(..).collect();
    if drained.is_empty() {
        return;
    }
    for act in drained {
        match act {
            BattleUiAction::SpawnSelectTeam(team) => {
                if battle.spawn_ctrl.model.team != team
                    && battle.spawn_ctrl.model.can_switch_teams(&battle.spawn_port)
                {
                    battle.spawn_ctrl.model.switch_team(
                        &battle.spawn_port,
                        TeamSwitchSource::Mouse,
                        super::battle::unit_random(),
                    );
                    battle.spawn_ctrl.sync_model_events();
                }
            }
            BattleUiAction::SpawnToggleTeam => {
                if battle.spawn_ctrl.model.can_switch_teams(&battle.spawn_port) {
                    battle.spawn_ctrl.model.switch_team(
                        &battle.spawn_port,
                        TeamSwitchSource::Mouse,
                        super::battle::unit_random(),
                    );
                    battle.spawn_ctrl.sync_model_events();
                }
            }
            BattleUiAction::SpawnSelectClass(idx) => {
                if battle.spawn_ctrl.model.select_class(&battle.spawn_port, idx) {
                    battle.spawn_ctrl.sync_model_events();
                }
            }
            BattleUiAction::SpawnSelectPin(post_name) => {
                if battle
                    .spawn_ctrl
                    .model
                    .select_post(&battle.spawn_port, &post_name)
                {
                    battle.spawn_ctrl.sync_model_events();
                }
            }
            BattleUiAction::SpawnClicked => {
                battle.spawn_ctrl.spawn(&mut battle.spawn_port);
                battle.flush_spawn_requests();
            }
            BattleUiAction::SpawnFigureDown(x, y) => {
                battle.page_battle.drag_start([x as f64, y as f64]);
            }
            BattleUiAction::SpawnFigureMove(x, y) => {
                battle.page_battle.drag_move([x as f64, y as f64]);
            }
            BattleUiAction::SpawnFigureUp => {
                battle.page_battle.drag_end();
            }
            BattleUiAction::ScoreSelectPage(page) => {
                let target = match page.as_str() {
                    "table" => Some(ScoreboardAction::Back),
                    "details" => Some(ScoreboardAction::Details),
                    "awards" => Some(ScoreboardAction::Awards),
                    _ => None,
                };
                if let Some(target) = target {
                    let current = battle.scoreboard_ctrl.model.page;
                    let valid = match target {
                        ScoreboardAction::Back => current != BoardPage::Table,
                        ScoreboardAction::Details => current != BoardPage::Details,
                        ScoreboardAction::Awards => current != BoardPage::Awards,
                        ScoreboardAction::Done => false,
                    };
                    if valid {
                        battle.scoreboard_ctrl.act(
                            target,
                            &mut battle.scoreboard_port,
                            battle.height,
                        );
                    }
                }
            }
            BattleUiAction::ScoreSortClicked(key) => {
                let mode = match key.as_str() {
                    "kills" => Some(SortMode::Kills),
                    "deaths" => Some(SortMode::Deaths),
                    "cp" => Some(SortMode::Points),
                    "name" => Some(SortMode::Name),
                    _ => None,
                };
                if let Some(mode) = mode {
                    if battle
                        .scoreboard_ctrl
                        .model
                        .sort_by(mode, &battle.scoreboard_port)
                    {
                        battle
                            .scoreboard_ctrl
                            .sounds
                            .push("shell_select_change");
                    }
                }
            }
            BattleUiAction::ScoreRowClicked(slot, tid) => {
                let side = if tid == 2 { 2 } else { 1 };
                let idx = slot.max(0) as usize;
                if battle.scoreboard_ctrl.model.select(side, idx) {
                    battle
                        .scoreboard_ctrl
                        .sounds
                        .push("shell_select_change");
                }
            }
            BattleUiAction::ScoreRowDoubleClicked(slot, tid) => {
                let side = if tid == 2 { 2 } else { 1 };
                let idx = slot.max(0) as usize;
                let _ = battle.scoreboard_ctrl.model.select(side, idx);
                battle.scoreboard_ctrl.act(
                    ScoreboardAction::Details,
                    &mut battle.scoreboard_port,
                    battle.height,
                );
            }
            BattleUiAction::ScoreTeamScrolled(_tid, dir) => {
                if battle.scoreboard_ctrl.model.scroll(dir as isize) {
                    battle
                        .scoreboard_ctrl
                        .sounds
                        .push("shell_select_change");
                }
            }
            BattleUiAction::ScoreLeftFootClicked => {
                let done = battle.scoreboard_ctrl.model.state == BoardState::End
                    && battle.scoreboard_ctrl.model.page == BoardPage::Table;
                let act = if done {
                    ScoreboardAction::Done
                } else {
                    ScoreboardAction::Back
                };
                battle
                    .scoreboard_ctrl
                    .act(act, &mut battle.scoreboard_port, battle.height);
            }
            BattleUiAction::ScoreMidFootClicked => {
                battle.scoreboard_ctrl.act(
                    ScoreboardAction::Details,
                    &mut battle.scoreboard_port,
                    battle.height,
                );
            }
            BattleUiAction::ScoreRightFootClicked => {
                battle.scoreboard_ctrl.act(
                    ScoreboardAction::Awards,
                    &mut battle.scoreboard_port,
                    battle.height,
                );
            }
            BattleUiAction::PauseRowHovered(idx) => {
                if battle.pause_menu.focused_row != idx || battle.pause_menu.back_focused {
                    battle.pause_menu.focused_row = idx;
                    battle.pause_menu.back_focused = false;
                    battle.pause_menu.sounds.push("shell_select_change");
                }
            }
            BattleUiAction::PauseRowActivated(idx) => {
                battle.pause_menu.focused_row = idx;
                battle.pause_menu.back_focused = false;
                if let Some(replicas) = &battle.scene.online {
                    let surface = HudBatchSurface {
                        height: battle.height,
                        aspect: battle.aspect,
                        batch: &mut battle.scene.bridge.hud_model.batch,
                        fonts: &battle.scene.bridge.hud_model.fonts,
                        assets: &battle.scene.bridge.hud_model.assets,
                        textures: Some(&replicas.hud.names),
                        strings: Some(&replicas.hud.strings),
                    };
                    battle.pause_menu.activate_main_row(
                        idx,
                        &surface,
                        &settings.player.pause_kv,
                    );
                }
            }
            BattleUiAction::PauseOptionHovered(idx) => {
                if let Some(PauseScreenState::Options { focused_row }) =
                    battle.pause_menu.stack.last_mut()
                {
                    if *focused_row != idx || battle.pause_menu.back_focused {
                        *focused_row = idx;
                        battle.pause_menu.back_focused = false;
                        battle.pause_menu.sounds.push("shell_select_change");
                    }
                }
            }
            BattleUiAction::PauseOptionActivated(idx) => {
                if let Some(PauseScreenState::Options { focused_row }) =
                    battle.pause_menu.stack.last_mut()
                {
                    *focused_row = idx;
                    battle.pause_menu.back_focused = false;
                    battle.pause_menu.sounds.push("shell_menu_enter");
                    // The client opens the chosen page over the paused battle.
                    battle.pause_menu.options_request = Some(idx);
                }
            }
            BattleUiAction::PausePlayerSelected(idx) => {
                if let Some(PauseScreenState::PlayerList { selected, .. }) =
                    battle.pause_menu.stack.last_mut()
                {
                    if *selected != idx {
                        *selected = idx;
                        battle.pause_menu.sounds.push("shell_select_change");
                    }
                }
            }
            BattleUiAction::PauseBackHovered => {
                if !battle.pause_menu.back_focused {
                    battle.pause_menu.back_focused = true;
                    battle.pause_menu.sounds.push("shell_select_change");
                }
            }
            BattleUiAction::PauseBackActivated => {
                if battle.pause_menu.stack.len() > 1 {
                    battle.pause_menu.stack.pop();
                    battle.pause_menu.back_focused = false;
                    battle.pause_menu.sounds.push("shell_menu_exit");
                } else {
                    battle.pause_menu.sounds.push("shell_menu_exit");
                    battle.pause_menu.close(true);
                }
            }
            BattleUiAction::PauseConfirmHovered(idx) => {
                if let Some(PauseScreenState::Confirm { focused_button, .. }) =
                    battle.pause_menu.stack.last_mut()
                {
                    if *focused_button != idx {
                        *focused_button = idx;
                        battle.pause_menu.sounds.push("shell_select_change");
                    }
                }
            }
            BattleUiAction::PauseConfirmAccepted => {
                battle
                    .pause_menu
                    .answer_confirm(true, &settings.player.pause_kv);
            }
            BattleUiAction::PauseConfirmCancelled => {
                battle
                    .pause_menu
                    .answer_confirm(false, &settings.player.pause_kv);
            }
            BattleUiAction::PauseThemeChanged(idx) => {
                battle.pause_page.set_theme(idx);
                battle.pause_page.save(settings);
            }
        }
    }
}

pub(super) fn sync_spawn_hotspots(battle: &mut BattleController) {
    let layout = battle.page_battle.layout;
    let w = layout.width;
    let h = layout.height;
    let ih = battle.height;
    let model = &battle.spawn_ctrl.model;
    let port = &battle.spawn_port;

    let Some(regions) = &mut battle.spawn_ctrl.view.theme_hotspots else {
        return;
    };
    regions.clear();

    if model.can_switch_teams(port) {
        for (i, r) in layout.teams.iter().copied().enumerate() {
            if (i + 1) as u8 != model.team {
                let mut hit = r;
                let emblem = (h * 0.074).clamp(46., 80.) + w * 0.007;
                if i == 0 {
                    hit[0] -= emblem;
                } else {
                    hit[2] += emblem;
                }
                regions.push((layout.interface(hit, ih), SpawnHitTarget::Team));
            }
        }
        let size = (h * 0.046).clamp(32., 46.);
        let center = [w * 0.5, (layout.teams[0][1] + layout.teams[0][3]) * 0.5];
        let swap = [
            center[0] - size * 0.5,
            center[1] - size * 0.5,
            center[0] + size * 0.5,
            center[1] + size * 0.5,
        ];
        regions.push((layout.interface(swap, ih), SpawnHitTarget::Team));
    }

    let roster = model.roster(port);
    for i in 0..roster.len() {
        let picked = i == model.class_index;
        let r = layout.class_rect(i, picked);
        regions.push((layout.interface(r, ih), SpawnHitTarget::Class(i)));
    }

    regions.push((layout.interface(layout.spawn, ih), SpawnHitTarget::Spawn));
    battle.page_battle.post_regions(
        model,
        port,
        battle.spawn_ctrl.view.map_proj,
        ih,
        regions,
    );
}

pub(super) fn sync_scoreboard_hotspots(battle: &mut BattleController) {
    let w = battle.page_size[0].max(1) as f64;
    let h = battle.page_size[1].max(1) as f64;
    let ih = battle.height;
    let shown = ((h * 0.90 - (h * 0.122 + 51. + h * 0.006 + h * 0.047 + (h * 0.038).clamp(28., 40.) + 6.))
        / ((h * 0.034).clamp(26., 38.) + 2.))
        .floor()
        .max(1.) as usize;
    battle.scoreboard_ctrl.model.shown = shown;

    let Some(regions) = &mut battle.scoreboard_ctrl.view.theme_hotspots else {
        return;
    };
    regions.clear();

    let to_if = |r: [f64; 4]| -> [f64; 4] {
        [
            r[0] * 1024. / w,
            r[1] * ih / h,
            r[2] * 1024. / w,
            r[3] * ih / h,
        ]
    };

    let edge = w * 0.03;
    let tab_y = h * 0.122;
    let model = &battle.scoreboard_ctrl.model;

    let mut tx = edge;
    for (page, label) in [
        (BoardPage::Table, "TABLE"),
        (BoardPage::Details, "DETAILS"),
        (BoardPage::Awards, "AWARDS"),
    ] {
        if page == BoardPage::Awards && model.state != BoardState::End {
            continue;
        }
        let tw = label.len() as f64 * 10. + 54.;
        let r = [tx, tab_y, tx + tw, tab_y + 42.];
        tx += tw + 8.;
        if page != model.page {
            regions.push((
                to_if(r),
                match page {
                    BoardPage::Table => ScoreboardHitTarget::Back,
                    BoardPage::Details => ScoreboardHitTarget::Details,
                    BoardPage::Awards => ScoreboardHitTarget::Awards,
                },
            ));
        }
    }

    if model.page == BoardPage::Table {
        let gap = w * 0.016;
        let pw = (w - 2. * edge - gap) * 0.5;
        let body_y = tab_y + 51. + h * 0.006;
        let panel_y = body_y + h * 0.047;
        let col_h = (h * 0.038).clamp(28., 40.);
        let row_h = (h * 0.034).clamp(26., 38.);
        let pitch = row_h + 2.;
        let list_top = panel_y + col_h + 2.;
        let show_ping = battle.scoreboard_port.network() && model.state == BoardState::Peek;
        let num_w = pw * if show_ping { 0.115 } else { 0.14 };
        let cp_w = pw * if show_ping { 0.15 } else { 0.17 };
        let ping_w = if show_ping { pw * 0.15 } else { 0. };
        let name_w = pw - ping_w - num_w * 2. - cp_w - 22.;
        let kills_x = 12. + name_w + ping_w;
        let deaths_x = kills_x + num_w;
        let cp_x = deaths_x + num_w;

        for side in [1usize, 2usize] {
            let x = if side == 1 { edge } else { edge + pw + gap };
            for (mode, cx, cw) in [
                (SortMode::Kills, x + kills_x, num_w),
                (SortMode::Deaths, x + deaths_x, num_w),
                (SortMode::Points, x + cp_x, cp_w),
            ] {
                regions.push((
                    to_if([cx, panel_y, cx + cw, panel_y + col_h]),
                    ScoreboardHitTarget::Sort { mode, side },
                ));
            }
            for (i, _) in model.lists[side - 1]
                .iter()
                .enumerate()
                .skip(model.first)
                .take(shown)
            {
                let y = list_top + (i - model.first) as f64 * pitch;
                let r = [x + 4., y, x + pw - 4., y + row_h];
                regions.push((to_if(r), ScoreboardHitTarget::Row { side, index: i }));
            }
        }
    }

    let bh = (h * 0.05).clamp(34., 48.);
    let bottom = h * 0.97;
    let done = model.state == BoardState::End && model.page == BoardPage::Table;
    regions.push((
        to_if([edge, bottom - bh, edge + 140., bottom]),
        if done {
            ScoreboardHitTarget::Done
        } else {
            ScoreboardHitTarget::Back
        },
    ));
    if model.page == BoardPage::Table {
        regions.push((
            to_if([w * 0.5 - 70., bottom - bh, w * 0.5 + 70., bottom]),
            ScoreboardHitTarget::Details,
        ));
        if model.state == BoardState::End {
            regions.push((
                to_if([w - edge - 140., bottom - bh, w - edge, bottom]),
                ScoreboardHitTarget::Awards,
            ));
        }
    }
}

pub(super) fn sync_pause_hotspots(battle: &mut BattleController) {
    let w = battle.page_size[0].max(1) as f64;
    let h = battle.page_size[1].max(1) as f64;
    let ih = battle.height;
    let hotspot = |r: [f64; 4], target: PauseHitTarget| PauseHotspot {
        target,
        corners: [
            [r[0] * 1024. / w, r[1] * ih / h],
            [r[0] * 1024. / w, r[3] * ih / h],
            [r[2] * 1024. / w, r[3] * ih / h],
            [r[2] * 1024. / w, r[1] * ih / h],
        ],
    };

    let mut hits = Vec::with_capacity(16);
    let bw = (w * 0.30).min(400.) - 16.;
    let bh = (h * 0.056).clamp(38., 56.);
    let gap = (h * 0.011).clamp(8., 14.);
    let heading_h = (h * 0.046).clamp(26., 50.);
    let pad_top = h * 0.07;
    let pad_bottom = h * 0.06 + (h * 0.046).clamp(34., 46.);
    let avail_h = (h - pad_top - pad_bottom).max(0.);
    let head_gap = h * 0.05;
    let body_y = pad_top + heading_h + h * 0.026;
    let confirm = matches!(battle.pause_menu.top(), Some(PauseScreenState::Confirm { .. }));

    if matches!(
        battle.pause_menu.top(),
        Some(PauseScreenState::Main) | None
    ) {
        let n = if let Some(replicas) = &battle.scene.online {
            let surface = HudBatchSurface {
                height: battle.height,
                aspect: battle.aspect,
                batch: &mut battle.scene.bridge.hud_model.batch,
                fonts: &battle.scene.bridge.hud_model.fonts,
                assets: &battle.scene.bridge.hud_model.assets,
                textures: Some(&replicas.hud.names),
                strings: Some(&replicas.hud.strings),
            };
            battle.pause_menu.visible_rows(&surface).len()
        } else {
            4 + usize::from(!matches!(
                battle.pause_menu.restart_mode,
                bfe_battle_ui::pause::PauseRestartMode::Hidden
            )) + usize::from(battle.pause_menu.show_player_list)
        };
        let rows_h = (n as f64 * bh + n.saturating_sub(1) as f64 * gap + 8.).max(0.);
        let heading_y = pad_top + (avail_h - (heading_h + head_gap + rows_h)) * 0.5;
        let top = heading_y + heading_h + head_gap + 4.;
        for i in 0..n {
            let y = top + i as f64 * (bh + gap);
            let r = [(w - bw) * 0.5, y, (w + bw) * 0.5, y + bh];
            hits.push(hotspot(r, PauseHitTarget::MainRow(i)));
        }
    } else if matches!(
        battle.pause_menu.top(),
        Some(PauseScreenState::Options { .. })
    ) {
        let n = 5usize;
        let rows_h = n as f64 * bh + (n - 1) as f64 * gap + 8.;
        let heading_y = pad_top + (avail_h - (heading_h + head_gap + rows_h)) * 0.5;
        let top = heading_y + heading_h + head_gap + 4.;
        for i in 0..n {
            let y = top + i as f64 * (bh + gap);
            let r = [(w - bw) * 0.5, y, (w + bw) * 0.5, y + bh];
            hits.push(hotspot(r, PauseHitTarget::OptionRow(i)));
        }
    } else if let Some(PauseScreenState::PlayerList { first, .. }) = battle.pause_menu.top() {
        let first = *first;
        for (i, _) in battle
            .pause_menu
            .players
            .iter()
            .enumerate()
            .skip(first)
            .take(12)
        {
            let y = body_y + (i - first) as f64 * 32.;
            let r = [w * 0.03, y, w * 0.97, y + 30.];
            hits.push(hotspot(r, PauseHitTarget::PlayerRow(i)));
        }
    } else if matches!(
        battle.pause_menu.top(),
        Some(PauseScreenState::Confirm { .. })
    ) {
        for i in 0..2 {
            let button_w = (w * 0.11).clamp(120., 200.);
            let button_h = (h * 0.046).clamp(34., 46.);
            let x = w * 0.5 + (i as f64 - 0.5) * (button_w + w * 0.012);
            let by = h * 0.5 + 24.;
            let r = [x - button_w * 0.5, by, x + button_w * 0.5, by + button_h];
            hits.push(hotspot(r, PauseHitTarget::ConfirmButton(i)));
        }
    }

    if !confirm {
        let edge = (w * 0.03).clamp(20., 56.);
        let r = [edge, h * 0.923, edge + 140., h * 0.97];
        hits.push(hotspot(r, PauseHitTarget::BackButton));
    }
    battle.pause_menu.set_theme_hotspots(&mut hits);
}
