//! `Euler.js` (r170). The `_onChangeCallback` sync to a sibling
//! quaternion lives in [`crate::TransformNode`]; a bare `Euler`
//! here is just the angles and their order, as it is in three when
//! nothing registered a callback.

use jsrt::v8math;

use crate::mathutils::clamp;
use crate::matrix::Matrix4;
use crate::quaternion::Quaternion;
use crate::vector::Vector3;

/// An Euler order: three's `'XYZ'`, `'YXZ'`, `'ZXY'`, `'ZYX'`,
/// `'YZX'`, `'XZY'` (Tait-Bryan orders; `Euler.DEFAULT_ORDER` is
/// `'XYZ'`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EulerOrder {
    #[default]
    XYZ,
    YXZ,
    ZXY,
    ZYX,
    YZX,
    XZY,
}

impl EulerOrder {
    /// The order as three spells it (`order` in toArray and JSON).
    pub fn as_str(&self) -> &'static str {
        match self {
            EulerOrder::XYZ => "XYZ",
            EulerOrder::YXZ => "YXZ",
            EulerOrder::ZXY => "ZXY",
            EulerOrder::ZYX => "ZYX",
            EulerOrder::YZX => "YZX",
            EulerOrder::XZY => "XZY",
        }
    }

    /// Parse three's spelling; anything else is the unknown order
    /// three warns about and then leaves the value alone.
    pub fn parse(s: &str) -> Option<EulerOrder> {
        match s {
            "XYZ" => Some(EulerOrder::XYZ),
            "YXZ" => Some(EulerOrder::YXZ),
            "ZXY" => Some(EulerOrder::ZXY),
            "ZYX" => Some(EulerOrder::ZYX),
            "YZX" => Some(EulerOrder::YZX),
            "XZY" => Some(EulerOrder::XZY),
            _ => None,
        }
    }
}

impl core::str::FromStr for EulerOrder {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}

/// A proper-Euler order, for
/// [`crate::mathutils::set_quaternion_from_proper_euler`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProperEulerOrder {
    XYX,
    YZY,
    ZXZ,
    XZX,
    YXY,
    ZYZ,
}

/// `THREE.Euler`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Euler {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub order: EulerOrder,
}

impl Default for Euler {
    /// `new Euler()`: (0, 0, 0, 'XYZ').
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, EulerOrder::XYZ)
    }
}

impl Euler {
    pub const fn new(x: f64, y: f64, z: f64, order: EulerOrder) -> Self {
        Euler { x, y, z, order }
    }

