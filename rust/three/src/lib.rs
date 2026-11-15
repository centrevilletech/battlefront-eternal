#![forbid(unsafe_code)]
//! three.js r170's math, transliterated for the Rust simulation port.

pub mod euler;
pub mod geom;
pub mod mathutils;
pub mod matrix;
pub mod quaternion;
pub mod transform;
pub mod vector;

pub use euler::{Euler, EulerOrder, ProperEulerOrder};
pub use geom::{Box3, Cylindrical, Line3, Plane, Ray, Sphere, Spherical, Triangle};
pub use mathutils::{clamp, deg_to_rad, lerp, smoothstep};
pub use matrix::{CoordinateSystem, Matrix3, Matrix4};
pub use quaternion::Quaternion;
pub use transform::{NodeId, TransformNode, TransformTree};
pub use vector::{Axis, Vector2, Vector3, Vector4};
