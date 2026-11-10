//! First-run choices and preflight checks, following electron/main.js's
//! askToPrepare/checkCopy and electron/ui/prepare.js's state text.
use super::settings::{
    application_resources, current_arch_str, current_platform_str, AppSettingsStore,
};
use bfe_desktop::{
    discover::GameInstall,
    prepare::{
        toolchain_paths, PrepareSession, PrepareSnapshot, PrepareSpawnOptions, ToolchainOptions,
    },
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Debug)]
pub struct SetupProgress {
    pub fraction: f64,
    pub label: String,
    pub numbers: String,
    pub phases: [u8; 7],
    pub show_phases: bool,
    pub estimating: bool,
}

#[derive(Clone, Debug)]
pub struct SetupSound {
    pub action: usize,
    pub checked: bool,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct SetupFact {
    pub name: String,
    pub value: String,
    pub monospace: bool,
    pub status: &'static str,
}

#[derive(Clone, Debug)]
pub struct SetupCopy {
    pub index: usize,
    pub action: usize,
    pub dir: String,
    pub how: String,
}

#[derive(Clone, Debug, Default)]
pub struct SetupView {
    pub planet: String,
    pub title: String,
    pub detail: String,
    pub facts: Vec<SetupFact>,
    pub problems: Vec<String>,
    pub help: String,
    pub repair: String,
    pub other_copies: Vec<SetupCopy>,
    pub actions: Vec<String>,
    pub progress: Option<SetupProgress>,
    pub code: Option<String>,
    pub sound: Option<SetupSound>,
    pub left_out_title: String,
    pub left_out_items: Vec<String>,
    pub left_out_help: String,
    pub listing: bool,
    pub busy: bool,
    pub logs: Vec<String>,
    pub link_action: Option<usize>,
    pub warning: bool,
    pub error: bool,
}

pub const NO_ACTION: usize = usize::MAX;

pub const MISSING_DETAIL: &str = "Battlefront Eternal plays the battles from your own copy of Star Wars: Battlefront (2004). Import a content export from your other computer, or install the original from Steam or GOG. If it is installed somewhere else, choose its folder.";
pub const REPAIR: &str = "In Steam: the game's Properties > Installed Files > Verify integrity of game files. In GOG Galaxy: the game's settings > Manage installation > Verify / Repair.";
/// Ports failureMessage: use the tool's saved receipt, including skipped tools.
pub fn failure_message(target: &Path, snapshot: &PrepareSnapshot) -> String {
    let state: Value = std::fs::read_to_string(target.join(".prepare.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    if let Some(skipped) = state["skipped"].as_array().filter(|s| !s.is_empty()) {
        let names = skipped
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("Some of the tools preparation needs are missing ({names}). See the preparation log for details.");
    }
    let failures = state["failures"].as_array().cloned().unwrap_or_default();
    if failures.is_empty() {
        return snapshot.error.clone().unwrap_or_else(|| "Game data could not be fully updated. Retry preparation before playing. See the preparation log for details.".into());
    }
    let mut lines =
        vec!["These parts of the game could not be prepared from your copy:".to_string()];
    for item in failures.iter().take(3) {
        let key = item["key"].as_str().unwrap_or("");
        let label = item["label"].as_str().unwrap_or(key);
        let mut name = if label != key {
            format!("{label} ({key})")
        } else {
            key.into()
        };
        if let Some(reason) = item["reason"].as_str().filter(|r| !r.is_empty()) {
            name.push_str(": ");
            name.push_str(reason);
        }
        lines.push(name);
    }
    if failures.len() > 3 {
        lines.push(format!("and {} more", failures.len() - 3));
    }
    lines.push("A game file is probably missing, changed or damaged, for example by a mod. Repair the copy, then choose TRY AGAIN, or CHOOSE FOLDER to use another copy.".into());
    lines.push(REPAIR.into());
    lines.join("\n")
}

#[derive(Clone, Copy)]
pub enum Action {
    Choose,
    Import,
    Steam,
    Gog,
    Search,
    Check,
    Prepare,
    Retry,
    Sound,
    Pick(usize),
    Details,
    Dismiss,
}

pub struct Setup {
    pub copies: Vec<GameInstall>,
    pub selected: usize,
    pub cursor: usize,
    /// The action the keyboard's focus is on, if any (the page's `:focus-visible` element).
    /// `cursor` is the lit button: the hovered one, the focused one, else the state's own choice.
    pub focus: Option<usize>,
    pub check: Option<Value>,
    check_session: Option<PrepareSession>,
    check_started: Instant,
    pub message: Option<String>,
    pub stopped: bool,
    pub transfer_error: bool,
    pub outdated: bool,
    pub show_details: bool,
    pub logs: Vec<String>,
    pub enhanced_sound: bool,
    pub failed_snapshot: Option<Box<PrepareSnapshot>>,
    timings: bfe_desktop::prepare::StepTimings,
}
impl Setup {
    pub fn missing(outdated: bool) -> Self {
        Self {
            copies: Vec::new(),
            selected: 0,
            cursor: 0,
            focus: None,
            check: None,
            check_session: None,
            check_started: Instant::now(),
            message: None,
            stopped: false,
            transfer_error: false,
            outdated,
            show_details: false,
            logs: Vec::new(),
            enhanced_sound: true,
            failed_snapshot: None,
            timings: Default::default(),
        }
    }
    pub fn confirm(copies: Vec<GameInstall>, store: &AppSettingsStore) -> Self {
        let mut model = Self::missing(false);
        model.copies = copies;
        model.enhanced_sound = store.desktop.enhanced_sound.unwrap_or(true);
        model.recheck(store);
        model
    }
    pub fn failure(message: String, game: Option<PathBuf>, logs: Vec<String>) -> Self {
        let mut model = Self::missing(false);
        model.message = Some(message);
        model.stopped = true;
        model.logs = logs;
        if let Some(dir) = game {
            model.copies.push(GameInstall {
                dir,
                source: "chosen".into(),
                how: "the folder you chose".into(),
                chosen: true,
            });
        }
        model
    }
    pub fn recheck(&mut self, store: &AppSettingsStore) {
        self.check = None;
        self.cursor = NO_ACTION;
        self.focus = None;
        self.timings = std::fs::read_to_string(&store.paths.timings_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        self.check_started = Instant::now();
        let Some(copy) = self.copies.get(self.selected) else {
            return;
        };
        match PrepareSession::start(options(
            &copy.dir,
            &store.paths.managed_content_dir,
            store,
            true,
        )) {
            Ok(session) => self.check_session = Some(session),
            Err(error) => {
                self.check = Some(unavailable(error));
                self.cursor = 0;
                self.focus = None;
            }
        }
    }
    pub fn poll(&mut self) {
        if let Some(session) = &self.check_session {
            while let Ok(snap) = session.events.try_recv() {
                if let Some(check) = snap.check_report {
                    self.check = Some(check);
                } else if snap.status == "failed" {
                    self.check = Some(unavailable(snap.error.unwrap_or_else(|| {
                        "The preparation tools couldn't check this copy.".into()
                    })));
                }
            }
            if self.check.is_none() && self.check_started.elapsed().as_secs() > 60 {
                session.cancel();
                self.check=Some(unavailable("Checking this copy took too long. Is it on a drive that's asleep or disconnected?".into()));
            }
            if self.check.is_some() {
                self.cursor = 0;
                self.focus = None;
                self.check_session = None;
            }
        }
    }
    /// Checking has no primary action until the read-only report arrives.
    pub fn primary_action(&self) -> usize {
        if self.check.is_none() && !self.copies.is_empty() && self.message.is_none() {
            NO_ACTION
        } else {
            0
        }
    }
    /// Whether an action is one of the front end's lit-together buttons (`#actions`); the
    /// sound option, the other-copy buttons and the details link sit outside that group.
    pub fn is_group_button(&self, index: usize) -> bool {
        !matches!(
            self.actions().get(index),
            Some((_, Action::Sound | Action::Pick(_) | Action::Details))
        )
    }
    /// Tab order of the page: the sound option, the other-copy buttons, the actions, then the
    /// details link, as prepare.js lays them out.
    fn focus_order(&self) -> Vec<usize> {
        let actions = self.actions();
        let rank = |a: &Action| match a {
            Action::Sound => 0,
            Action::Pick(_) => 1,
            Action::Details => 3,
            _ => 2,
        };
        let mut order: Vec<usize> = (0..actions.len()).collect();
        order.sort_by_key(|&i| rank(&actions[i].1));
        order
    }
    /// Tab (and the arrow keys) move the keyboard's focus; nothing holds it until the first
    /// press, which takes the first or, backwards, the last focusable item.
    pub fn move_focus(&mut self, backwards: bool) {
        let order = self.focus_order();
        if order.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| order.iter().position(|&i| i == f));
        let next = match (at, backwards) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(p), false) => (p + 1) % order.len(),
            (Some(p), true) => (p + order.len() - 1) % order.len(),
        };
        let target = order[next];
        self.focus = Some(target);
        // prepare.js lights a button when it takes the focus; focus leaving the
        // button group lights the state's own choice again.
        self.cursor = if self.is_group_button(target) {
            target
        } else {
            self.primary_action()
        };
    }
    pub fn actions(&self) -> Vec<(String, Action)> {
        let button = |label: &str, action| (label.into(), action);
        if self.stopped {
            let mut actions = vec![
                button("TRY AGAIN", Action::Retry),
                button("CHOOSE FOLDER", Action::Choose),
            ];
            if !self.logs.is_empty() {
                actions.push(button(
                    if self.show_details {
                        "HIDE DETAILS"
                    } else {
                        "SHOW DETAILS"
                    },
                    Action::Details,
                ));
            }
            return actions;
        }
        if self.message.is_some() {
            if self.transfer_error {
                return vec![
                    button("IMPORT CONTENT EXPORT", Action::Import),
                    button("CHOOSE GAME FOLDER", Action::Choose),
                ];
            }
            return vec![button("OK", Action::Dismiss)];
        }
        if self.copies.is_empty() {
            return if self.outdated {
                vec![
                    button("CHOOSE GAME FOLDER", Action::Choose),
                    button("IMPORT CONTENT EXPORT", Action::Import),
                    button("SEARCH AGAIN", Action::Search),
                ]
            } else {
                vec![
                    button("IMPORT CONTENT EXPORT", Action::Import),
                    button("CHOOSE GAME FOLDER", Action::Choose),
                    button("GET IT ON STEAM", Action::Steam),
                    button("GET IT ON GOG", Action::Gog),
                    button("SEARCH AGAIN", Action::Search),
                ]
            };
        }
        let mut actions = Vec::new();
        if let Some(check) = &self.check {
            if check["ok"] == true {
                actions.push(button("PREPARE", Action::Prepare));
            } else {
                actions.push(button("CHECK AGAIN", Action::Check));
                if check["unavailable"] == true {
                    actions.push(button("PREPARE ANYWAY", Action::Prepare));
                }
            }
        }
        actions.push(button("CHOOSE ANOTHER FOLDER", Action::Choose));
        actions.push(button("IMPORT CONTENT EXPORT", Action::Import));
        if self
            .check
            .as_ref()
            .is_some_and(|c| c["ok"] == true || c["unavailable"] == true)
        {
            actions.push(button("ENHANCED SOUND", Action::Sound));
        }
        for i in 0..self.copies.len() {
            if i != self.selected {
                actions.push(button("USE THIS COPY", Action::Pick(i)));
            }
        }
        actions
    }
    pub fn view(&self) -> SetupView {
        let mut v = SetupView::default();
        let action_pairs = self.actions();
        v.actions = action_pairs.iter().map(|(label, _)| label.clone()).collect();
        if self.stopped {
            v.title = "SOMETHING WENT WRONG".into();
            v.detail = self.message.clone().unwrap_or_default();
            v.error = true;
            if let Some(snapshot) = self.failed_snapshot.as_ref() {
                let pv = progress_view(snapshot);
                v.progress = pv.progress;
                v.left_out_title = pv.left_out_title;
                v.left_out_items = pv.left_out_items;
                v.left_out_help = pv.left_out_help;
            }
            v.link_action = (v.actions.len() > 2).then_some(2);
            if self.show_details {
                v.logs = self.logs.clone();
            }
            return v;
        }
        if let Some(message) = &self.message {
            v.title = if self.transfer_error {
                "CONTENT TRANSFER STOPPED"
            } else {
                "SOMETHING WENT WRONG"
            }
            .into();
            v.detail = message.clone();
            v.error = true;
            return v;
        }
        let Some(copy) = self.copies.get(self.selected) else {
            v.warning = true;
            v.title = if self.outdated {
                "GAME DATA NEEDS AN UPDATE"
            } else {
                "STAR WARS BATTLEFRONT NOT FOUND"
            }
            .into();
            v.detail=if self.outdated{"This content was prepared for a different version. Install Star Wars: Battlefront (2004) from Steam or GOG, or choose its folder, to update automatically, or import a current content export from your other computer."}else{MISSING_DETAIL}.into();
            return v;
        };
        v.listing = true;
        v.busy = self.check.is_none();
        let unchecked = self
            .check
            .as_ref()
            .is_some_and(|c| c["unavailable"] == true);
        let failed = self
            .check
            .as_ref()
            .is_some_and(|c| c["ok"] != true && !unchecked);
        v.title = if self.check.is_none() {
            "CHECKING YOUR COPY"
        } else if failed {
            "THIS COPY CAN'T BE PREPARED"
        } else if unchecked {
            "YOUR COPY COULDN'T BE CHECKED"
        } else {
            "STAR WARS BATTLEFRONT FOUND"
        }
        .into();
        v.detail=if failed{"Battlefront Eternal looked at this copy before converting anything, and found what would stop it."}else if unchecked{"Battlefront Eternal couldn't look at this copy before converting it, so it doesn't know yet whether anything is missing. CHECK AGAIN tries once more. PREPARE ANYWAY starts now, and says what stopped it if a file turns out to be missing or damaged."}else{"Battlefront Eternal converts the game's maps, armies, sound and movies into formats it can read. This happens once. Your installation is only read, never changed."}.into();
        if !failed && !unchecked {
            if let Some(check) = &self.check {
                if check["counts"].is_object() {
                    let workers = check["workers"].as_f64().unwrap_or(1.).max(1.);
                    let sides = check["counts"]["sides"].as_f64().unwrap_or(0.);
                    let maps = check["counts"]["maps"].as_f64().unwrap_or(0.);
                    let sec = |kind| {
                        bfe_desktop::prepare::expected_seconds_for_kind(kind, Some(&self.timings))
                    };
                    let sound = if self.enhanced_sound {
                        check["counts"]["sublevels"].as_f64().unwrap_or(0.) * sec("audio")
                    } else {
                        0.
                    };
                    let serial = [
                        "catalog",
                        "common",
                        "shell",
                        "catalog-update",
                        "environment",
                        "hud",
                        "textures",
                        "movies",
                    ]
                    .iter()
                    .map(|kind| sec(kind))
                    .sum::<f64>();
                    let seconds =
                        serial + (sides * sec("side") + maps * sec("map")) / workers + sound;
                    v.detail=format!("Battlefront Eternal converts the game's maps, armies, sound and movies into formats it can read. This happens once and takes about {} on this computer. Your installation is only read, never changed.",clock(seconds));
                }
            }
        }
        v.error = failed;
        v.warning = unchecked;
        v.facts.push(SetupFact {
            name: "FOLDER".into(),
            value: copy.dir.to_string_lossy().into_owned(),
            monospace: true,
            status: "",
        });
        v.facts.push(SetupFact {
            name: "FOUND".into(),
            value: found_in(copy),
            monospace: false,
            status: "",
        });
        let (check_text, check_status) = if let Some(check) = &self.check {
            if check["ok"] == true {
                ("Everything preparation needs is there.".into(), "good")
            } else if unchecked {
                (
                    check["problems"][0]["message"]
                        .as_str()
                        .unwrap_or("The check couldn't run.")
                        .into(),
                    "unchecked",
                )
            } else {
                let n = check["problems"].as_array().map_or(0, Vec::len);
                let text = if n == 1 {
                    "A problem found.".into()
                } else {
                    format!("{n} problems found.")
                };
                (text, "bad")
            }
        } else {
            ("Looking at what preparation needs.".into(), "busy")
        };
        v.facts.push(SetupFact {
            name: "CHECK".into(),
            value: check_text,
            monospace: false,
            status: check_status,
        });
        if let Some(check) = &self.check {
            if failed {
                if let Some(problems) = check["problems"].as_array() {
                    for p in problems.iter().take(6) {
                        if let Some(text) = p["message"].as_str() {
                            v.problems.push(text.into());
                        }
                    }
                    if problems.len() > 6 {
                        v.problems.push(format!("and {} more", problems.len() - 6));
                    }
                }
                v.help = if self.copies.len() > 1 {
                    "Use another copy below, or repair this one and choose CHECK AGAIN.".into()
                } else {
                    "Repair this one and choose CHECK AGAIN.".into()
                };
                if copy.source != "chosen" && !copy.chosen {
                    v.repair = REPAIR.into();
                }
            } else {
                if check["ok"] == true {
                    let add_on_text = check["addOns"]
                        .as_array()
                        .filter(|a| !a.is_empty())
                        .map(|add_ons| {
                            let names: Vec<_> = add_ons
                                .iter()
                                .take(4)
                                .map(|item| {
                                    let id = item["id"].as_str().unwrap_or("");
                                    let name = item["name"].as_str().unwrap_or(id);
                                    if !name.is_empty() && name != id {
                                        format!("{name} ({id})")
                                    } else {
                                        id.to_string()
                                    }
                                })
                                .collect();
                            let more = if add_ons.len() > 4 {
                                format!(" and {} more", add_ons.len() - 4)
                            } else {
                                String::new()
                            };
                            format!(
                                "{}{more} \u{2014} included automatically",
                                names.join(", ")
                            )
                        })
                        .unwrap_or_else(|| "None".into());
                    v.facts.push(SetupFact {
                        name: "ADD-ON MAPS".into(),
                        value: add_on_text,
                        monospace: false,
                        status: "",
                    });
                }
                if let Some(index) = action_pairs
                    .iter()
                    .position(|(_, a)| matches!(a, Action::Sound))
                {
                    let sound = check["counts"]["sublevels"].as_f64().unwrap_or(0.)
                        * bfe_desktop::prepare::expected_seconds_for_kind(
                            "audio",
                            Some(&self.timings),
                        );
                    let adds = if sound > 0. {
                        format!(" Adds about {}.", clock(sound))
                    } else {
                        String::new()
                    };
                    v.sound = Some(SetupSound {
                        action: index,
                        checked: self.enhanced_sound,
                        description: format!("The original sound, resampled and sharpened for modern speakers.{adds} Without it the game plays the original sound, and you can add it later."),
                    });
                }
            }
        }
        for (i, other) in self.copies.iter().enumerate() {
            if i != self.selected {
                let action_idx = action_pairs
                    .iter()
                    .position(|(_, a)| matches!(a, Action::Pick(idx) if *idx == i))
                    .unwrap_or(NO_ACTION);
                v.other_copies.push(SetupCopy {
                    index: i,
                    action: action_idx,
                    dir: other.dir.to_string_lossy().into_owned(),
                    how: found_in(other),
                });
            }
        }
        v
    }
}
/// Ports `foundIn` in electron/ui/prepare.js: where a copy of the game came from.
fn found_in(copy: &GameInstall) -> String {
    match copy.source.as_str() {
        "chosen" => "The folder you chose".into(),
        "steam" | "gog" => format!("In {}", copy.how),
        _ => {
            let mut letters = copy.how.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect())
                .unwrap_or_default()
        }
    }
}
fn unavailable(message: String) -> Value {
    json!({"ok":false,"unavailable":true,"problems":[{"code":"check","message":message}]})
}

