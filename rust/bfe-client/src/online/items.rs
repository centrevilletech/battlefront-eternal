//! EntityItemPresentation borrows the shared Items flight/contact owner. The
//! native host supplies collision answers and existing model handles only.
use super::{models::Model, receiver::Receiver, replay::Collision};
use crate::render_bridge::Bridge;
use bfe_content_native::BattleContent;
use bfe_presentation::{
    items::Items,
    items_layout::{
        item_answer as a, item_definition as d, item_frame as f, item_header as h, item_limits as limit, item_record as r,
        item_request as q, item_view as v,
    },
    network_layout::{net_kind as kind, net_record as net},
};
use three::{Matrix4, Quaternion, Vector3};
mod ride;

fn riding_radius(definition: &[f64]) -> Option<f64> {
    (definition[d::MINE] != 0.).then_some(definition[d::RADIUS])
}
// ItemPresentation retains its own fall, bob and water clocks. Wire records
// admit or retire qualified items; a renderer recipe is only a resource
// prerequisite. The host never integrates an independent falling body.
// Mines and power-ups therefore share the client's numerical item owner.
pub struct Views {
    pub owner: Items,
    definitions: Vec<Option<usize>>,
    models: Vec<Option<Model>>,
    // Each definition's steady light (LightColor as 0..1 times its alpha,
    // LightRadius), none when the radius is 0.
    lights: Vec<Option<([f64; 3], f64)>>,
    // Each item definition's looping TrailEffect name (e.g. blinking detpack/timebomb).
    trails: Vec<Option<String>>,
    /// Item lights added, summed over frames.
    pub lit: u64,
    remote_models: Vec<Option<Model>>,
    remote_trails: Vec<Option<String>>,
    remote_guided: Vec<bool>,
    // A guided remote's engine (flame and contrail scale) and its ContrailEffect, by definition.
    remote_engines: Vec<Option<super::combat::swbf2::RemoteEngine>>,
    remote_contrails: Vec<Option<String>>,
    remotes: Vec<(usize, u32, u32, [f64; 3], f64, f64)>,
    // Active looping effects attached to live mines and flying guided remotes.
    mine_fx: Vec<((u32, u32), u32, bool)>,
    remote_fx: Vec<((u32, u32), u32, bool)>,
    contrail_fx: Vec<((u32, u32), u32, bool)>,
    reset_fx: bool,
    pub hide_piloted_remote: bool,
    /// The connection epoch the owner was last reset for.
    epoch: Option<f64>,
    /// By item id: the generation of the active row last forwarded, and
    /// whether this frame's table still holds a row for the id.
    lives: Vec<(Option<f64>, bool)>,
    collision: Collision,
    world: u32,
    water: Option<bfe_sim::units::water::WaterState>,
    last_water: f64,
    pub visible: usize,
    // Mines riding hulls, worked out from the records (ride.rs).
    riders: ride::Riders,
    family: bfe_family::GameFamily,
}

fn light_color_released(def: &bfe_content::Value) -> [f64; 3] {
    let color = def.get("LightColor").and_then(bfe_content::Value::as_array);
    let channel = |k: usize, default: f64| color.and_then(|c| c.get(k)).and_then(bfe_content::Value::as_f64).unwrap_or(default);
    let alpha = channel(3, 255.) / 255.;
    std::array::from_fn(|k| channel(k, 0.) / 255. * alpha)
}

fn light_color_swbf2(def: &bfe_content::Value) -> [f64; 3] {
    let parsed_color: Vec<f64> = match def.get("LightColor") {
        Some(bfe_content::Value::Array(arr)) => arr.iter().map(|v| v.as_f64().unwrap_or(0.)).collect(),
        Some(bfe_content::Value::String(line)) => line.split_whitespace().filter_map(|s| s.parse::<f64>().ok()).collect(),
        _ => Vec::new(),
    };
    let channel = |k: usize, default: f64| parsed_color.get(k).copied().unwrap_or(default);
    let alpha = channel(3, 255.) / 255.;
    std::array::from_fn(|k| channel(k, 0.) / 255. * alpha)
}

fn light_color_for_family(family: bfe_family::GameFamily, def: &bfe_content::Value) -> [f64; 3] {
    match family {
        bfe_family::GameFamily::Swbf1 => light_color_released(def),
        bfe_family::GameFamily::Swbf2 => light_color_swbf2(def),
    }
}

