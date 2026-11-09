//! Page battle compositor. The existing controllers own actions and the HUD
//! owns map contents; this host supplies the independent preview camera/target.
pub(crate) mod appearance;
mod cache;
mod director;
mod gpu;
mod rig;
mod textures;
use super::settings::AppSettingsStore;
use crate::scene::Scene;
use bfe_battle_ui::{
    shapes::{root_anchor, scale_xyz},
    spawn::{ClassicSpawnView, MapProjection, SpawnHitTarget, SpawnModel},
};
use bfe_render::{
    hud as draw,
    hud_model::{layout::HudMapDraw as M, MAP_DRAW},
};
use bfe_shell::page::{BattleLook, SpawnLayout};
use glow::HasContext;
pub(super) struct PageBattle {
    pub cache: cache::Cache,
    pub spawn: bool,
    pub scoreboard: bool,
    rig: rig::Rig,
    gpu: gpu::Gpu,
    was_open: bool,
    class: String,
    team: u8,
    pub layout: SpawnLayout,
    drag: [f64; 2],
    pub dt: f64,
    building: f64,
    pulse: f64,
    walker: bool,
    strength: f64,
    pub age: f64,
    pub class_age: f64,
    pub team_age: f64,
    pub class_id: usize,
    framed: bool,
    framing_look: usize,
    logo_spin: f64,
    logo_swap: f64,
    cursor: [f64; 2],
    pin_heights: Vec<f64>,
    director: director::Director,
    post: Option<String>,
    pub shot: Option<bfe_battle_ui::spawn::CameraShot>,
}
/// An explicitly named view wins. Unknown names retain the browser's Classic
/// fallback; no override follows the front end that launched the battle.
fn page_view(front: &str, override_name: &str) -> bool {
    if override_name.is_empty() {
        !matches!(front, "classic" | "original")
    } else {
        override_name == "page"
    }
}
impl PageBattle {
    pub fn new(
        scene: &mut Scene,
        size: [i32; 2],
        settings: &AppSettingsStore,
    ) -> Result<Self, String> {
        let front = &settings.player.shell.menu_theme;
        let models = std::mem::take(&mut scene.menu_models);
        let gpu = gpu::Gpu::new(
            &mut scene.bridge,
            &models,
            scene.menu_grid.ok_or("grid room texture is absent")?,
        )?;
        scene.menu_models.clear();
        scene.bridge.r.core.state.reset();
        Ok(Self {
            cache: cache::Cache::new(),
            spawn: page_view(front, &settings.interface.get_string("spawnScreenView")),
            scoreboard: page_view(front, &settings.interface.get_string("scoreboardView")),
            rig: rig::Rig::new(),
            gpu,
            was_open: false,
            class: String::new(),
            team: 0,
            layout: SpawnLayout::new(size[0] as f64, size[1] as f64, 2),
            drag: [0.; 2],
            dt: 0.,
            building: -0.35,
            pulse: 1.,
            walker: false,
            strength: 0.,
            age: 0.,
            class_age: 0.,
            team_age: 0.,
            class_id: 0,
            framed: false,
            framing_look: 2,
            logo_spin: 0.,
            logo_swap: 0.,
            cursor: [size[0] as f64 * 0.5, size[1] as f64 * 0.5],
            pin_heights: Vec::new(),
            director: director::Director::new(scene.spawn_shots.clone()),
            post: None,
            shot: None,
        })
    }
    pub fn dragging(&self) -> bool {
        self.rig.dragging
    }
    /// The map is drawn as the selected team sees it. These UI relations
    /// are refreshed from authority facts before the next ordinary HUD frame.
    pub fn map_team(hud: &mut bfe_render::hud_model::HudModel, team: u8) {
        use bfe_render::hud_model::{
            layout::{HudFrame as H, HudPost as P, HudPostCount as PC, HudTeam as T},
            MAX_POSTS, MAX_TEAMS, POSTS, TEAMS,
        };
        let input = &mut hud.input;
        input[H::VIEWER_TEAM] = team as f64;
        let relation = |other: f64| {
            if other == 0. {
                0.
            } else if other == team as f64 {
                1.
            } else {
                -1.
            }
        };
        for other in 0..MAX_TEAMS {
            input[TEAMS + other * T::WORDS + T::RELATION] = relation(other as f64);
        }
        for index in 0..(input[H::POSTS] as usize).min(MAX_POSTS) {
            let at = POSTS + index * P::WORDS;
            input[at + P::RELATION] = relation(input[at + P::TEAM]);
            input[at + P::TOWARD_RELATION] = relation(input[at + P::TOWARD]);
            let first = input[at + P::COUNTS] as usize;
            for i in 0..(input[at + P::COUNTS_LENGTH] as usize).min(MAX_TEAMS) {
                let at = first + i * PC::WORDS;
                input[at + PC::RELATION] = relation(input[at + PC::TEAM]);
            }
        }
    }
    pub fn turned(&self) -> bool {
        self.rig.turned
    }
    pub fn closed(&mut self) {
        if self.was_open {
            eprintln!("page spawn view closed");
        }
        self.was_open = false;
        self.strength = 0.;
        self.rig.dragging = false;
    }
    pub fn update(
        &mut self,
        dt: f64,
        model: &SpawnModel,
        port: &super::battle::LiveSpawnPort,
        size: [i32; 2],
        theme: &str,
        countdown: bool,
    ) {
        self.dt = dt;
        let goals = port.teams[usize::from(model.team == 2)]
            .objectives
            .iter()
            .filter(|s| !s.is_empty())
            .count();
        let previous_layout = self.layout;
        self.layout =
            SpawnLayout::new(size[0] as f64, size[1] as f64, goals).with_countdown(countdown);
        let opening = !self.was_open;
        if opening {
            eprintln!(
                "page spawn view opened: team={} class={} post={}",
                model.team,
                model.class_index,
                model.post.as_deref().unwrap_or("none")
            );
            self.age = 0.;
            self.logo_swap = 0.;
            self.rig.open();
            self.building = -0.35;
            self.pulse = 1.;
            self.director.open();
            self.post = None;
            self.was_open = true;
        }
        self.class_id = port.class_registry_ids[usize::from(model.team == 2)]
            .get(model.class_index)
            .copied()
            .unwrap_or(0);
        let class = port.rosters[usize::from(model.team == 2)]
            .get(model.class_index)
            .map_or("", |c| c.class_name.as_str());
        if self.team != model.team {
            self.team = model.team;
            self.team_age = 0.;
            self.logo_swap = 0.;
            if !opening {
                self.rig.changed(true);
            }
        }
        if self.class != class {
            self.class_age = 0.;
            if !opening {
                self.building = 0.;
            }
            self.pulse = 1.;
            self.walker = class.to_ascii_lowercase().contains("droideka");
            self.class.clear();
            self.class.push_str(class);
            if !opening {
                self.rig.changed(false);
            }
        }
        if self.post != model.post {
            self.post = model.post.clone();
            if let Some(post) = port
                .posts
                .iter()
                .find(|p| Some(p.name.as_str()) == self.post.as_deref())
            {
                self.director.follow(post.position);
            }
        }
        self.age += dt;
        self.class_age += dt;
        self.team_age += dt;
        self.strength += (1. - self.strength) * (dt * 3.5).min(1.);
        self.building += dt;
        self.pulse = (self.pulse - dt * 1.6).max(0.);
        self.shot = self.director.update(dt);
        let look = BattleLook::new(theme).index;
        if !self.framed || previous_layout != self.layout || self.framing_look != look {
            self.rig.frame(self.layout, theme == "bf2");
            self.framed = true;
            self.framing_look = look;
        }
        self.logo_swap = (self.logo_swap + dt).min(0.4);
        self.logo_spin += if self.logo_swap < 0.4 {
            (-720f64).to_radians() * dt
        } else {
            (-14f64).to_radians() * dt
        };
        self.pin_heights.resize(port.posts.len(), 0.07);
        self.rig.update(dt, size[0] as f64 / size[1] as f64);
    }
    pub fn pointer(&mut self, p: [f64; 2]) {
        self.cursor = p;
        self.rig.pointer(
            p[0] / self.layout.width * 2. - 1.,
            1. - p[1] / self.layout.height * 2.,
        );
        if self.rig.dragging {
            let k = 1080. / self.layout.height;
            self.rig.drag(
                (p[0] - self.drag[0]) * k,
                (p[1] - self.drag[1]) * k,
                self.dt,
            );
            self.drag = p;
        }
    }
    pub fn button(&mut self, p: [f64; 2], button: u8, down: bool) -> bool {
        if button != 0 {
            return false;
        }
        if !down {
            let was = self.rig.dragging;
            self.rig.release();
            return was;
        }
        let r = self.layout.figure;
        if p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3] {
            self.rig.dragging = true;
            self.rig.turned = true;
            self.drag = p;
            true
        } else {
            false
        }
    }
    /// The battlefield pass builds the same 360-unit map that the Page
    /// browser projects. Its temporary geometry never replaces the UI batch.
    pub fn map_texture(&self, scene: &mut Scene, _view: &ClassicSpawnView, ih: f64) {
        let bridge = &mut scene.bridge;
        let saved: [f64; M::WORDS] = bridge.hud_model.input[MAP_DRAW..MAP_DRAW + M::WORDS]
            .try_into()
            .unwrap();
        let mut m = bfe_render::interface::translate(root_anchor(ih), 512., ih * 0.5, 0.);
        m = scale_xyz(m, 1024. / 360., ih / 360., 1.);
        let values = &mut bridge.hud_model.input;
        values[MAP_DRAW..MAP_DRAW + M::WORDS].fill(0.);
        values[MAP_DRAW + M::MATRIX..MAP_DRAW + M::MATRIX + 16].copy_from_slice(&m);
        values[MAP_DRAW + M::SIZE] = 360.;
        values[MAP_DRAW + M::LABELS] = 1.;
        values[MAP_DRAW + M::SPAWN] = 1.;
        values[MAP_DRAW + M::RINGS] = saved[M::RINGS];
        values[MAP_DRAW + M::MASK] = scene
            .online
            .as_ref()
            .and_then(|r| r.hud.names.get("map_mask").copied())
            .unwrap_or(0) as f64;
        bridge.hud_model.build(2);
        bridge.hud_sync_batch(true);
        let viewport = [
            bridge.r.hud.input[draw::IN_VIEWPORT],
            bridge.r.hud.input[draw::IN_VIEWPORT + 1],
        ];
        bridge.r.hud.input[draw::IN_VIEWPORT] = 1024.;
        bridge.r.hud.input[draw::IN_VIEWPORT + 1] = 1024.;
        unsafe {
            let gl = &bridge.r.core.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.gpu.target));
            gl.viewport(0, 0, 1024, 1024);
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0., 0., 0., 0.);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
        bridge.r.core.state.reset();
        bridge
            .r
            .hud
            .draw_to(&mut bridge.r.core, Some(self.gpu.target));
        bridge.r.hud.input[draw::IN_VIEWPORT..draw::IN_VIEWPORT + 2].copy_from_slice(&viewport);
        bridge.hud_model.input[MAP_DRAW..MAP_DRAW + M::WORDS].copy_from_slice(&saved);
        unsafe {
            bridge
                .r
                .core
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(scene.canvas.framebuffer));
            bridge
                .r
                .core
                .gl
                .viewport(0, 0, scene.size[0], scene.size[1]);
        }
        bridge.r.core.state.reset();
    }
    pub fn post_regions(
        &self,
        model: &SpawnModel,
        port: &super::battle::LiveSpawnPort,
        projection: MapProjection,
        ih: f64,
        regions: &mut Vec<([f64; 4], SpawnHitTarget)>,
    ) {
        for (index, post) in port.posts.iter().enumerate() {
            if post.owner != model.team {
                continue;
            }
            let x = (post.position[0] - projection.map_x) / projection.map_half;
            let y = -(post.position[2] - projection.map_z) / projection.map_half;
            let p = self.rig.map_point(x, y, self.layout);
            let reach = 12.;
            regions.push((
                self.layout
                    .interface([p[0] - reach, p[1] - reach, p[0] + reach, p[1] + reach], ih),
                SpawnHitTarget::PostIndex(index),
            ));
        }
    }
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        class: usize,
        theme: &str,
        projection: MapProjection,
        model: &SpawnModel,
        port: &super::battle::LiveSpawnPort,
    ) {
        let look = BattleLook::new(theme);
        let aspect = self.layout.width / self.layout.height;
        self.gpu.before(
            &mut scene.bridge,
            &self.rig,
            aspect,
            look.index,
            self.strength,
        );
        unsafe {
            scene.bridge.r.core.gl.depth_mask(true);
            scene.bridge.r.core.gl.clear(glow::DEPTH_BUFFER_BIT);
        }
        self.gpu.projector(
            &mut scene.bridge,
            &self.rig,
            look.index,
            self.strength,
            self.pulse,
            self.dt,
        );
        let scale = if self.walker { 0.9 } else { 1. };
        if let Some(online) = &mut scene.online {
            let records = online.preview(
                &mut scene.bridge,
                class,
                &self.rig.parent(scale),
                self.dt,
                &self.rig.inverse,
                &self.rig.projection,
                look.index,
                self.building,
            );
            scene.bridge.r.core.state.reset();
            scene
                .bridge
                .r
                .interface
                .draw(&mut scene.bridge.r.core, records);
        }
        self.gpu.map(
            &mut scene.bridge,
            &self.rig,
            aspect,
            look.index,
            self.strength,
        );
        for (index, post) in port.posts.iter().enumerate() {
            let x = (post.position[0] - projection.map_x) / projection.map_half;
            let y = -(post.position[2] - projection.map_z) / projection.map_half;
            let picked = model.post.as_deref() == Some(post.name.as_str());
            let own = post.owner == model.team;
            let point = self.rig.map_point(x, y, self.layout);
            let hovered = (point[0] - self.cursor[0]).hypot(point[1] - self.cursor[1]) < 12.;
            let target = if picked {
                0.42
            } else if hovered {
                0.26
            } else if own {
                0.12
            } else {
                0.07
            };
            let height = &mut self.pin_heights[index];
            *height += (target - *height) * (self.dt * 9.).min(1.);
            self.gpu.pin(
                &mut scene.bridge,
                &self.rig,
                x,
                y,
                *height,
                if own {
                    [0.2, 1., 0.2, 1.]
                } else {
                    [1., 0.2, 0.2, 1.]
                },
            );
        }
        let sides = &port.teams;
        self.gpu.logos(
            &mut scene.bridge,
            &self.rig,
            self.layout,
            model.team,
            self.logo_spin,
            [sides[0].side.as_str(), sides[1].side.as_str()],
        );
        scene.bridge.r.core.state.reset();
    }
    pub fn direct(&self, view: &mut crate::scene::View) {
        if let Some(shot) = self.shot {
            view.values[16..32].copy_from_slice(&director::inverse(shot));
            if let Some(index) = bfe_render::shader::FRAME_MEMBERS
                .iter()
                .position(|(name, _)| *name == "cameraPosition")
            {
                let at = bfe_render::shader::FRAME_OFFSETS[index];
                view.values[at..at + 3].copy_from_slice(&shot.position);
            }
        }
    }
    pub fn dispose(&self, scene: &Scene) {
        self.gpu.dispose(&scene.bridge.r.core.gl);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn view_selection_retains_explicit_classic() {
        assert!(super::page_view("eternal", ""));
        assert!(super::page_view("bf1", ""));
        assert!(!super::page_view("classic", ""));
        assert!(!super::page_view("eternal", "classic"));
        assert!(super::page_view("classic", "page"));
        assert!(!super::page_view("eternal", "unknown"));
    }
}
