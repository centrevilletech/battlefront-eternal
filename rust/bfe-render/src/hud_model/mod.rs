//! BattleHud's persistent display model and frame geometry. The host supplies
//! scalar game facts, never already-computed fills, colours or glyph quads.
//! Ports hud.js and hud-units.js; interface geometry is shared with Rust UI.
// The battle HUD, laid out as the 2004 game lays it out.
//
// It draws in the interface space and with the drawing kit of
// the shared Rust interface module: 1024 units across the screen and
// H = 768 + (h / w - 0.75) x 768 units down it, y down. Each group hangs
// from an anchor on the screen, most inside a 5% safe margin, through its
// own transform, and it is all drawn through one fixed perspective camera
// (60 degrees across, 46.8 up, whatever the window's shape) onto a plane
// 886.81 units away, so the groups turned about their vertical axis show in
// perspective. Panels are cut from hud1 and hud2 by exact texture
// rectangles, text is the game's own bitmap fonts, and every quad is its
// texture times a tint, alpha blended.
//
// - Weapons, top left, turned 30 degrees: a panel per channel with the
//   weapon's icon, rounds in the clip / in reserve, and a bar for a
//   launcher's lock, a charge or a reload's progress (green) or what heat
//   leaves (blue, red overheated). A panel's weapon's name shows when it
//   changes and fades over 2 s.
// - Health, top right, turned -30 degrees: a bar cut from its left end, blue,
//   blinking toward red at half health and below (faster as it falls), and
//   flashing white when hurt. Under it a second, smaller bar shows what the
//   soldier has besides health, with its icon: a jetpack's fuel is blue, red
//   below what lighting it takes, and pulsing red if it gets that low in
//   flight. Under that, red, the health of the enemy his aim is on, fading
//   for 2 s once it is off.
// - Both groups fade to 40% after 3 s with nothing new and come back on a
//   change.
// - Reinforcements, top centre at 1.7x and 80% alpha: each team's bar and
//   count; a team that is bleeding blinks toward white. Below it, the
//   countdown for a team holding no posts.
// - The reticle sits on the screen's vertical centre line at the height of
//   the point 1024 m along the aim, at its texture's size, dimmed while the
//   weapon waits between shots, reloads, or cools.
// - Capture, right edge: hud_flag_timer filled in the colours of the teams
//   taking the post the soldier stands in.
// - The minimap, bottom left: the map at half alpha through a round mask,
//   125 m to its rim (out to 250 m as what he controls speeds up), turned
//   so the soldier (or his seat) faces up, with the posts in relation
//   colours pinned inside the rim. M shows the whole map instead.
// - Messages, lower right: five lines at most, newest at the bottom, 10 s
//   each.
// - Damage direction: a bar at the screen edge on the side an explosion hit
//   from, fading over a second.
// - Outside the battlefield: LEAVING BATTLEFIELD flashing red in the middle
//   of the screen (src/battlefield.js).
// - Stats, left middle: the player's KILLS, DEATHS and CP POINTS, faint at
//   rest, for 3 s after one changes, the changed row flashing yellow or red.
// Dead, the player has no body: until the spawn screen only the
// reinforcements, the messages and the stats show.
// - Zoomed in first person, the weapon's scope overlay: a quarter of it per
//   screen quarter, mirrored about the centre.
// - Seated (a vehicle's seat, a craft's as any other, or a turret), the
//   same displays, read from the seat this page knows he holds each frame:
//   the seat's two channels in the weapons panels, named for the seat
//   (PILOT, MAIN GUNNER...) when he takes it; his own bar still on top and
//   what he sits in on the second panel, with a pip per seat of the hull
//   (his white, anyone else's green); both channels' reticles at one anchor,
//   each in the colour of what its own weapon is on; no capture timer. A
//   craft's pilot gets the port's own takeoff and landing prompts where
//   the original game's tool tips go.
//
// A layer (setLayer) can take the screen over, as the spawn screen
// (src/spawn-screen) and the scoreboard (src/scoreboard) do: the player's
// own displays and the messages hide, the reinforcements count from the
// layer's team (or hide), the map shows the layer's battlefield, and the
// layer draws its own quads and 3D models around the HUD's. An overlay
// (setOverlay), the pause menu (src/pause), goes over all of that: under it
// only the layer's 3D passes still draw (the spawn screen's room and body,
// as the original's pause over the spawn screen shows them).

