//! Poses and rigs for the simulation: the skeletons, hull rigs and hidden
//! gun rigs the world tick reads, and the points it reads off them (a
//! rider's hit shapes and where he falls from, a droideka's muzzles, a
//! hull's fire, seat and rider nodes).
//!
//! - `math`: the transcendental functions, in one place (V8's values, and
//!   the replays' check of them against what V8 answered);
//! - `skeleton`: a model's bones and their bind pose (src/actor.js);
//! - `rider`: a rider's points, hit shapes and drop point
//!   (src/rider-pose.js);
//! - `droideka`: a droideka's hidden gun rig and its animator
//!   (src/droideka.js);
//! - `hull`: a hull's rig, its fire, seat and rider nodes and whether its
//!   walk pose aims its guns (src/vehicles.js, src/muzzles.js);
//! - `clips`: what the simulation reads of a clip (its duration, stride
//!   and speed; a soldier's get-up and dive times);
//! - `content`: the rigs' inputs from rust/content.
//!
//! The three.js r170 arithmetic and Object3D transform semantics the rigs
//! are built on are rust/three's (`Vector3`, `Quaternion`, `Matrix4`,
//! `Euler`, `Box3`, `TransformTree` with three's Euler sync, deferred), and
//! `math` computes with rust/v8math (V8 13.6's functions, bit for bit).

pub mod clips;
pub mod content;
pub mod droideka;
pub mod hull;
pub mod math;
pub mod rider;
pub mod skeleton;

pub mod construction;