impl Views {
    pub async fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], collision: Collision, world: u32, bounds: bfe_sim::wire::PositionBounds) -> Result<Self, String> {
        let family = content.content.battle.family;
        let swbf2 = family == bfe_family::GameFamily::Swbf2;
        let mut budget = crate::loading_work::Budget::new();
        let shadow_cascades = bridge.shadow_cascades();
        let defs = content.defs()?;
        let mut owner = Items::new();
        let mut definitions = vec![None; defs.registry.combat.len() + 1];
        let mut models = Vec::new();
        let mut lights = Vec::new();
        let mut trails = Vec::new();
        let mut remote_models: Vec<Option<Model>> = (0..=defs.registry.combat.len()).map(|_| None).collect();
        let mut remote_trails: Vec<Option<String>> = vec![None; defs.registry.combat.len() + 1];
        let mut remote_guided: Vec<bool> = vec![false; defs.registry.combat.len() + 1];
        let mut remote_engines = vec![None; defs.registry.combat.len() + 1];
        let mut remote_contrails: Vec<Option<String>> = vec![None; defs.registry.combat.len() + 1];
        for (i, (_, def)) in defs.registry.combat.iter().enumerate() {
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
            let base = def.get("base").and_then(bfe_content::Value::as_str).unwrap_or("");
            if base == "droid" {
                let name = def.get("GeometryName").and_then(bfe_content::Value::as_str).unwrap_or("");
                remote_models[i + 1] = Model::new(content, bridge, textures, name, shadow_cascades)?;
                if swbf2 {
                    remote_trails[i + 1] = def
                        .get("ThrustEffect")
                        .and_then(bfe_content::Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    remote_guided[i + 1] = def
                        .get("GuidedMissile")
                        .and_then(bfe_content::Value::as_f64)
                        .is_some_and(|n| n != 0.);
                    remote_engines[i + 1] = Some(super::combat::swbf2::RemoteEngine::read(def));
                    remote_contrails[i + 1] = def
                        .get("ContrailEffect")
                        .and_then(bfe_content::Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                }
                continue;
            }
            if base != "mine" && base != "powerupitem" {
                continue;
            }
            if models.len() == limit::DEFINITIONS {
                continue;
            }
            let index = models.len();
            definitions[i + 1] = Some(index);
            let name = def.get("GeometryName").and_then(bfe_content::Value::as_str).unwrap_or("");
            let model = Model::new(content, bridge, textures, name, shadow_cascades)?;
            let at = index * d::STRIDE;
            let row = &mut owner.definitions[at..at + d::STRIDE];
            for (at, key, default) in [(d::GRAVITY, "Gravity", 1.), (d::REBOUND, "Rebound", 0.3), (d::FRICTION, "Friction", 0.5)] {
                row[at] = def.get(key).filter(|v| !v.is_null()).map_or(default, |v| bfe_content::js::to_number(Some(v)));
            }
            row[d::MINE] = (base == "mine") as u8 as f64;
            row[d::RADIUS] = bfe_content::derived::item_radius(content.content.weapon_models.get(name)).0;
            row[d::LIGHT_RADIUS] = def.get("LightRadius").and_then(bfe_content::Value::as_f64).unwrap_or(0.).max(0.);
            // color01: a missing colour is black, no light.
            let rgb = light_color_for_family(family, def);
            lights.push((row[d::LIGHT_RADIUS] > 0.).then_some((rgb, row[d::LIGHT_RADIUS])));
            trails.push(
                (swbf2 && base == "mine")
                    .then(|| {
                        def.get("TrailEffect")
                            .and_then(bfe_content::Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
                    .flatten(),
            );
            row[d::MESH] = model.is_some() as u8 as f64;
            owner.define(index);
            models.push(model);
        }
        // WaterState is the existing shared crossing policy and tick bob. Its
        // cells are immutable map content, not another gameplay simulation.
        let water = super::water::new(content);
        for (axis, step) in bounds.step().into_iter().enumerate() {
            owner.frame[f::ROUND_X + axis] = step / 2. + 0.002;
        }
        Ok(Self {
            owner,
            definitions,
            models,
            lights,
            trails,
            lit: 0,
            remote_models,
            remote_trails,
            remote_guided,
            remote_engines,
            remote_contrails,
            remotes: Vec::with_capacity(64),
            mine_fx: Vec::with_capacity(32),
            remote_fx: Vec::with_capacity(16),
            contrail_fx: Vec::with_capacity(16),
            reset_fx: false,
            hide_piloted_remote: false,
            epoch: None,
            lives: vec![(None, false); limit::SLOTS],
            collision,
            world,
            water,
            last_water: 0.,
            visible: 0,
            riders: ride::Riders::new(limit::SLOTS),
            family,
        })
    }
    // Changed item records are applied before sampling. The retained owner
    // can suspend for all map and water queries, consume their answers and
    // resume without returning through an item object or allocating a result.
    // A long packet burst clips to the owner's existing record capacity.
    pub fn frame(&mut self, receiver: &Receiver, tick: f64) {
        // EntityItemPresentation.reset belongs to a new connection epoch. It
        // forgets every life, and a life made again from this frame's row
        // would start ahead of the shown clock and never be drawn.
        if self.epoch != Some(receiver.epoch) {
            self.epoch = Some(receiver.epoch);
            self.owner.reset_epoch(receiver.epoch);
            self.lives.fill((None, false));
            self.reset_fx = true;
        }
        for life in &mut self.lives {
            life.1 = false;
        }
        let mut count = 0;
        for row in receiver.active().filter(|r| r[net::KIND] == kind::ITEM as f64) {
            if count == limit::BATCH {
                self.owner.apply_records(count);
                count = 0;
            }
            let Some(definition) = self.definitions.get(row[net::DEFINITION_ID] as usize).copied().flatten() else {
                continue;
            };
            if let Some(life) = self.lives.get_mut(row[net::ID] as usize) {
                *life = ((row[net::ACTIVE] != 0.).then_some(row[net::GENERATION]), true);
            }
            let x = &mut self.owner.records[count * r::STRIDE..][..r::STRIDE];
            x[r::EPOCH] = receiver.epoch;
            x[r::ID] = row[net::ID];
            x[r::GENERATION] = row[net::GENERATION];
            x[r::TICK] = row[net::TICK];
            x[r::ACTIVE] = row[net::ACTIVE];
            x[r::DEFINITION] = definition as f64;
            x[r::POSITION..r::POSITION + 3].copy_from_slice(&row[net::POSITION..net::POSITION + 3]);
            x[r::VELOCITY..r::VELOCITY + 3].copy_from_slice(&row[net::VELOCITY..net::VELOCITY + 3]);
            x[r::YAW] = row[net::YAW];
            x[r::UP..r::UP + 3].copy_from_slice(&row[net::UP..net::UP + 3]);
            count += 1;
        }
        // A taken or expired item can leave the table in the frame that would
        // have carried its inactive record. End that life as the record would.
        for id in 0..self.lives.len() {
            let (Some(generation), false) = self.lives[id] else {
                continue;
            };
            self.lives[id].0 = None;
            if count == limit::BATCH {
                self.owner.apply_records(count);
                count = 0;
            }
            let x = &mut self.owner.records[count * r::STRIDE..][..r::STRIDE];
            x.fill(0.);
            x[r::EPOCH] = receiver.epoch;
            x[r::ID] = id as f64;
            x[r::GENERATION] = generation;
            x[r::TICK] = receiver.latest;
            count += 1;
        }
        self.owner.apply_records(count);
        if let Some(water) = &mut self.water {
            water.step((tick - self.last_water).max(0.) / 60.);
        }
        self.last_water = tick;
        self.owner.frame[f::TICK] = tick;
        self.owner.frame[f::WATER] = self.water.is_some() as u8 as f64;
        self.owner.present();
        // A frame resumes whole query rounds. The owner's fixed round bound
        // preserves its previous pose if delayed state asks for too much work.
        for _ in 0..limit::ROUNDS {
            let count = self.owner.header[h::REQUESTS] as usize;
            if count == 0 {
                break;
            }
            for i in 0..count.min(limit::SLOTS) {
                let request = &self.owner.requests[i * q::STRIDE..][..q::STRIDE];
                let answer = &mut self.owner.answers[i * a::STRIDE..][..a::STRIDE];
                answer.fill(0.);
                let kind = request[q::KIND] as u32;
                if kind & 2 != 0 {
                    let o = q::ORIGIN;
                    let d = q::DIRECTION;
                    let ray = bfe_sim::collision::Ray {
                        ox: request[o],
                        oy: request[o + 1],
                        oz: request[o + 2],
                        dx: request[d],
                        dy: request[d + 1],
                        dz: request[d + 2],
                    };
                    if let Some((_, hit)) = self.collision.borrow_mut().ray_hit(self.world, ray, request[q::FAR], -1) {
                        answer[a::HIT] = 1.;
                        answer[a::POINT..a::POINT + 3].copy_from_slice(&[hit.px, hit.py, hit.pz]);
                        answer[a::NORMAL..a::NORMAL + 3].copy_from_slice(&[hit.nx, hit.ny, hit.nz]);
                    }
                }
                if kind & 1 != 0 {
                    if let Some(water) = &self.water {
                        let f = q::FROM;
                        let t = q::TO;
                        if let Some(p) = water.crossing(
                            &Vector3::new(request[f], request[f + 1], request[f + 2]),
                            &Vector3::new(request[t], request[t + 1], request[t + 2]),
                        ) {
                            answer[a::WET] = 1.;
                            answer[a::WET_POINT..a::WET_POINT + 3].copy_from_slice(&[p.x, p.y, p.z]);
                        }
                    }
                }
            }
            self.owner.resume(false);
        }
        self.remotes.clear();
        for row in receiver
            .active()
            .filter(|r| r[net::KIND] == kind::REMOTE as f64 && r[net::ACTIVE] != 0. && r[net::HEALTH] > 0.)
        {
            let def_id = row[net::DEFINITION_ID] as usize;
            if self.remote_models.get(def_id).and_then(Option::as_ref).is_none() {
                continue;
            }
            let id = row[net::ID] as u32;
            let gen = row[net::GENERATION] as u32;
            let guided = self.remote_guided.get(def_id).copied().unwrap_or(false);
            let pitch = if guided { row[net::PITCH] } else { 0. };
            if let Some((rid, rgen, _)) = receiver.own.remote {
                if row[net::ID] == rid as f64 && row[net::GENERATION] == rgen as f64 {
                    if !self.hide_piloted_remote {
                        let own_pitch = if guided { receiver.own.remote_pitch } else { 0. };
                        self.remotes.push((def_id, id, gen, receiver.own.remote_position, receiver.own.remote_yaw, own_pitch));
                    }
                    continue;
                }
            }
            self.remotes.push((
                def_id,
                id,
                gen,
                [row[net::POSITION], row[net::POSITION + 1], row[net::POSITION + 2]],
                row[net::YAW],
                pitch,
            ));
        }
        self.visible = self.owner.header[h::VIEWS] as usize + self.remotes.len();
    }
    /// A mine riding a hull is drawn at its place on the hull as this frame
    /// draws the hull (ride.rs); every other item as its copy has it. Call
    /// after the frame's hulls and before the item lights.
    pub fn ride(&mut self, receiver: &Receiver, hulls: &super::hulls::Hulls, dt: f64) {
        let three = |row: &[f64], at: usize| [row[at], row[at + 1], row[at + 2]];
        self.riders.begin();
        // A ground hull's tilt and roll come in its mount record, sent when
        // they change; its own record has its heading alone.
        for field in receiver.publications.iter().filter(|f| f.row[net::KIND] == kind::MOUNT as f64) {
            let row = &field.row;
            self.riders.lean((row[net::ID] as u32, row[net::GENERATION] as u32), row[net::TILT], row[net::MOUNT_ROLL]);
        }
        for row in receiver.active().filter(|r| r[net::KIND] == kind::VEHICLE as f64) {
            let key = (row[net::ID] as u32, row[net::GENERATION] as u32);
            let Some(flying) = hulls.flies(key) else {
                continue;
            };
            let turn = [row[net::QUATERNION], row[net::QUATERNION + 1], row[net::QUATERNION + 2], row[net::QUATERNION + 3]];
            self.riders.hull(key, row[net::TICK], three(row, net::POSITION), turn, flying);
        }
        for row in receiver.active().filter(|r| r[net::KIND] == kind::ITEM as f64) {
            let Some(index) = self.definitions.get(row[net::DEFINITION_ID] as usize).copied().flatten() else { continue };
            let definition = &self.owner.definitions[index * d::STRIDE..][..d::STRIDE];
            let slot = row[net::ID] as usize;
            let Some(radius) = riding_radius(definition) else {
                self.riders.forget(slot);
                continue;
            };
            let still = row[net::VELOCITY..net::VELOCITY + 3].iter().all(|v| *v == 0.);
            let frame = ride::item_frame(three(row, net::POSITION), three(row, net::UP), row[net::YAW]);
            self.riders.item(slot, row[net::GENERATION], row[net::ACTIVE] != 0., row[net::TICK], frame, still, radius);
        }
        self.riders.settle(|key, point, reach| hulls.touches(key, point, reach));
        let views = self.owner.header[h::VIEWS] as usize;
        for view in self.owner.views.chunks_exact_mut(v::STRIDE).take(views) {
            let (p, q) = (v::POSITION, v::QUATERNION);
            let mut at = Vector3::new(view[p], view[p + 1], view[p + 2]);
            let mut turn = Quaternion::new(view[q], view[q + 1], view[q + 2], view[q + 3]);
            // An item with no model has a position alone.
            let oriented = view[q..q + 4].iter().any(|c| *c != 0.);
            self.riders.draw(view[v::SLOT] as usize, view[v::GENERATION], &mut at, &mut turn, dt, |key| hulls.drawn(key));
            view[p..p + 3].copy_from_slice(&[at.x, at.y, at.z]);
            if oriented {
                view[q..q + 4].copy_from_slice(&[turn.x, turn.y, turn.z, turn.w]);
            }
        }
    }
    pub fn record_budget(&self) -> usize {
        self.owner
            .views
            .chunks_exact(v::STRIDE)
            .take(self.owner.header[h::VIEWS] as usize)
            .filter_map(|v| self.models.get(v[v::DEFINITION] as usize).and_then(Option::as_ref))
            .map(|m| m.rows.len())
            .sum::<usize>()
            + self
                .remotes
                .iter()
                .filter_map(|&(def_id, _, _, _, _, _)| self.remote_models.get(def_id).and_then(Option::as_ref))
                .map(|m| m.rows.len())
                .sum::<usize>()
    }
    /// Attach and retire looping particle effects for active mines (`TrailEffect`,
    /// e.g. blinking detpacks and timebombs) and guided remotes (`ThrustEffect`, its
    /// size following the remote's airspeed, and `ContrailEffect`).
    pub fn sync_effects(&mut self, receiver: &Receiver, combat: &mut super::combat::Combat) {
        if self.family != bfe_family::GameFamily::Swbf2 {
            return;
        }
        if self.reset_fx {
            self.reset_fx = false;
            for (_, handle, _) in self.mine_fx.drain(..) {
                combat.stop_effect(handle);
            }
            for (_, handle, _) in self.remote_fx.drain(..).chain(self.contrail_fx.drain(..)) {
                combat.stop_effect(handle);
            }
        }
        for entry in &mut self.mine_fx {
            entry.2 = false;
        }
        for view in self.owner.views.chunks_exact(v::STRIDE).take(self.owner.header[h::VIEWS] as usize) {
            let def_idx = view[v::DEFINITION] as usize;
            let Some(Some(name)) = self.trails.get(def_idx) else {
                continue;
            };
            let key = (view[v::SLOT] as u32, view[v::GENERATION] as u32);
            let pos = [view[v::POSITION], view[v::POSITION + 1], view[v::POSITION + 2]];
            if let Some(entry) = self.mine_fx.iter_mut().find(|e| e.0 == key) {
                entry.2 = true;
                combat.sync_effect(entry.1, pos, None);
            } else if let Some(group) = combat.resolve_effect_group("mine trail", name) {
                if let Some(handle) = combat.start_effect(group, pos, None, key.0.wrapping_mul(31) ^ key.1) {
                    self.mine_fx.push((key, handle, true));
                }
            }
        }
        let mut mi = 0;
        while mi < self.mine_fx.len() {
            if !self.mine_fx[mi].2 {
                combat.stop_effect(self.mine_fx[mi].1);
                self.mine_fx.swap_remove(mi);
            } else {
                mi += 1;
            }
        }
        for entry in self.remote_fx.iter_mut().chain(self.contrail_fx.iter_mut()) {
            entry.2 = false;
        }
        for &(def_id, id, gen, pos, yaw, pitch) in &self.remotes {
            let key = (id, gen);
            let mut q = Quaternion::default();
            q.set_from_euler(&three::Euler::new(-pitch, yaw, 0., three::EulerOrder::YXZ));
            let turn = [q.x, q.y, q.z, q.w];
            let speed = receiver
                .record(kind::REMOTE, f64::from(id), f64::from(gen))
                .map_or(0., |row| (row[net::VELOCITY].powi(2) + row[net::VELOCITY + 1].powi(2) + row[net::VELOCITY + 2].powi(2)).sqrt());
            let (flame, trail) = self.remote_engines.get(def_id).copied().flatten().map_or((Some(1.), None), |e| e.scales(speed));
            let effects = [
                (self.remote_trails.get(def_id).and_then(Option::as_deref), flame, "remote thrust", 17),
                (self.remote_contrails.get(def_id).and_then(Option::as_deref), trail, "remote contrail", 23),
            ];
            for (at, (name, scale, source, salt)) in effects.into_iter().enumerate() {
                let Some(name) = name else { continue };
                let list = if at == 0 { &mut self.remote_fx } else { &mut self.contrail_fx };
                let Some(scale) = scale else { continue };
                if let Some(entry) = list.iter_mut().find(|e| e.0 == key) {
                    entry.2 = true;
                    combat.sync_effect(entry.1, pos, Some(turn));
                    combat.scale_effect(entry.1, scale);
                } else if let Some(group) = combat.resolve_effect_group(source, name) {
                    if let Some(handle) = combat.start_effect(group, pos, Some(turn), id.wrapping_mul(salt) ^ gen) {
                        combat.scale_effect(handle, scale);
                        list.push((key, handle, true));
                    }
                }
            }
        }
        for list in [&mut self.remote_fx, &mut self.contrail_fx] {
            let mut ri = 0;
            while ri < list.len() {
                if !list[ri].2 {
                    combat.stop_effect(list[ri].1);
                    list.swap_remove(ri);
                } else {
                    ri += 1;
                }
            }
        }
    }
    // Published views already contain position and orientation. Compose the
    // rigid draw matrix without advancing bob, fall or water a second time.
    // Optional missing models consume no record or skin slot, and the common
    // model submission checks both renderer budgets before copying a row.
    /// Items.update: each mine or power-up with a LightRadius lights its
    /// surroundings steadily while it lies there.
    pub fn light(&mut self, lights: &mut super::lights::SceneLights) {
        for view in self.owner.views.chunks_exact(v::STRIDE).take(self.owner.header[h::VIEWS] as usize) {
            if let Some(&Some((rgb, radius))) = self.lights.get(view[v::DEFINITION] as usize) {
                lights.add([view[v::POSITION], view[v::POSITION + 1], view[v::POSITION + 2]], rgb, radius, 1.);
                self.lit += 1;
            }
        }
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        for view in self.owner.views.chunks_exact(v::STRIDE).take(self.owner.header[h::VIEWS] as usize) {
            let Some(model) = self.models.get(view[v::DEFINITION] as usize).and_then(Option::as_ref) else {
                continue;
            };
            let at = v::POSITION;
            let q = v::QUATERNION;
            let mut matrix = Matrix4::default();
            matrix.compose(
                &Vector3::new(view[at], view[at + 1], view[at + 2]),
                &Quaternion::new(view[q], view[q + 1], view[q + 2], view[q + 3]),
                &Vector3::new(1., 1., 1.),
            );
            (records, bones) = model.append(bridge, &matrix.elements, records, bones);
        }
        for &(def_id, _, _, pos, yaw, pitch) in &self.remotes {
            let Some(model) = self.remote_models.get(def_id).and_then(Option::as_ref) else {
                continue;
            };
            let mut q = Quaternion::default();
            if pitch != 0. {
                q.set_from_euler(&three::Euler::new(-pitch, yaw, 0., three::EulerOrder::YXZ));
            } else {
                q.set_from_axis_angle(&Vector3::new(0., 1., 0.), yaw);
            }
            let mut matrix = Matrix4::default();
            matrix.compose(&Vector3::new(pos[0], pos[1], pos[2]), &q, &Vector3::new(1., 1., 1.));
            (records, bones) = model.append(bridge, &matrix.elements, records, bones);
        }
        (records, bones)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_light_color_ignores_strings_in_2004_and_parses_them_in_2005() {
        let string_def = bfe_content::json::parse(r#"{"LightColor":"255 128 0 255"}"#).unwrap();
        let array_def = bfe_content::json::parse(r#"{"LightColor":[255,128,0,255]}"#).unwrap();
        assert_eq!(light_color_for_family(bfe_family::GameFamily::Swbf1, &string_def), [0., 0., 0.]);
        let want = [1., 128. / 255., 0.];
        assert_eq!(light_color_for_family(bfe_family::GameFamily::Swbf1, &array_def), want);
        assert_eq!(light_color_for_family(bfe_family::GameFamily::Swbf2, &string_def), want);
    }
}