pub mod layout;
mod map;
mod panels;
mod radar;
use crate::interface::*;
use layout::{
    HudArt as A, HudCommand as C, HudState as S, HudTeam as T, HudFrame as H, HudMapDraw as M, HudPost as P, HudPostCount as PC, HudRing as R,
    HudUnit as U, HudWeapon as W,
};
use std::f64::consts::PI;
use std::fmt::Write;

// The flat block has fixed regions, with row widths supplied by the schema.
// A browser and a native client therefore feed the same model without object
// identity, a string heap or callbacks on the frame boundary. Latest-value
// state rows have dirty bits; real events enter the model synchronously.
pub const MAX_POSTS: usize = 128;
pub const MAX_UNITS: usize = 2048;
pub const MAX_TEAMS: usize = 32;
pub const WEAPONS: usize = H::WORDS;
pub const POSTS: usize = WEAPONS + 4 * W::WORDS;
pub const POST_COUNTS: usize = POSTS + MAX_POSTS * P::WORDS;
pub const UNITS: usize = POST_COUNTS + MAX_POSTS * MAX_TEAMS * PC::WORDS;
pub const TEAMS: usize = UNITS + MAX_UNITS * U::WORDS;
pub const STATES: usize = TEAMS + MAX_TEAMS * T::WORDS;
pub const MAP_DRAW: usize = STATES + (C::KILL as usize + 1) * S::WORDS;
pub const RINGS: usize = MAP_DRAW + M::WORDS;
pub const INPUT_LEN: usize = RINGS + 128 * R::WORDS;
pub const WHITE: Color = [1.; 4];
pub const SHADOW: Color = [0., 0., 0., 207. / 255.];
pub const GREEN: Color = [32. / 255., 223. / 255., 32. / 255., 1.];
pub const RED: Color = [223. / 255., 32. / 255., 32. / 255., 1.];
pub const GRAY: Color = [192. / 255., 192. / 255., 192. / 255., 1.];
pub const YELLOW: Color = [223. / 255., 223. / 255., 32. / 255., 1.];
pub const BAR: Color = [0., 152. / 255., 1., 1.];
pub const LOW: Color = [203. / 255., 57. / 255., 64. / 255., 1.];
pub const RETICLE: Color = [67. / 255., 167. / 255., 242. / 255., 1.];
pub fn relation(n: f64) -> Color {
    match n as u32 {
        1 => GREEN,
        2 => RED,
        3 => YELLOW,
        4 => WHITE,
        _ => GRAY,
    }
}
pub fn clamp(n: f64) -> f64 {
    n.clamp(0., 1.)
}
pub fn blink(time: f64, period: f64) -> f64 {
    let into = (time % period) - (period - 0.25);
    if into <= 0. {
        0.
    } else {
        1. - ((into / 0.25) * 2. - 1.).abs()
    }
}

// Asset handles are stable across image upgrades and context loss. These
// named slots match the once-at-load battle art list in the host binding.
pub const HUD1: u32 = A::HUD1;
pub const HUD2: u32 = A::HUD2;
pub const HEALTH: u32 = A::HUD_HEALTH;
pub const HULL: u32 = A::HUD_VEHICLEHEALTH;
pub const CAPTURE: u32 = A::HUD_FLAG_TIMER;
pub const FLAG: u32 = A::HUD_FLAG_ICON;
pub const PLAYER: u32 = A::PLAYER_ICON;
pub const MASK: u32 = A::MAP_MASK;
pub const DAMAGE: u32 = A::AMBUSH_ICON;
pub const HIT: u32 = A::HIT_INDICATOR;
pub const STATS: [u32; 3] = [A::STATS_KILLS, A::STATS_DEATHS, A::STATS_CPP];
pub const SOLID: u32 = A::SOLID;

