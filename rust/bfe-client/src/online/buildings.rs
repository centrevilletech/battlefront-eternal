//! Placed building looks follow the shared world's published life and supply state.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content_native::BattleContent;
use bfe_pose::rig::hull::HullRig;
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_render::instances as draw;
use three::{Matrix4, Quaternion, Vector3};
struct Look {
    bind: Vec<(Vector3, Quaternion)>,
    rig: HullRig,
    inverses: Vec<Matrix4>,
    skin: Vec<f32>,
    rows: Vec<[f64; draw::REC_STRIDE]>,
}
struct Building {
    turret: bool,
    station: bool,
    id: usize,
    looks: [Option<Look>; 3],
    state: usize,
    health: f64,
    settings: bfe_content::Value,
    placement: Matrix4,
    mode: bool,
    active: f64,
    yaw: f64,
    wait: f64,
    turning: bool,
    direction: f64,
    span: f64,
    seed: u32,
    station_bounds: [[f64; 3]; 2],
}
/// The wreck's world bounds and current repair progress for destruction effects.
pub struct StationFrame {
    pub id: usize,
    pub destroyed: bool,
    pub health: f64,
    pub max_health: f64,
    pub bounds: [[f64; 3]; 2],
    pub center: [f64; 3],
}
pub struct Buildings {
    rows: Vec<Building>,
}
fn number(s: &bfe_content::Value, key: &str, default: f64) -> f64 {
    bfe_content::odf::num(s.get(key), default)
}
impl Buildings {
    pub fn new(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
    ) -> Result<Self, String> {
        let cascades = bridge.shadow_cascades();
        let mut rows = Vec::new();
        let Some(data) = content.content.battle.props.as_ref() else {
            return Ok(Self { rows });
        };
        for (id, source) in
            bfe_sim::world::admit::rows::props::live_sources(&content.content.battle)
                .into_iter()
                .enumerate()
        {
            let desc = data
                .get("instances")
                .and_then(|v| v.at(source))
                .ok_or("missing building")?;
            if !matches!(
                desc.get("base").and_then(bfe_content::Value::as_str),
                Some("powerupstation" | "commandpost" | "armedbuilding")
            ) {
                continue;
            }
            let turret =
                desc.get("base").and_then(bfe_content::Value::as_str) == Some("armedbuilding");
            let mut looks = [None, None, None];
            let mut placement = Matrix4::default();
            for (i, n) in desc
                .get("matrix")
                .and_then(bfe_content::Value::as_array)
                .unwrap_or(&[])
                .iter()
                .enumerate()
                .take(16)
            {
                placement.elements[i] = n.as_f64().unwrap_or(0.);
            }
            for (index, slot) in looks.iter_mut().enumerate() {
                if turret && index == 0 {
                    continue;
                }
                let model = if index == 0 {
                    desc.get("model")
                } else {
                    desc.get(if index == 1 { "destroyed" } else { "unbuilt" })
                        .and_then(|v| v.get("model"))
                };
                let Some(model) = model.and_then(bfe_content::Value::as_str) else {
                    continue;
                };
                let Some(rig) = data.get("models").and_then(|v| v.get(model)) else {
                    continue;
                };
                let def = bfe_pose::rig::content::hull_records(
                    &bfe_content::Value::Null,
                    rig,
                    &[],
                    vec![],
                    false,
                );
                let mut rig = HullRig::new(&def);
                let inverses = rig
                    .bones
                    .iter()
                    .chain(std::iter::once(&rig.static_bone))
                    .map(|id| {
                        let mut m = rig.tree.node(*id).matrix_world;
                        m.invert();
                        m
                    })
                    .collect::<Vec<_>>();
                rig.tree.node_mut(rig.root).matrix = placement;
                rig.tree.node_mut(rig.root).matrix_auto_update = false;
                rig.tree.update_matrix_world(rig.root, true);
                let admitted = crate::static_scene::props(
                    bridge,
                    content,
                    textures,
                    bfe_content_native::placed::admit_look(content, source, index)?,
                    cascades,
                )?;
                let bind = rig
                    .bones
                    .iter()
                    .map(|id| {
                        let n = rig.tree.node(*id);
                        (n.position, n.quaternion)
                    })
                    .collect();
                *slot = Some(Look {
                    bind,
                    skin: vec![0.; inverses.len() * 16],
                    rig,
                    inverses,
                    rows: admitted.records,
                });
            }
            let settings = desc
                .get("settings")
                .cloned()
                .unwrap_or(bfe_content::Value::Null);
            let wait = number(&settings, "IdleWaitTime", 2.);
            let bounds_model = desc.get("destroyed").and_then(|d| d.get("model"))
                .or_else(|| desc.get("model")).and_then(bfe_content::Value::as_str)
                .and_then(|name| data.get("models").and_then(|m| m.get(name)));
            let bounds = bounds_model.and_then(|m| m.get("box"));
            let local: [[f64; 3]; 2] = std::array::from_fn(|side| std::array::from_fn(|axis|
                bounds.and_then(|b| b.at(side)).and_then(|b| b.at(axis))
                    .and_then(bfe_content::Value::as_f64)
                    .unwrap_or([[-0.5, 0., -0.5], [0.5, 1.2, 0.5]][side][axis])));
            let mut station_bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for corner in 0..8 {
                let mut p = Vector3::new(local[corner & 1][0], local[(corner >> 1) & 1][1], local[(corner >> 2) & 1][2]);
                p.apply_matrix4(&placement);
                for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
                    station_bounds[0][axis] = station_bounds[0][axis].min(value);
                    station_bounds[1][axis] = station_bounds[1][axis].max(value);
                }
            }
            rows.push(Building {
                turret,
                station: desc.get("base").and_then(bfe_content::Value::as_str)
                    == Some("powerupstation"),
                id,
                state: if looks[2].is_some() { 2 } else { 0 },
                health: if looks[2].is_some(){0.}else{number(&settings,"MaxHealth",f64::INFINITY)},
                looks,
                settings,
                placement,
                mode: false,
                active: 0.,
                yaw: 0.,
                wait,
                turning: false,
                direction: 1.,
                span: 1.,
                seed: (id as u32 + 1) * 7919,
                station_bounds,
            });
        }
        Ok(Self { rows })
    }
    pub fn frame(&mut self, receiver: &Receiver, dt: f64) {
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::SUMMARY as f64 || field.row[r::SCHEMA] != 7. {
                continue;
            }
            type State<'a> = (
                usize,
                &'a str,
                Option<f64>,
                f64,
                Option<&'a str>,
                f64,
                f64,
                f64,
                f64,
                bool,
                bool,
            );
            let Ok(s) = serde_json::from_slice::<State>(&field.bytes[..field.length]) else {
                continue;
            };
            if let Some(b) = self.rows.iter_mut().find(|b| b.id == s.0) {
                b.state = match s.1 {
                    "built" => 0,
                    "destroyed" => 1,
                    _ => 2,
                };
                b.health = s.2.unwrap_or(f64::INFINITY);
                b.mode = s.4 == Some("active");
                b.active = s.5;
            }
        }
        for row in receiver
            .active()
            .filter(|row| row[r::KIND] == k::TURRET as f64)
        {
            if let Some(b) = self
                .rows
                .iter_mut()
                .find(|b| b.turret && b.id == row[r::BUILDING] as usize)
            {
                b.health = row[r::HEALTH];
                b.state = match row[r::STATE] as u32 {
                    1 => 0,
                    2 => {
                        if b.looks[1].is_some() {
                            1
                        } else {
                            2
                        }
                    }
                    _ => 2,
                };
            }
        }
        for b in &mut self.rows {
            if b.station && b.state == 0 {
                let speed = number(&b.settings, "IdleRotateSpeed", 1.);
                if b.mode {
                    b.active += dt;
                    b.wait = number(&b.settings, "IdleWaitTime", 2.);
                    b.turning = false;
                } else {
                    b.wait -= dt;
                    if b.wait < 0. {
                        if b.turning {
                            b.turning = false;
                            b.wait = number(&b.settings, "IdleWaitTime", 2.);
                        } else {
                            b.turning = true;
                            b.direction = -b.direction;
                            b.seed = b.seed.wrapping_mul(1664525).wrapping_add(1013904223);
                            b.span = std::f64::consts::PI
                                * (0.75 + 0.5 * b.seed as f64 / u32::MAX as f64)
                                / speed;
                            b.wait = b.span;
                        }
                    }
                    if b.turning {
                        b.yaw += b.direction * speed * dt;
                    }
                }
            }
            let Some(look) = b.looks[b.state].as_mut() else {
                continue;
            };
            if b.station && b.state == 0 {
                // Pose from the authored bind on each frame; waiting retains the last pose.
                let root = look.rig.root;
                let mut turn = Matrix4::default();
                turn.make_rotation_y(b.yaw);
                let mut matrix = b.placement;
                matrix.multiply(&turn);
                look.rig.tree.node_mut(root).matrix = matrix;
                let node = |name: &str| {
                    look.rig
                        .by_name
                        .get(&name.to_lowercase())
                        .map(|i| look.rig.bones[i])
                };
                if let Some(id) = b
                    .settings
                    .get("IdleWobbleNode")
                    .and_then(bfe_content::Value::as_str)
                    .and_then(node)
                {
                    let index = look.rig.bones.iter().position(|n| *n == id).unwrap();
                    let (position, quaternion) = look.bind[index];
                    let n = look.rig.tree.node_mut(id);
                    if b.mode {
                        n.position = position;
                        n.position.y += 0.04 * ((std::f64::consts::TAU * b.active).cos() - 1.);
                        n.quaternion = quaternion;
                    } else if b.turning {
                        let mut q = Quaternion::default();
                        q.set_from_axis_angle(
                            &Vector3::new(0., 0., 1.),
                            (12. * std::f64::consts::PI * b.wait / b.span).sin()
                                * number(&b.settings, "IdleWobbleFactor", 0.),
                        );
                        n.quaternion = quaternion;
                        n.quaternion.multiply(&q);
                        n.position = position;
                    }
                }
                for key in ["ActiveRotateNode", "ActiveSpinNode"] {
                    let names = b
                        .settings
                        .get(key)
                        .map(|v| {
                            v.as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(bfe_content::Value::as_str)
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_else(|| v.as_str().into_iter().collect())
                        })
                        .unwrap_or_default();
                    if b.mode {
                        for name in names {
                            if let Some(id) = look.rig.named(Some(name)) {
                                let axis = if key == "ActiveRotateNode" {
                                    Vector3::new(1., 0., 0.)
                                } else {
                                    Vector3::new(0., 0., 1.)
                                };
                                let amount = if key == "ActiveRotateNode" {
                                    0.15
                                } else {
                                    std::f64::consts::PI
                                };
                                let index = look.rig.bone_index(id).unwrap();
                                let mut q = Quaternion::default();
                                q.set_from_axis_angle(
                                    &axis,
                                    amount * (std::f64::consts::TAU * b.active).sin(),
                                );
                                let n = look.rig.tree.node_mut(id);
                                n.quaternion = look.bind[index].1;
                                n.quaternion.multiply(&q);
                            }
                        }
                    }
                }
            }
            look.rig.tree.update_matrix_world(look.rig.root, true);
            for (i, id) in look
                .rig
                .bones
                .iter()
                .chain(std::iter::once(&look.rig.static_bone))
                .enumerate()
            {
                let mut m = look.rig.tree.node(*id).matrix_world;
                m.multiply(&look.inverses[i]);
                for j in 0..16 {
                    look.skin[i * 16 + j] = m.elements[j] as f32;
                }
            }
        }
    }
    pub fn states(&self) -> impl Iterator<Item=(usize, &'static str, f64, bool, f64)> + '_ {
        self.rows.iter().map(|b|(b.id,["built","destroyed","unbuilt"][b.state],b.health,b.mode,b.active))
    }
    pub fn station_frames(&self) -> impl Iterator<Item = StationFrame> + '_ {
        self.rows.iter().filter(|b| b.station).map(|b| StationFrame {
            id: b.id,
            destroyed: b.state == 1,
            health: b.health,
            max_health: number(&b.settings, "MaxHealth", 1.),
            bounds: b.station_bounds,
            center: std::array::from_fn(|i| (b.station_bounds[0][i] + b.station_bounds[1][i]) * 0.5),
        })
    }
    pub fn max_bones(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|b| b.looks.iter().flatten())
            .map(|l| l.inverses.len())
            .max()
            .unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        self.rows
            .iter()
            .filter_map(|b| b.looks[b.state].as_ref())
            .map(|l| l.rows.len())
            .sum()
    }
    pub fn append(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        for b in &self.rows {
            let Some(l) = b.looks[b.state].as_ref() else {
                continue;
            };
            for recipe in &l.rows {
                let count = l.inverses.len();
                if records >= draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                    continue;
                }
                let mut row = *recipe;
                row[draw::REC_BONE_AT] = bones as f64;
                row[draw::REC_BONES] = count as f64;
                bridge.r.core.bones[bones * 16..(bones + count) * 16].copy_from_slice(&l.skin);
                let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
                bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
                records += 1;
                bones += count;
            }
        }
        (records, bones)
    }
}
