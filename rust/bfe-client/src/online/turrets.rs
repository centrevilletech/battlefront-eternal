//! Armed buildings retain the shared prop tree and Turret.present owner.
//! Packet callbacks name buildings; a frame publishes all drums, guns and
//! rider sources together, preserving the authored bind and parent turns.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content_native::{placed::Placements, BattleContent};
use bfe_presentation::{
    layout::{prop_frame as f, scene_node as n, turret_presentation_input as t, VERSION},
    network_layout::{net_kind as k, net_record as r},
    props::Props,
    scene::{Input, Node},
    turrets::Descriptor,
};
use bfe_render::instances as draw;
use three::{Matrix4, Quaternion, Vector3};
struct Part {
    row: [f64; draw::REC_STRIDE],
    node: usize,
}
struct Turret {
    instance: usize,
    start: usize,
    bones: Vec<usize>,
    inverses: Vec<Matrix4>,
    skin: Vec<f32>,
    parts: Vec<Part>,
    id: f64,
    generation: f64,
    shown: bool,
    pub clip: Option<String>,
    pub nine: bool,
    camera: Vec<f64>,
    aim: super::seat_view::View,
}
pub struct Turrets {
    pub owner: Box<Props>,
    rows: Vec<Turret>,
    pub visible: usize,
    pub hidden: Option<usize>,
    preview: Box<bfe_presentation::network::preview::Phase>,
}
fn copy_nodes(tree: &three::TransformTree, node: three::NodeId, parent: i32, out: &mut Vec<Node>, ids: &mut [usize]) {
    let id = out.len();
    ids[node as usize] = id;
    let n = tree.node(node);
    out.push(Node {
        parent,
        pose: [
            n.position.x,
            n.position.y,
            n.position.z,
            n.quaternion.x,
            n.quaternion.y,
            n.quaternion.z,
            n.quaternion.w,
            n.scale.x,
            n.scale.y,
            n.scale.z,
        ],
        matrix: n.matrix.elements,
        auto: n.matrix_auto_update,
        skinned: false,
    });
    for child in &n.children {
        copy_nodes(tree, *child, id as i32, out, ids);
    }
}
impl Turrets {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32]) -> Result<Self, String> {
        let placed = bfe_content_native::placed::admit_turrets(content)?;
        let props = content.content.battle.props.as_ref();
        let mut owner = Box::new(Props::new());
        let mut rows = Vec::new();
        for (instance, desc) in
            props.and_then(|p| p.get("instances")).and_then(bfe_content::Value::as_array).unwrap_or(&[]).iter().enumerate()
        {
            if desc.get("base").and_then(bfe_content::Value::as_str) != Some("armedbuilding") {
                continue;
            }
            let model = desc.get("model").and_then(bfe_content::Value::as_str).ok_or("turret has no model")?;
            let rig = props.and_then(|p| p.get("models")).and_then(|models| models.get(model)).ok_or("turret has no rig")?;
            let def = bfe_pose::rig::content::hull_records(&bfe_content::Value::Null, rig, &[], vec![], false);
            let mut hull = bfe_pose::rig::hull::HullRig::new(&def);
            // Inverses are captured in the unplaced bind frame. Reusing a
            // placed inverse would erase the attached skin's world rounding.
            let inverses = hull
                .bones
                .iter()
                .chain(std::iter::once(&hull.static_bone))
                .map(|id| {
                    let mut m = hull.tree.node(*id).matrix_world;
                    m.invert();
                    m
                })
                .collect::<Vec<_>>();
            let matrix: Vec<f64> =
                desc.get("matrix").and_then(bfe_content::Value::as_array).unwrap_or(&[]).iter().map(|n| n.as_f64().unwrap_or(0.)).collect();
            if matrix.len() != 16 {
                return Err("invalid turret placement".into());
            }
            let root = hull.root;
            hull.tree.node_mut(root).matrix.elements.copy_from_slice(&matrix);
            hull.tree.node_mut(root).matrix_auto_update = false;
            hull.tree.update_matrix_world(root, true);
            let settings = desc.get("settings");
            let text = |key: &str| settings.and_then(|s| s.get(key)).and_then(bfe_content::Value::as_str).unwrap_or("");
            let bone = |name: &str| hull.by_name.get(&name.to_lowercase()).map(|i| hull.bones[i]);
            let drum = bone(text("TurretNodeName"));
            let gun = bone(text("AimerNodeName"));
            let seat = bone(text("PilotPosition")).unwrap_or(root);
            let mut root_turn = Quaternion::default();
            hull.tree.get_world_quaternion(root, &mut root_turn);
            root_turn.invert();
            let mut up = Vector3::new(0., 1., 0.);
            if let Some(drum) = drum {
                let mut turn = Quaternion::default();
                hull.tree.get_world_quaternion(drum, &mut turn);
                let mut local = root_turn;
                local.multiply(&turn);
                up.apply_quaternion(&local).normalize();
            }
            let parent_turn = drum.and_then(|drum| hull.tree.node(drum).parent).map(|parent| {
                let mut turn = Quaternion::default();
                hull.tree.get_world_quaternion(parent, &mut turn);
                let mut local = root_turn;
                local.multiply(&turn);
                local.invert();
                [local.x, local.y, local.z, local.w]
            });
            let gun_bind = gun.map(|gun| {
                let q = hull.tree.node(gun).quaternion;
                [q.x, q.y, q.z, q.w]
            });
            let mut ids = vec![usize::MAX; hull.tree.nodes.len()];
            let mut nodes = Vec::new();
            copy_nodes(&hull.tree, root, -1, &mut nodes, &mut ids);
            let bones = hull.bones.iter().chain(std::iter::once(&hull.static_bone)).map(|id| ids[*id as usize]).collect::<Vec<_>>();
            let anim = settings
                .and_then(|s| s.get("Pilot9Pose"))
                .filter(|v| v.truthy())
                .or_else(|| settings.and_then(|s| s.get("PilotAnimation")))
                .and_then(bfe_content::Value::as_str);
            let clip = anim.map(|name| format!("human_{}", name.to_lowercase()));
            let nine = settings.and_then(|s| s.get("Pilot9Pose")).is_some_and(bfe_content::Value::truthy);
            let number = |key: &str, default: f64| bfe_content::odf::num(settings.and_then(|s| s.get(key)), default);
            let start = owner.add_turret(Descriptor {
                scene: Input { nodes, bones: bones.clone(), parent: None },
                seat: ids[seat as usize] as i32,
                source_shown: clip.is_some(),
                turn_rate: number("MaxTurnSpeed", 2.),
                drum: drum.map_or(-1, |id| ids[id as usize] as i32),
                gun: gun.map_or(-1, |id| ids[id as usize] as i32),
                parent_turn,
                gun_bind,
                up: [up.x, up.y, up.z],
                yaw_factor: number("YawTurnFactor", 1.),
                pitch_factor: number("PitchTurnFactor", 1.),
            });
            let parts = placed
                .parts
                .iter()
                .filter(|p| p.instance == instance)
                .map(|p| {
                    let node = p.attachment.as_ref().and_then(|name| bone(name)).map_or(ids[root as usize], |id| ids[id as usize]);
                    (p, node)
                })
                .collect::<Vec<_>>();
            let mut recipes = Vec::new();
            for (part, node) in parts {
                let part = bfe_content_native::placed::Part {
                    instance: part.instance,
                    attachment: part.attachment.clone(),
                    model: part.model.clone(),
                    source: part.source,
                    geometry_key: part.geometry_key.clone(),
                    matrix: part.matrix,
                    positions: part.positions.clone(),
                    normals: part.normals.clone(),
                    uv: part.uv.clone(),
                    index: part.index.clone(),
                    joints: part.joints.clone(),
                    weights: part.weights.clone(),
                    bones: part.bones.clone(),
                    bind_inverse: part.bind_inverse,
                    center: part.center,
                    radius: part.radius,
                };
                let admission = crate::static_scene::props(bridge, content, textures, Placements { parts: vec![part], deferred: 0 }, crate::shadows::COUNT)?;
                for row in admission.records {
                    recipes.push(Part { row, node });
                }
            }
            // Camera owns obstruction and easing. Resource admission supplies
            // the same bind-frame pivot, up and authored view as Turret.
            use bfe_presentation::layout::camera_frame as cf;
            let mut camera = vec![0.; cf::STRIDE];
            camera[cf::TURRET_ROOT..cf::TURRET_ROOT + 16].copy_from_slice(&matrix);
            let mut pivot = Vector3::default();
            if let Some(drum) = drum {
                hull.tree.get_world_position(drum, &mut pivot);
                let mut inverse = hull.tree.node(root).matrix_world;
                inverse.invert();
                pivot.apply_matrix4(&inverse);
            } else if let Some(v) = settings.and_then(|s| s.get("f2aa4a90")) {
                let text = bfe_content::odf::to_text(Some(v));
                let xyz: Vec<_> = text.split_whitespace().filter_map(|n| n.parse::<f64>().ok()).collect();
                if xyz.len() >= 3 {
                    pivot.set(xyz[0], xyz[1], xyz[2]);
                }
            }
            camera[cf::TURRET_PIVOT..cf::TURRET_PIVOT + 3].copy_from_slice(&[pivot.x, pivot.y, pivot.z]);
            camera[cf::TURRET_UP..cf::TURRET_UP + 3].copy_from_slice(&[up.x, up.y, up.z]);
            camera[cf::TURRET_FACING] = matrix[8].atan2(matrix[10]);
            let view = bfe_pose::rig::construction::turret_view(settings);
            for (at, key, count) in [
                (cf::TURRET_EYE, "eye", 3),
                (cf::TURRET_CENTER, "center", 3),
                (cf::TURRET_OFFSET, "offset", 3),
                (cf::TURRET_RATES, "moveTension", 6),
            ] {
                for k in 0..count {
                    camera[at + k] = view[key][k].as_f64().unwrap_or(0.);
                }
            }
            for (at, key) in
                [(cf::TURRET_FIRST, "firstFov"), (cf::TURRET_THIRD, "thirdFov"), (cf::TURRET_TILT, "tilt"), (cf::TURRET_AIM, "aimTension")]
            {
                camera[at] = view[key].as_f64().unwrap_or(0.);
            }
            rows.push(Turret {
                instance,
                start,
                bones,
                skin: vec![0.; inverses.len() * 16],
                inverses,
                parts: recipes,
                id: -1.,
                generation: 0.,
                shown: true,
                clip,
                nine,
                camera,
                aim: super::seat_view::View::new(&view),
            });
        }
        Ok(Self { owner, rows, visible: 0, hidden: None, preview: Box::new(Default::default()) })
    }
    pub fn source(&self, id: f64, generation: f64) -> Option<usize> {
        self.rows.iter().position(|t| t.id == id && t.generation == generation && t.shown)
    }
    pub fn rider(&self, index: usize) -> Option<(&str, bool)> {
        let t = self.rows.get(index)?;
        Some((t.clip.as_deref().unwrap_or("human_minigun_9pose"), t.nine))
    }
    pub fn frame(&mut self, receiver: &Receiver, history: &super::replay::History, dt: f64) {
        for row in receiver.active().filter(|r| r[r::KIND] == k::TURRET as f64) {
            if let Some((index, turret)) = self.rows.iter_mut().enumerate().find(|(_, t)| t.instance == row[r::BUILDING] as usize) {
                turret.id = row[r::ID];
                turret.generation = row[r::GENERATION];
                turret.shown = row[r::STATE] == 1.;
                let input = &mut self.owner.turret_input[index * t::STRIDE..][..t::STRIDE];
                input[t::YAW] = row[r::YAW];
                input[t::PITCH] = row[r::PITCH];
                input[t::GUN_YAW] = row[r::GUN_YAW];
                input[t::GUN_PITCH] = row[r::GUN_PITCH];
                input[t::YAW_RATE] = 0.;
            }
        }
        // A gunner's exact mounted checkpoint, rather than the delayed
        // remote drum sample, is the baseline for every pending command.
        if let Ok(Some(mount)) = bfe_sim::prediction::corrections::own_mounted(&receiver.checkpoint[..receiver.checkpoint_length]) {
            if mount.mode == 3 {
                if let Some(index) = self.source(mount.id as f64, mount.generation as f64) {
                    use bfe_presentation::network_layout::{net_preview_frame as p, net_preview_output as o};
                    let turret = &self.rows[index];
                    let view = &turret.aim;
                    let a = &mut self.preview.frame;
                    a.fill(0.);
                    a[p::ENABLED] = 1.;
                    a[p::PREDICTED] = 1.;
                    a[p::SCALE] = 1.;
                    a[p::ZOOM] = 1.;
                    a[p::TURRET_ACTIVE] = 1.;
                    a[p::TURRET_ID] = mount.id as f64;
                    a[p::TURRET_GENERATION] = mount.generation as f64;
                    a[p::TURRET_YAW] = mount.values[0];
                    a[p::TURRET_PITCH] = mount.values[1];
                    a[p::YAW_VEL] = mount.values[2];
                    a[p::PITCH_VEL] = mount.values[3];
                    view.preview(a, true);
                    a[p::INPUT_ALPHA] = 1.;
                    a[p::PRESENT_ALPHA] = 1.;
                    history.preview(&mut self.preview);
                    self.preview.present();
                    let input = &mut self.owner.turret_input[index * t::STRIDE..][..t::STRIDE];
                    input[t::YAW] = self.preview.output[o::SHOWN_YAW];
                    input[t::PITCH] = self.preview.output[o::SHOWN_PITCH];
                    input[t::GUN_YAW] = mount.values[4];
                    input[t::GUN_PITCH] = mount.values[5];
                }
            }
        }
        self.owner.input[f::VERSION] = VERSION as f64;
        self.owner.input[f::DT] = dt;
        self.owner.frame();
        self.visible = 0;
        for turret in &mut self.rows {
            if !turret.shown {
                continue;
            }
            self.visible += 1;
            for (i, node) in turret.bones.iter().enumerate() {
                let at = turret.start + node * n::STRIDE + n::WORLD;
                let mut world = Matrix4::default();
                world.elements.copy_from_slice(&self.owner.nodes[at..at + 16]);
                let mut skin = Matrix4::default();
                skin.multiply_matrices(&world, &turret.inverses[i]);
                for (k, v) in skin.elements.iter().enumerate() {
                    turret.skin[i * 16 + k] = *v as f32;
                }
            }
        }
    }
    pub fn camera(&self, id: f64, generation: f64, x: &mut [f64]) -> Option<usize> {
        use bfe_presentation::layout::camera_frame as f;
        let index = self.source(id, generation)?;
        let turret = &self.rows[index];
        x[f::TURRET_ACTIVE] = 1.;
        x[f::TURRET_ID] = id;
        for (at, count) in [
            (f::TURRET_ROOT, 16),
            (f::TURRET_PIVOT, 3),
            (f::TURRET_UP, 3),
            (f::TURRET_EYE, 3),
            (f::TURRET_CENTER, 3),
            (f::TURRET_OFFSET, 3),
            (f::TURRET_RATES, 6),
        ] {
            x[at..at + count].copy_from_slice(&turret.camera[at..at + count]);
        }
        for at in [f::TURRET_FACING, f::TURRET_FIRST, f::TURRET_THIRD, f::TURRET_TILT, f::TURRET_AIM] {
            x[at] = turret.camera[at];
        }
        let input = &self.owner.turret_input[index * t::STRIDE..];
        x[f::TURRET_YAW] = input[t::YAW];
        x[f::TURRET_PITCH] = input[t::PITCH];
        Some(index)
    }
    // Shadow storage covers the largest admitted rig, including models that
    // have not appeared in a snapshot yet.
    pub fn max_bones(&self) -> usize {
        self.rows.iter().map(|turret| turret.inverses.len()).max().unwrap_or(1)
    }
    pub fn record_budget(&self) -> usize {
        self.rows.iter().filter(|t| t.shown).map(|t| t.parts.len()).sum()
    }
    pub fn hidden_shadow_budget(&self) -> usize {
        self.hidden
            .and_then(|index| self.rows.get(index))
            .filter(|t| t.shown)
            .map_or(0, |t| t.parts.len())
    }
    pub fn append(&self, bridge: &mut Bridge, records: usize, bones: usize) -> (usize, usize) {
        self.append_visibility(bridge, records, bones, false)
    }
    pub fn append_hidden_shadow(
        &self,
        bridge: &mut Bridge,
        records: usize,
        bones: usize,
    ) -> (usize, usize) {
        self.append_visibility(bridge, records, bones, true)
    }
    fn append_visibility(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
        hidden: bool,
    ) -> (usize, usize) {
        for (index, t) in self.rows.iter().enumerate() {
            if !t.shown || (self.hidden == Some(index)) != hidden {
                continue;
            }
            for part in &t.parts {
                let count = t.inverses.len();
                if records == draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                    continue;
                }
                let at = t.start + part.node * n::STRIDE + n::WORLD;
                let world = &self.owner.nodes[at..at + 16];
                let mut inverse = Matrix4::default();
                inverse.elements.copy_from_slice(world);
                inverse.invert();
                let mut row = part.row;
                row[draw::REC_WORLD..draw::REC_WORLD + 16].copy_from_slice(world);
                row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
                row[draw::REC_BONES] = count as f64;
                row[draw::REC_BONE_AT] = bones as f64;
                bridge.r.core.bones[bones * 16..(bones + count) * 16].copy_from_slice(&t.skin);
                let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
                bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
                records += 1;
                bones += count;
            }
        }
        (records, bones)
    }
}