#[derive(Default)]
struct Fader {
    alpha: f64,
    hold: f64,
}
impl Fader {
    // Fader.wake/update: a wake during drawing starts the hold next update.
    fn wake(&mut self) {
        self.hold = 3.;
    }
    // posts: the CommandPosts; player: { position, yaw, team, dead, speed }
    // (speed: across the ground, m/s, of what he controls); battle:
    // the reinforcement counts, starting counts, bleed rates, and countdown.
    // How long the craft's prompt has shown, from when it first applied.
    // Over a layer's battlefield the radar is that team's, with no soldier
    // of its own to see from.
    fn update(&mut self, dt: f64) {
        if self.hold > 0. {
            self.alpha = (self.alpha + 8. * dt).min(1.);
            self.hold -= dt;
        } else {
            self.alpha = (self.alpha - (1. / 7.) * dt).max(0.4);
        }
    }
}
#[derive(Clone, Copy)]
// message/update: only five feed entries are visible. Ordinary lines retain
// an immutable text handle; formatted kills use the inline UTF-16 buffer.
// Neither creates a String while scrolling or expiring. A new line restarts
// scrolling without restarting the remaining life of older entries.
struct Message {
    text: usize,
    line: [u16; 512],
    len: usize,
    color: Color,
    age: f64,
}
impl Default for Message {
    fn default() -> Self {
        Self { text: 0, line: [0; 512], len: 0, color: [0.; 4], age: 0. }
    }
}
#[derive(Clone, Copy)]
// RadarUnits.state stores discovery across frames but not across lives.
// A pooled unit can reuse a handle; generation equality controls whether its
// alpha, previous heading and shot memory still belong to that unit's life.
// Shot windows carry both their end and original duration for the flash ramp.
struct RadarState {
    generation: f64,
    alpha: f64,
    yaw: f64,
    shots: [(f64, f64); 32],
}
impl Default for RadarState {
    fn default() -> Self {
        Self { generation: f64::NAN, alpha: 0., yaw: f64::NAN, shots: [(0., 0.); 32] }
    }
}