pub fn options(
    game: &Path,
    target: &Path,
    store: &AppSettingsStore,
    check: bool,
) -> PrepareSpawnOptions {
    let resources = application_resources();
    PrepareSpawnOptions {
        toolchain: toolchain_paths(&ToolchainOptions {
            packaged: resources.is_some(),
            resources_path: resources,
            project_root: store.paths.project_root.clone(),
            platform: current_platform_str().into(),
            arch: current_arch_str().into(),
        }),
        game_dir: game.into(),
        content_dir: target.into(),
        env: std::env::vars().collect(),
        platform: current_platform_str().into(),
        include_add_ons: true,
        enhanced_sound: store.desktop.enhanced_sound.unwrap_or(true),
        plan_only: check,
    }
}

/// estimate(snapshot) and drawProgress from the browser: duration-weighted
/// work, capped running fractions, batched armies/maps and the same clock text.
pub fn progress_view(snapshot: &PrepareSnapshot) -> SetupView {
    let mut v = SetupView::default();
    v.busy = true;
    v.title = if snapshot.playable {
        "BATTLEFIELDS READY \u{00b7} FINISHING UP"
    } else {
        "PREPARING YOUR BATTLEFRONT"
    }
    .into();
    v.detail = if snapshot.enhanced_sound {"Preparing maps, armies, sound and movies from your copy. This happens once."} else {"Preparing maps, armies and movies from your copy. This happens once. The Enhanced sound is off: you can add it later."}.into();
    if snapshot.playable {
        v.detail=if snapshot.enhanced_sound {"Every battlefield is in. Now the HUD, the enhanced sound, textures and the menu movies."}else{"Every battlefield is in. Now the HUD, textures and the menu movies."}.into();
    } else {
        v.code = snapshot.game.clone();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let (mut total, mut done, mut remaining, mut measured, mut expected, mut samples) =
        (0., 0., 0., 0., 0., 0);
    let mut running = Vec::new();
    for step in &snapshot.steps {
        if ["unavailable", "off", "left-out"].contains(&step.status.as_str()) {
            continue;
        }
        let e = bfe_desktop::prepare::expected_seconds(step, Some(&snapshot.timings));
        total += e;
        let batch = step.id.starts_with("map:") || step.id.starts_with("side:");
        let workers = if batch {
            snapshot.workers.max(1) as f64
        } else {
            1.
        };
        if step.status == "done" || step.status == "failed" {
            done += e;
            if step.status == "done" && !step.cached {
                if let Some(seconds) = step.seconds.filter(|n| *n > 0.) {
                    measured += seconds;
                    expected += e;
                    samples += 1;
                }
            }
        } else if step.status == "running" {
            let part = step
                .started_at
                .map_or(0., |since| now.saturating_sub(since) as f64 / 1000.)
                .min(e * 0.95);
            done += part;
            remaining += (e - part) / workers;
            running.push(step);
        } else {
            remaining += e / workers;
        }
    }
    let known = snapshot.steps.iter().any(|s| s.id.starts_with("map:"));
    let over = snapshot.finished_at.is_some();
    let fraction = if over {
        1.
    } else if known && total > 0. {
        (done / total).min(0.999)
    } else {
        0.
    };
    let calibration = if samples >= 3 && expected > 0. {
        (measured / expected).clamp(0.5, 3.)
    } else {
        1.
    };
    let current = snapshot
        .current_step
        .as_ref()
        .and_then(|key| running.iter().find(|s| &s.id == key));
    let label = current.map_or_else(
        || {
            if over {
                String::new()
            } else {
                "STARTING".into()
            }
        },
        |s| {
            format!(
                "{}{}",
                s.label.to_uppercase(),
                if running.len() > 1 {
                    format!("  +{} MORE", running.len() - 1)
                } else {
                    String::new()
                }
            )
        },
    );
    let elapsed = snapshot
        .started_at
        .map_or(0., |since| now.saturating_sub(since) as f64 / 1000.);
    let mut numbers = vec![format!("{}%", (fraction * 100.).floor() as u32)];
    if elapsed >= 60. && !over {
        numbers.push(format!("{} elapsed", clock(elapsed)));
    }
    if remaining > 0. && !over {
        numbers.push(format!("about {} left", clock(remaining * calibration)));
    }
    let groups = [
        "catalog", "armies", "shell", "maps", "audio", "textures", "movies",
    ];
    let mut phases = [0; 7];
    for (i, group) in groups.iter().enumerate() {
        let matching: Vec<_> = snapshot
            .steps
            .iter()
            .filter(|s| phase(&s.id) == *group)
            .collect();
        if matching.iter().any(|s| s.status == "running") {
            phases[i] = 2;
        } else if !matching.is_empty() && matching.iter().all(|s| s.status == "off") {
            phases[i] = 3;
        } else if !matching.is_empty() && matching.iter().all(|s| s.status == "unavailable") {
            phases[i] = 4;
        } else if !matching.is_empty()
            && matching.iter().all(|s| {
                ["done", "failed", "off", "unavailable", "left-out"].contains(&s.status.as_str())
            })
        {
            phases[i] = 1;
        }
    }
    let left_out_steps: Vec<_> = snapshot
        .steps
        .iter()
        .filter(|s| s.status == "left-out")
        .collect();
    if !left_out_steps.is_empty() {
        v.left_out_title = if left_out_steps.len() == 1 {
            "AN ADD-ON MAP WAS LEFT OUT".into()
        } else {
            format!("{} ADD-ON MAPS WERE LEFT OUT", left_out_steps.len())
        };
        for item in left_out_steps.iter().take(4) {
            let short = item.id.strip_prefix("map:").unwrap_or(&item.id);
            let mut text = if !item.label.is_empty() && item.label != short {
                format!("{} ({short})", item.label)
            } else {
                short.to_string()
            };
            if let Some(reason) = item.detail.as_deref().filter(|r| !r.is_empty()) {
                text.push_str(": ");
                text.push_str(reason);
            }
            v.left_out_items.push(text);
        }
        if left_out_steps.len() > 4 {
            v.left_out_items
                .push(format!("and {} more", left_out_steps.len() - 4));
        }
        v.left_out_help = "The original battlefields are unaffected.".into();
    }
    v.progress = Some(SetupProgress {
        fraction,
        label,
        numbers: if known {
            // prepare.js joins with two spaces each side; the page's `white-space: nowrap`
            // collapses them, so the drawn text has one.
            numbers.join(" \u{00b7} ")
        } else {
            String::new()
        },
        phases,
        show_phases: true,
        estimating: !known && !over,
    });
    v
}
fn phase(key: &str) -> &str {
    if key.starts_with("side:") || key == "common" {
        "armies"
    } else if key.starts_with("map:") {
        "maps"
    } else if key.starts_with("audio:") {
        "audio"
    } else if key == "catalog-update" {
        "catalog"
    } else if ["hud", "environment"].contains(&key) {
        "shell"
    } else {
        key
    }
}
pub fn clock(seconds: f64) -> String {
    let minutes = (seconds / 60.).round() as u64;
    if minutes < 1 {
        "under a minute".into()
    } else if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

/// File import uses the existing archive verifier and commits only after the
/// final checksum. The window receives progress without blocking on disk I/O.
pub struct ImportWork {
    result: std::sync::mpsc::Receiver<Result<PathBuf, String>>,
    progress: std::sync::mpsc::Receiver<(String, u64, u64)>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    current: (String, u64, u64),
}
impl ImportWork {
    pub fn start(file: PathBuf, store: &AppSettingsStore) -> Self {
        let (tx, result) = std::sync::mpsc::channel();
        let (progress_tx, progress) = std::sync::mpsc::channel();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = cancelled.clone();
        let parent = store.paths.user_data.join("content-imports");
        let tools = application_resources()
            .map(|r| r.join("tools"))
            .unwrap_or_else(|| store.paths.project_root.join("tools"));
        std::thread::spawn(move || {
            let out = bfe_desktop::archive::import_content_with_progress(
                &file,
                &parent,
                Some(&tools),
                |name, done, total| {
                    if flag.load(std::sync::atomic::Ordering::Acquire) {
                        return Err("Transfer cancelled. Your current content is unchanged.".into());
                    }
                    let _ = progress_tx.send((name.into(), done, total));
                    Ok(())
                },
            )
            .map(|result| result.content_dir);
            if flag.load(std::sync::atomic::Ordering::Acquire) {
                if let Ok(path) = &out {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
            let _ = tx.send(out);
        });
        Self {
            result,
            progress,
            cancelled,
            current: (String::new(), 0, 0),
        }
    }
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        while let Ok(progress) = self.progress.try_recv() {
            self.current = progress;
        }
        self.result.try_recv().ok().map(|result| {
            if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                if let Ok(path) = result {
                    let _ = std::fs::remove_dir_all(path);
                }
                Err("Transfer cancelled. Your current content is unchanged.".into())
            } else {
                result
            }
        })
    }
    pub fn view(&self) -> SetupView {
        let mut view = SetupView {
            title: "IMPORTING YOUR CONTENT".into(),
            detail: "Importing".into(),
            actions: vec!["CANCEL".into()],
            ..Default::default()
        };
        let (file, done, total) = &self.current;
        view.code = (!file.is_empty()).then(|| file.clone());
        let fraction = if *total > 0 {
            *done as f64 / *total as f64
        } else {
            0.
        };
        view.progress = Some(SetupProgress {
            fraction,
            label: "Importing".into(),
            numbers: format!("{}%", (fraction * 100.).floor() as u64),
            phases: [0; 7],
            show_phases: false,
            estimating: false,
        });
        view
    }
}
impl Drop for ImportWork {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn copy(dir: &str) -> GameInstall {
        GameInstall {
            dir: dir.into(),
            source: "steam".into(),
            how: "in your Steam library".into(),
            chosen: false,
        }
    }

    /// A checked copy with a second install: the sound option, the other copy's
    /// button and the three actions, in the order prepare.js puts them in the page.
    fn ready() -> Setup {
        let mut setup = Setup::missing(false);
        setup.copies = vec![copy("/opt/example/one"), copy("/opt/example/two")];
        setup.check = Some(json!({"ok": true}));
        setup.cursor = 0;
        setup
    }

    #[test]
    fn found_in_words_each_source_like_the_page() {
        let mut install = copy("/opt/example/one");
        install.how = "your Steam library".into();
        assert_eq!(found_in(&install), "In your Steam library");
        install.source = "chosen".into();
        assert_eq!(found_in(&install), "The folder you chose");
        install.source = "folder".into();
        install.how = "a folder next to the game".into();
        assert_eq!(found_in(&install), "A folder next to the game");
    }

    #[test]
    fn nothing_is_focused_until_the_first_tab_takes_the_first_item() {
        let mut setup = ready();
        assert_eq!(setup.focus, None);
        setup.move_focus(false);
        let actions = setup.actions();
        assert!(matches!(actions[setup.focus.unwrap()].1, Action::Sound));
        // The sound option is outside the lit button group: the state's own choice stays lit.
        assert_eq!(setup.cursor, setup.primary_action());
    }

    #[test]
    fn tab_walks_sound_then_copies_then_the_actions_and_wraps() {
        let mut setup = ready();
        let mut seen = Vec::new();
        for _ in 0..6 {
            setup.move_focus(false);
            seen.push(setup.actions()[setup.focus.unwrap()].1);
        }
        let names: Vec<_> = seen
            .iter()
            .map(|a| match a {
                Action::Sound => "sound",
                Action::Pick(_) => "copy",
                Action::Prepare => "prepare",
                Action::Choose => "choose",
                Action::Import => "import",
                _ => "other",
            })
            .collect();
        assert_eq!(names, ["sound", "copy", "prepare", "choose", "import", "sound"]);
    }

    #[test]
    fn shift_tab_from_nothing_takes_the_last_item_and_lights_it() {
        let mut setup = ready();
        setup.move_focus(true);
        let focus = setup.focus.unwrap();
        assert!(matches!(setup.actions()[focus].1, Action::Import));
        assert_eq!(setup.cursor, focus);
    }
}