    /// `set(x, y, z, order = this._order)`.
    pub fn set(&mut self, x: f64, y: f64, z: f64, order: Option<EulerOrder>) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        if let Some(order) = order {
            self.order = order;
        }
        self
    }

    /// `copy(euler)`.
    pub fn copy(&mut self, euler: &Euler) -> &mut Self {
        self.x = euler.x;
        self.y = euler.y;
        self.z = euler.z;
        self.order = euler.order;
        self
    }

    /// `setFromRotationMatrix(m, order = this._order, update)`:
    /// assumes the upper 3x3 of `m` is a pure rotation matrix. The
    /// gimbal branches and the 0.9999999 thresholds are three's,
    /// including which angle is zeroed.
    pub fn set_from_rotation_matrix(&mut self, m: &Matrix4, order: Option<EulerOrder>) -> &mut Self {
        let te = &m.elements;
        let m11 = te[0];
        let m12 = te[4];
        let m13 = te[8];
        let m21 = te[1];
        let m22 = te[5];
        let m23 = te[9];
        let m31 = te[2];
        let m32 = te[6];
        let m33 = te[10];
        let order = order.unwrap_or(self.order);
        match order {
            EulerOrder::XYZ => {
                self.y = v8math::asin(clamp(m13, -1.0, 1.0));
                if m13.abs() < 0.9999999 {
                    self.x = v8math::atan2(-m23, m33);
                    self.z = v8math::atan2(-m12, m11);
                } else {
                    self.x = v8math::atan2(m32, m22);
                    self.z = 0.0;
                }
            }
            EulerOrder::YXZ => {
                self.x = v8math::asin(-clamp(m23, -1.0, 1.0));
                if m23.abs() < 0.9999999 {
                    self.y = v8math::atan2(m13, m33);
                    self.z = v8math::atan2(m21, m22);
                } else {
                    self.y = v8math::atan2(-m31, m11);
                    self.z = 0.0;
                }
            }
            EulerOrder::ZXY => {
                self.x = v8math::asin(clamp(m32, -1.0, 1.0));
                if m32.abs() < 0.9999999 {
                    self.y = v8math::atan2(-m31, m33);
                    self.z = v8math::atan2(-m12, m22);
                } else {
                    self.y = 0.0;
                    self.z = v8math::atan2(m21, m11);
                }
            }
            EulerOrder::ZYX => {
                self.y = v8math::asin(-clamp(m31, -1.0, 1.0));
                if m31.abs() < 0.9999999 {
                    self.x = v8math::atan2(m32, m33);
                    self.z = v8math::atan2(m21, m11);
                } else {
                    self.x = 0.0;
                    self.z = v8math::atan2(-m12, m22);
                }
            }
            EulerOrder::YZX => {
                self.z = v8math::asin(clamp(m21, -1.0, 1.0));
                if m21.abs() < 0.9999999 {
                    self.x = v8math::atan2(-m23, m22);
                    self.y = v8math::atan2(-m31, m11);
                } else {
                    self.x = 0.0;
                    self.y = v8math::atan2(m13, m33);
                }
            }
            EulerOrder::XZY => {
                self.z = v8math::asin(-clamp(m12, -1.0, 1.0));
                if m12.abs() < 0.9999999 {
                    self.x = v8math::atan2(m32, m22);
                    self.y = v8math::atan2(m13, m11);
                } else {
                    self.x = v8math::atan2(-m23, m33);
                    self.y = 0.0;
                }
            }
        }
        self.order = order;
        self
    }

    /// `setFromQuaternion(q, order, update)`: through a rotation
    /// matrix, as three does (its module `_matrix`). An absent
    /// order keeps this Euler's, which is how the Object3D
    /// quaternion-to-Euler sync calls it (`undefined`).
    pub fn set_from_quaternion(&mut self, q: &Quaternion, order: Option<EulerOrder>) -> &mut Self {
        let mut m = Matrix4::default();
        m.make_rotation_from_quaternion(q);
        self.set_from_rotation_matrix(&m, order)
    }

    /// `setFromVector3(v, order = this._order)`.
    pub fn set_from_vector3(&mut self, v: &Vector3, order: Option<EulerOrder>) -> &mut Self {
        self.set(v.x, v.y, v.z, order)
    }

    /// `reorder(newOrder)`: through a quaternion (this discards
    /// revolution information, as three warns).
    pub fn reorder(&mut self, new_order: EulerOrder) -> &mut Self {
        let mut q = Quaternion::default();
        q.set_from_euler(self);
        self.set_from_quaternion(&q, Some(new_order))
    }

    /// `equals(euler)`: `===` on every component and the order.
    pub fn equals(&self, euler: &Euler) -> bool {
        euler.x == self.x && euler.y == self.y && euler.z == self.z && euler.order == self.order
    }

    /// `fromArray(array)` without the optional order slot.
    pub fn from_array(&mut self, array: &[f64]) -> &mut Self {
        self.x = array[0];
        self.y = array[1];
        self.z = array[2];
        self
    }

    /// `fromArray` with the order slot present (`array[3]`, three's
    /// spelling). An unknown order string leaves the order as it
    /// cannot be held here; sim content spells valid orders.
    pub fn from_array_with_order(&mut self, array: &[f64], order: Option<EulerOrder>) -> &mut Self {
        self.from_array(array);
        if let Some(order) = order {
            self.order = order;
        }
        self
    }

    /// `toArray(array, offset)`: the three angles (the order, which
    /// three writes at `offset + 3`, is this Euler's `order`).
    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset] = self.x;
        array[offset + 1] = self.y;
        array[offset + 2] = self.z;
        array
    }
}