// BattleHud's display state is independent of the authoritative simulation.
// Health flashes, visibility holds, feed ages and countdown flares use visual
// dt. Weapon lock progress and objective progress are read-only game facts.
// The model also survives a GPU reset: losing buffers must not wake every
// panel, forget radar discovery or restart the message feed.
pub struct HudModel {
    pub input: Vec<f64>,
    pub batch: Batch,
    pub assets: Vec<Asset>,
    pub texts: Vec<Vec<u16>>,
    pub fonts: [Font; 4],
    pub time: f64,
    weapon_fader: Fader,
    health_fader: Fader,
    shown_health: f64,
    health_flash: f64,
    shown_hull: f64,
    hull_flash: f64,
    second_woke: f64,
    named: [f64; 2],
    name_left: [f64; 2],
    numbers: [Label; 2],
    messages: [Message; 5],
    message_count: usize,
    scroll: f64,
    banner: usize,
    banner_color: Color,
    damage: [f64; 4],
    hit: [f64; 3],
    enemy_fill: f64,
    enemy_icon: u32,
    enemy_left: f64,
    stats: [f64; 3],
    has_stats: bool,
    stat_changes: [f64; 3],
    stat_flash: [f64; 3],
    stats_left: f64,
    command: f64,
    command_left: f64,
    countdown_second: f64,
    countdown_alpha: f64,
    expanded: bool,
    prompt: f64,
    prompt_age: f64,
    radar: Vec<RadarState>,
    radar_icons: Vec<radar::Icon>,
}
impl Default for HudModel {
    fn default() -> Self {
        Self::new()
    }
}
impl HudModel {
    // BattleHud.constructor: all frame-sized storage is committed here, before
    // assets are used. Vec capacities are hard limits rather than growth hints:
    // quad construction clips at the existing interface budget, radar stops at
    // 450 eligible markers, and oversized host source lists clip without aborting a frame.
    pub fn new() -> Self {
        Self {
            input: vec![0.; INPUT_LEN],
            batch: Batch::new(),
            assets: vec![Asset::default(); 4096],
            texts: vec![vec![]],
            fonts: std::array::from_fn(|_| Font::default()),
            time: 0.,
            weapon_fader: Fader::default(),
            health_fader: Fader::default(),
            shown_health: f64::NAN,
            health_flash: 0.,
            shown_hull: f64::NAN,
            hull_flash: 0.,
            second_woke: f64::NAN,
            named: [0.; 2],
            name_left: [0.; 2],
            numbers: std::array::from_fn(|_| Label::default()),
            messages: [Message::default(); 5],
            message_count: 0,
            scroll: 0.,
            banner: 0,
            banner_color: WHITE,
            damage: [0.; 4],
            hit: [0.; 3],
            enemy_fill: 0.,
            enemy_icon: 0,
            enemy_left: 0.,
            stats: [0.; 3],
            has_stats: false,
            stat_changes: [0.; 3],
            stat_flash: [0.; 3],
            stats_left: 0.,
            command: 0.,
            command_left: 0.,
            countdown_second: f64::NAN,
            countdown_alpha: 1.,
            expanded: false,
            prompt: 0.,
            prompt_age: 0.,
            radar: vec![RadarState::default(); MAX_UNITS + 1],
            radar_icons: Vec::with_capacity(450),
        }
    }
    // Loading-only methods. Text uses UTF-16 because bitmap lookup in Fonts.draw
    // reads charCodeAt(0), including the first unit of a supplementary glyph.
    pub fn add_text(&mut self, text: &str, uppercase: bool) -> u32 {
        let id = self.texts.len();
        self.texts.push(if uppercase {
            text.to_uppercase().encode_utf16().collect()
        } else {
            text.encode_utf16().collect()
        });
        id as u32
    }
    pub fn asset(&self, id: u32) -> Asset {
        self.assets.get(id as usize).copied().unwrap_or_default()
    }
    // BattleHud.texture: zero is the absent-art handle. A descriptor can exist
    // before its GPU sampler; skipping that descriptor must not select another
    // art asset. Authored health-icon defaults are chosen by the panel itself.
    fn texture(&self, id: u32) -> u32 {
        if id != 0 && self.asset(id).texture >= 0 {
            id
        } else {
            0
        }
    }
    fn at(&self, x: f64, y: f64, safe: bool) -> Matrix {
        anchor(x, y, safe, self.input[H::HEIGHT])
    }
    fn quad(&mut self, m: Matrix, r: [f64; 4], texture: u32, uv: [f64; 4], color: Color) {
        let t = self.texture(texture);
        self.batch.quad(m, r, t, uv, color, 0., 0);
    }
    fn text(
        &mut self,
        m: Matrix,
        x: f64,
        y: f64,
        id: usize,
        font: usize,
        color: Color,
        align: u32,
        scale: f64,
        z: f64,
    ) {
        if let Some(text) = self.texts.get(id) {
            self.fonts[font].draw(&mut self.batch, m, x, y, text, color, align, scale, 1., z);
        }
    }
    fn label(&mut self, m: Matrix, x: f64, y: f64, text: &Label, font: usize, color: Color, align: u32, scale: f64) {
        self.fonts[font].draw(&mut self.batch, m, x, y, text.text(), color, align, scale, 1., 0.);
    }
    // drawWeapons/drawTeams/drawStats all share integer bitmap labels. Writing
    // into the stack label keeps changing ammo and countdown digits off the heap.
    // Alignment is applied by Font to the rounded advances of that exact label.
    fn number(&mut self, m: Matrix, x: f64, y: f64, n: f64, font: usize, color: Color, align: u32, scale: f64) {
        let mut label = Label::default();
        let _ = write!(label, "{n:.0}");
        self.label(m, x, y, &label, font, color, align, scale);
    }

