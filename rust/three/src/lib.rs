#![forbid(unsafe_code)]
//! three.js r170's math, transliterated for the Rust simulation port.
//!
//! Every method repeats the r170 JavaScript operation by operation,
//! in f64, in the same order, with the same branches — including the
//! places where three is not the mathematically tidy choice (the
//! gimbal branches in `Euler::set_from_rotation_matrix` at
//! 0.9999999, `Quaternion::normalize` inventing (0,0,0,1) for a zero
//! quaternion, `Vector3::normalize` dividing by `length || 1`,
//! `Matrix4::look_at` nudging z by 0.0001). Anything else would move
//! a turret, a rider or a hit shape by an ulp and diverge a battle.
//! Comments cite the r170 source file each impl ports.
//!
//! Covered: `Vector2`, `Vector3`, `Vector4`, `Quaternion`, `Euler`,
//! `Matrix3`, `Matrix4`, `Box3`, `Sphere`, `Plane`, `Ray`, `Line3`,
//! `Triangle`, `Spherical`, `Cylindrical`, `MathUtils`, and
//! [`TransformTree`]: the `Object3D` transform semantics the sim's
//! rigs live on (local TRS, the Euler/quaternion sync, `matrix` and
//! `matrixWorld` with `matrixAutoUpdate` / `matrixWorldAutoUpdate` /
//! `matrixWorldNeedsUpdate`, `updateMatrixWorld` and
//! `updateWorldMatrix`, the world getters, `lookAt`, `attach`, the
//! rotate and translate helpers). Not covered, deliberately: color,
//! frustum and camera classes, geometry and buffer attributes
//! (`fromBufferAttribute`, `Box3::setFromObject` and friends need
//! them), and anything that draws. Packages that need a vertex
//! walk compose it from `expand_by_point` and `apply_matrix4`.
//!
//! Method names are Rust's spelling of three's (`setFromAxisAngle`
//! is `set_from_axis_angle`); methods that return `this` in three
//! return `&mut Self` here, and target-taking methods
//! (`getCenter(target)`) return the target. The `Math.random`
//! methods (`Vector3::random`, `generateUUID`, ...) take the draw
//! source as a parameter: in a tick that source is a named
//! `jsrt` stream or the value is not drawn at all (a bare
//! `Math.random` in the sim is a determinism bug the port exists to
//! remove, and three's own UUID draws are among the leaks P0
//! routes).
//!
//! Every transcendental goes through `jsrt::v8math` (the seam to
//! rust/v8math, W13). While that crate is in flight the seam holds
//! a stub, so the methods that reach sin/cos/asin/atan2/acos/exp
//! are exact in structure but not yet in their last bits; the
//! differential harness in `tools/` reports those separately from
//! the algebraic methods, which are proven bit for bit against
//! three r170 under Node 24.22 (V8 13.6). `Math.min`/`Math.max`,
//! `Math.round` and friends come from `jsrt`, never from Rust.

pub mod euler;
pub mod geom;
pub mod mathutils;
pub mod matrix;
pub mod quaternion;
pub mod transform;
pub mod vector;

pub use euler::{Euler, EulerOrder};
pub use geom::{Box3, Cylindrical, Line3, Plane, Ray, Sphere, Spherical, Triangle};
pub use matrix::{CoordinateSystem, Matrix3, Matrix4};
pub use quaternion::Quaternion;
pub use transform::{NodeId, TransformNode, TransformTree};
pub use vector::{Vector2, Vector3, Vector4};
