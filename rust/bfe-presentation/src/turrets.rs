//! Turret.present and poseTurret: the page's replicated drum and gun pose.
//! All turrets join the prop frame. Preview angles are scalar inputs; no
//! gunner, packet callback or camera makes an individual WASM call.
use crate::{layout::turret_presentation_input as i, scene::{Input, Scene}, vehicles::{aim_vector, look_basis}};
use serde::Deserialize;
use three::{Euler, EulerOrder, Quaternion, Vector3};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub scene: Input,
    #[serde(default)]
    pub seat:i32,
    #[serde(default)]
    pub source_shown:bool,
    #[serde(default)]
    pub turn_rate:f64,
    pub drum: i32,
    pub gun: i32,
    pub parent_turn: Option<[f64; 4]>,
    pub gun_bind: Option<[f64; 4]>,
    pub up: [f64; 3],
    pub yaw_factor: f64,
    pub pitch_factor: f64,
}
// Constructor-derived axes and bind turns never change between frames.
// Resolve optional node ids now instead of searching names while drawing.
pub struct Turret {
    pub scene: Scene,
    seat:usize,shown:bool,turn_rate:f64,
    drum: Option<usize>,
    gun: Option<usize>,
    parent_turn: Option<Quaternion>,
    gun_bind: Option<Quaternion>,
    up: Vector3,
    yaw_factor: f64,
    pitch_factor: f64,
}
fn q(x: [f64; 4]) -> Quaternion { Quaternion::new(x[0], x[1], x[2], x[3]) }
impl Turret {
    /// The built rig stays retained when the building switches to its pad
    /// or wreck. Invalid node handles simply leave that part at rest.
    pub fn new(d: Descriptor, out: &mut Vec<f64>) -> Self {
        let scene = Scene::new(d.scene, out);
        let node = |index: i32| (index >= 0 && (index as usize) < scene.nodes.len()).then_some(index as usize);
        Self {seat:d.seat.max(0) as usize,shown:d.source_shown,turn_rate:d.turn_rate, drum: node(d.drum), gun: node(d.gun), scene,
            parent_turn: d.parent_turn.map(q), gun_bind: d.gun_bind.map(q),
            up: Vector3::new(d.up[0], d.up[1], d.up[2]), yaw_factor: d.yaw_factor, pitch_factor: d.pitch_factor }
    }
    /// The body owner reads the complete native seat frame, preserving slope
    /// and authored drum binds instead of transporting its Three matrix.
    pub fn rider_source(&self,input:&[f64])->crate::rider_sources::Source {
        let frame=self.scene.nodes.get(self.seat).map_or(three::Matrix4::default(),|&node|self.scene.tree.node(node).matrix_world);
        crate::rider_sources::Source{frame,position:Vector3::new(frame.elements[12],frame.elements[13],frame.elements[14]),
            yaw:input[i::YAW],pitch:input[i::PITCH],yaw_rate:input[i::YAW_RATE],shown:self.shown,turn_rate:self.turn_rate,..Default::default()}
    }
    /// props.js Turret.present preserves the operation order: parent turn
    /// times look basis for the drum, bind times YXZ for the gun.
    pub fn present(&mut self, input: &[f64], out: &mut [f64]) {
        // Malformed/stale scalar inputs retain the last pose. A bounded
        // frame never poisons the scene with a non-finite quaternion.
        if input[..i::STRIDE].iter().any(|value|!value.is_finite()) {self.scene.publish(out);return;}
        // The drum takes the aim basis in its parent's frame. Its authored
        // heading is not multiplied in a second time on a backward bind.
        if let (Some(drum), Some(mut parent)) = (self.drum, self.parent_turn) {
            parent.multiply(&look_basis(&aim_vector(self.yaw_factor * input[i::YAW], self.pitch_factor * input[i::PITCH]), &self.up));
            self.scene.tree.node_mut(self.scene.nodes[drum]).copy_quaternion(&parent);
        }
        // The gun keeps its bind turn; up-positive pitch is a negative
        // rotation about X, composed in the original YXZ order.
        if let (Some(gun), Some(mut bind)) = (self.gun, self.gun_bind) {
            let mut turn = Quaternion::default();
            turn.set_from_euler(&Euler::new(-input[i::GUN_PITCH], input[i::GUN_YAW], 0.0, EulerOrder::YXZ));
            bind.multiply(&turn);
            self.scene.tree.node_mut(self.scene.nodes[gun]).copy_quaternion(&bind);
        }
        self.scene.publish(out);
    }
}