    // BattleHud state setters publish their latest values in retained rows.
    // Wakes precede health comparisons, as when taking a new seat. Repeated
    // loading previews therefore occupy one row each, independent of frame rate.
    pub fn commands(&mut self) {
        let mask = self.input[H::STATES] as u32;
        self.input[H::STATES] = 0.;
        for kind in 1..=C::KILL {
            if mask & (1 << kind) != 0 {
                let o = STATES + kind as usize * S::WORDS + S::VALUES;
                self.event(kind, self.input[o], self.input[o + 1], self.input[o + 2], self.input[o + 3]);
            }
        }
    }
    // Real events are consumed synchronously, even during a long load with no
    // update or draw. There is no frame-sized queue to fill: messages retain the
    // original last five lines, hits accumulate and shot windows update in place.
    pub fn event(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64) {
        match kind {
            C::WAKE => {
                self.weapon_fader.wake();
                self.health_fader.wake();
                self.shown_health = f64::NAN;
                self.shown_hull = f64::NAN;
            }
            // The channels' weapons (his own, or his seat's or turret's) and his own
            // health, 0..1, with its icon (hud_health: the original game never shows the
            // class's own there), seated or not (setHull shows what he sits in). A
            // change of more than 1% wakes the health group; a loss flashes the bar.
            C::HEALTH => {
                if self.shown_health.is_nan() || (a - self.shown_health).abs() > 0.01 {
                    if !self.shown_health.is_nan() && a < self.shown_health {
                        self.health_flash = 1.;
                    }
                    self.shown_health = a;
                    self.health_fader.wake();
                }
            }
            // The health group's second bar: an icon texture name, a fill 0..1, and
            // how it looks ("low" solid red, "pulse" pulsing toward red), or null to
            // hide it. A change of more than 1% wakes the group.
            C::SECOND => {
                if a != 0. && (self.second_woke.is_nan() || (clamp(b) - self.second_woke).abs() > 0.01) {
                    self.health_fader.wake();
                    self.second_woke = clamp(b);
                }
            }
            // What he sits in, under his own health (the original game's health group
            // adds it for any controlled thing but the soldier: a vehicle, from any
            // of its seats, or a turret): its HealthTexture (hud_vehiclehealth for a
            // class with none) and its health, 0..1, or no icon off a seat. It
            // blinks and flashes as his own bar does.
            C::HULL => {
                if a == 0. {
                    self.shown_hull = f64::NAN;
                } else {
                    let f = clamp(b);
                    if self.shown_hull.is_nan() || (f - self.shown_hull).abs() > 0.01 {
                        if !self.shown_hull.is_nan() && f < self.shown_hull {
                            self.hull_flash = 1.;
                        }
                        self.shown_hull = f;
                        self.health_fader.wake();
                    }
                }
            }
            // A droid serving the soldier: whether it is giving health, and ammo.
            // While it is, the health bar (and the rounds) pulse green and stay shown.
            C::SUPPLY => {
                if a != 0. {
                    self.health_fader.wake();
                }
                if b != 0. {
                    self.weapon_fader.wake();
                }
            }
            C::ENEMY => {
                self.enemy_fill = clamp(a);
                self.enemy_icon = b as u32;
                self.enemy_left = 2.;
                self.health_fader.wake();
            }
            // The player's Character's numbers, alive or dead: KILLS (kills - 2 x team
            // kills), DEATHS and CP POINTS. A change flashes that row and shows the
            // popup.
            C::STATS => {
                let next = [a - 2. * b, c, d];
                if self.has_stats {
                    let mut changed = false;
                    for j in 0..3 {
                        if next[j] != self.stats[j] {
                            self.stat_changes[j] = (next[j] - self.stats[j]).signum();
                            self.stat_flash[j] = 1.;
                            changed = true;
                        }
                    }
                    if changed {
                        self.stats_left = 3.;
                    }
                }
                self.stats = next;
                self.has_stats = true;
            }
            C::MESSAGE => {
                self.push_message(Message { text: a as usize, color: relation(b), ..Message::default() });
            }
            C::KILL => self.kill_message(a as usize, b as usize, c as usize, relation(d)),
            C::BANNER => {
                self.banner = a as usize;
                self.banner_color = relation(b);
            }
            // An explosion hurt the soldier from `angle` radians off his facing
            // (positive to his right).
            C::DAMAGE => {
                let angle = a.sin().atan2(a.cos());
                let side = if angle.abs() <= PI / 4. {
                    3
                } else if angle.abs() >= 3. * PI / 4. {
                    1
                } else if angle > 0. {
                    0
                } else {
                    2
                };
                self.damage[side] = 1.;
            }
            C::HIT => {
                let i = match a as u32 {
                    1 => 1,
                    2 | 3 => 0,
                    _ => 2,
                };
                self.hit[i] += 0.5;
            }
            C::SHOT => self.shot(a as usize, b, c),
            C::SQUAD => {
                self.command = a;
                self.command_left = 2.;
            }
            C::TOGGLE_MAP => self.expanded = !self.expanded,
            _ => {}
        }
    }

