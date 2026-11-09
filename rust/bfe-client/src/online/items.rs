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
// ItemPresentation retains its own fall, bob and water clocks. Wire records
// admit or retire qualified items; a renderer recipe is only a resource
// prerequisite. The host never integrates an independent falling body.
// Mines and power-ups therefore share the page's numerical item owner.
pub struct Views {
    pub owner: Items,
    definitions: Vec<Option<usize>>,
    models: Vec<Option<Model>>,
    collision: Collision,
    world: u32,
    water: Option<bfe_sim::units::water::WaterState>,
    last_water: f64,
    pub visible: usize,
}
impl Views {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], collision: Collision, world: u32) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let mut owner = Items::new();
        let mut definitions = vec![None; defs.registry.combat.len() + 1];
        let mut models = Vec::new();
        for (i, (_, def)) in defs.registry.combat.iter().enumerate() {
            let base = def.get("base").and_then(bfe_content::Value::as_str).unwrap_or("");
            if base != "mine" && base != "powerupitem" {
                continue;
            }
            if models.len() == limit::DEFINITIONS {
                continue;
            }
            let index = models.len();
            definitions[i + 1] = Some(index);
            let name = def.get("GeometryName").and_then(bfe_content::Value::as_str).unwrap_or("");
            let model = Model::new(content, bridge, textures, name, crate::shadows::COUNT)?;
            let at = index * d::STRIDE;
            let row = &mut owner.definitions[at..at + d::STRIDE];
            for (at, key, default) in [(d::GRAVITY, "Gravity", 1.), (d::REBOUND, "Rebound", 0.3), (d::FRICTION, "Friction", 0.5)] {
                row[at] = def.get(key).filter(|v| !v.is_null()).map_or(default, |v| bfe_content::js::to_number(Some(v)));
            }
            row[d::MINE] = (base == "mine") as u8 as f64;
            row[d::RADIUS] = bfe_content::derived::item_radius(content.content.weapon_models.get(name)).0;
            row[d::LIGHT_RADIUS] = def.get("LightRadius").and_then(bfe_content::Value::as_f64).unwrap_or(0.).max(0.);
            row[d::MESH] = model.is_some() as u8 as f64;
            owner.define(index);
            models.push(model);
        }
        // WaterState is the existing shared crossing policy and tick bob. Its
        // cells are immutable map content, not another gameplay simulation.
        let water = super::water::new(content);
        for (axis, extent) in [super::receiver::BOUNDS[1], super::receiver::BOUNDS[5], super::receiver::BOUNDS[3]].iter().enumerate() {
            owner.frame[f::ROUND_X + axis] = extent / 65535. / 2. + 0.002;
        }
        Ok(Self { owner, definitions, models, collision, world, water, last_water: 0., visible: 0 })
    }
    // Changed item records are applied before sampling. The retained owner
    // can suspend for all map and water queries, consume their answers and
    // resume without returning through an item object or allocating a result.
    // A long packet burst clips to the owner's existing record capacity.
    pub fn frame(&mut self, receiver: &Receiver, tick: f64) {
        self.owner.reset_epoch(receiver.epoch);
        let mut count = 0;
        for row in receiver.active().filter(|r| r[net::KIND] == kind::ITEM as f64) {
            if count == limit::BATCH {
                self.owner.apply_records(count);
                count = 0;
            }
            let Some(definition) = self.definitions.get(row[net::DEFINITION_ID] as usize).copied().flatten() else {
                continue;
            };
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
        self.visible = self.owner.header[h::VIEWS] as usize;
    }
    pub fn record_budget(&self) -> usize {
        self.owner
            .views
            .chunks_exact(v::STRIDE)
            .take(self.visible)
            .filter_map(|v| self.models.get(v[v::DEFINITION] as usize).and_then(Option::as_ref))
            .map(|m| m.rows.len())
            .sum()
    }
    // Published views already contain position and orientation. Compose the
    // rigid draw matrix without advancing bob, fall or water a second time.
    // Optional missing models consume no record or skin slot, and the common
    // model submission checks both renderer budgets before copying a row.
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        for view in self.owner.views.chunks_exact(v::STRIDE).take(self.visible) {
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
        (records, bones)
    }
}