    fn push_message(&mut self, message: Message) {
        if self.message_count == 5 {
            self.messages.copy_within(1..5, 0);
            self.message_count = 4;
        }
        self.messages[self.message_count] = message;
        self.message_count += 1;
        self.scroll = 20.;
    }
    // killMessage substitutes names in the localized template without building
    // a frame String. A name containing a percent sequence is literal content.
    // Overflow is clipped at the bitmap feed's retained 512-character capacity.
    fn kill_message(&mut self, kind: usize, killer: usize, victim: usize, color: Color) {
        let template = self.input[H::KILL_TEMPLATES + kind.min(3)] as usize;
        let mut message = Message { color, ..Message::default() };
        if let Some(text) = self.texts.get(template) {
            let mut i = 0;
            let mut arg = 0;
            while i < text.len() && message.len < 512 {
                if i + 1 < text.len() && text[i] == 37 && text[i + 1] == 83 {
                    let id = if kind < 2 || arg > 0 { victim } else { killer };
                    arg += 1;
                    i += 2;
                    if let Some(name) = self.texts.get(id) {
                        for &ch in name {
                            if message.len == 512 {
                                break;
                            }
                            message.line[message.len] = ch;
                            message.len += 1;
                        }
                    }
                } else {
                    message.line[message.len] = text[i];
                    message.len += 1;
                    i += 1;
                }
            }
        }
        self.push_message(message);
    }

    // BattleHud.update: all fade clocks advance even if no frame is drawn.
    // Filtering messages compacts a fixed array; no frame allocates a list.
    pub fn update(&mut self) {
        self.commands();
        let dt = self.input[H::DT];
        self.time += dt;
        // Fader.update precedes flash/name expiry, matching BattleHud.update.
        // Drawing can wake a fader after this step; that wake affects the next
        // update, while the current geometry uses the alpha already sampled here.
        self.weapon_fader.update(dt);
        self.health_fader.update(dt);
        self.health_flash = (self.health_flash - dt / 0.5).max(0.);
        self.hull_flash = (self.hull_flash - dt / 0.5).max(0.);
        let prompt = self.prompt_line();
        if prompt != self.prompt {
            self.prompt = prompt;
            self.prompt_age = 0.;
        } else if prompt != 0. {
            self.prompt_age += dt;
        }
        for n in &mut self.name_left {
            *n = (*n - dt).max(0.);
        }
        self.scroll = (self.scroll - 40. * dt).max(0.);
        // Message expiry is stable compaction: insertion order remains the draw
        // order, and removing an old line never changes another line's age. The
        // scroll offset is a separate scalar shared by every surviving line.
        let mut n = 0;
        for i in 0..self.message_count {
            let mut line = self.messages[i];
            line.age += dt;
            if line.age < 10. {
                self.messages[n] = line;
                n += 1;
            }
        }
        self.message_count = n;
        for a in &mut self.damage {
            *a = (*a - dt).max(0.);
        }
        self.countdown_alpha = (self.countdown_alpha - 2. * dt).max(0.5);
        for t in &mut self.hit {
            *t = (*t - dt).max(0.);
        }
        self.enemy_left = (self.enemy_left - dt).max(0.);
        self.stats_left = (self.stats_left - dt).max(0.);
        for t in &mut self.stat_flash {
            *t = (*t - dt).max(0.);
        }
        self.command_left = (self.command_left - dt).max(0.);
        // Eligibility and discovery sample simulation positions here. Map drawing
        // later samples visual positions without testing sensors again, so a smooth
        // interpolated body cannot reveal itself by crossing a sensor on screen.
        self.update_radar(dt);
    }

    // flightPrompt/promptLine selects from the craft's replicated state. A
    // refusal takes priority over takeoff and landing hints, and only the
    // pilot sees any prompt. Text handles include the current bound key.
    fn prompt_line(&self) -> f64 {
        if self.input[H::FLIGHT_PILOT] == 0. {
            return 0.;
        }
        let warning = self.input[H::FLIGHT_WARNING] as usize;
        let index = if warning > 0 {
            warning + 1
        } else if self.input[H::FLIGHT_STATE] == 1. {
            0
        } else if self.input[H::FLIGHT_STATE] == 2. && self.input[H::FLIGHT_CAN_LAND] != 0. {
            1
        } else {
            return 0.;
        };
        self.input[H::PROMPT_TEXTS + index]
    }

    // BattleHud.render's own groups. Host layers still draw their independent
    // surfaces around this batch; an overlay that covers HUD emits no groups.
    pub fn build(&mut self, mode: u32) {
        self.commands();
        self.batch.begin();
        // The shell holomap requests only battlefield geometry with its own matrix.
        // It must not advance the HUD clock or run the player's panels a second time.
        // Mode one similarly emits only the banner over a layer's own surface.
        if mode == 2 {
            self.battlefield();
            return;
        }
        if self.input[H::COVERED] != 0. {
            return;
        }
        if mode == 1 {
            self.draw_banner();
            return;
        }
        if self.input[H::LAYER] != 0. {
            if self.input[H::SPAWN_MAP] != 0. {
                self.spawn_map();
            }
            if self.input[H::HIDE_TEAMS] == 0. {
                self.teams();
            }
            return;
        }
        // A dead player has no health, weapons, reticle or minimap in the original
        // HUD. Reinforcements, messages and the death-triggered stat popup remain.
        // Layer and covering-overlay precedence was resolved before reaching this
        // branch, so a spawn map never inherits a dead soldier's personal displays.
        if self.input[H::DEAD] != 0. {
            self.teams();
            self.messages();
            self.stats();
        } else {
            self.scope();
            self.map();
            self.teams();
            self.weapons();
            self.health();
            self.flight_prompt();
            self.capture();
            self.messages();
            self.damage();
            self.leaving();
            self.reticle();
            self.hit_ring();
            self.lock_diamond();
            self.stats();
            self.squad();
        }
        self.draw_banner();
    }
    fn draw_banner(&mut self) {
        if self.banner != 0 {
            self.text(self.at(0.5, 0.35, true), 0., 0., self.banner, 3, self.banner_color, 2, 1.5, 0.);
        }
    }
}

#[cfg(test)]
mod tests;
