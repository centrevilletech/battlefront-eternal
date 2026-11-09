//! Scalar camera arithmetic. The host supplies its Math implementation so
//! V8's lens and turns keep the last bits of the view they replace.
//! No camera policy or object crosses this primitive arithmetic boundary.
use three::{Matrix4, Quaternion as Q, Vector3 as V};

// camera.js, death-shot.js and flight/camera.js used the host Math builtins.
// Their elementary rounding is observable in the final camera matrix bits.
// Keep that choice explicit instead of hiding it in a vector implementation.
// The WASM adapter imports scalar arithmetic, never camera state or objects.
// Native tests use V8's scalar implementation with the same operation order.
// A future native host can bind its compatibility math through this trait.
// None of these functions allocates a result or calls a camera algorithm.
// Square roots and basic f64 operations retain their IEEE hardware behavior.
pub trait Math {
    fn sin(x: f64) -> f64;
    fn cos(x: f64) -> f64;
    fn tan(x: f64) -> f64;
    fn atan(x: f64) -> f64;
    fn atan2(y: f64, x: f64) -> f64;
    fn asin(x: f64) -> f64;
    fn acos(x: f64) -> f64;
    fn exp(x: f64) -> f64;
    fn log(x: f64) -> f64;
    fn hypot2(x: f64, y: f64) -> f64;
    fn hypot(x: f64, y: f64, z: f64) -> f64;
    fn pow(x: f64, y: f64) -> f64;
}

// Native records use the server's V8 arithmetic. A browser host binds its
// own standard math library; the phase still owns every camera operation.
pub struct V8;
impl Math for V8 {
    fn sin(x: f64) -> f64 {
        v8math::sin(x)
    }
    fn cos(x: f64) -> f64 {
        v8math::cos(x)
    }
    fn tan(x: f64) -> f64 {
        v8math::tan(x)
    }
    fn atan(x: f64) -> f64 {
        v8math::atan(x)
    }
    fn atan2(y: f64, x: f64) -> f64 {
        v8math::atan2(y, x)
    }
    fn asin(x: f64) -> f64 {
        v8math::asin(x)
    }
    fn acos(x: f64) -> f64 {
        v8math::acos(x)
    }
    fn exp(x: f64) -> f64 {
        v8math::exp(x)
    }
    fn log(x: f64) -> f64 {
        v8math::log(x)
    }
    // DeathShot.begin/enhanced used two arguments. Appending zero can
    // change V8's compensated sum by one bit even for a finite radius.
    fn hypot2(x: f64, y: f64) -> f64 {
        v8math::hypot(x, y)
    }
    fn hypot(x: f64, y: f64, z: f64) -> f64 {
        v8math::hypot3(x, y, z)
    }
    fn pow(x: f64, y: f64) -> f64 {
        v8math::pow(x, y)
    }
}

pub const PI: f64 = std::f64::consts::PI;
pub const DEG: f64 = PI / 180.0;
pub const UP: V = V {
    x: 0.0,
    y: 1.0,
    z: 0.0,
};
pub fn clamp(x: f64, a: f64, b: f64) -> f64 {
    x.max(a).min(b)
}
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}
pub fn ease(u: f64) -> f64 {
    if u <= 0.0 {
        0.0
    } else if u >= 1.0 {
        1.0
    } else {
        u * u * u * (u * (u * 6.0 - 15.0) + 10.0)
    }
}
pub fn wrap(a: f64) -> f64 {
    a - 2.0 * PI * v8math::round(a / (2.0 * PI))
}
// SoldierCamera.update and FirstPersonMotion.turn use different Euler orders.
// The boolean selects the two authored cases rather than a string lookup.
// Dividing each angle before sin/cos preserves Quaternion.setFromEuler's
// rounding, including zero signs when an axis has no authored motion.
// Multiplications below deliberately keep the original expression grouping.
// These turns are not normalized: Three's Euler conversion did not do so.
pub fn euler<M: Math>(x: f64, y: f64, z: f64, zyx: bool) -> Q {
    let (c1, c2, c3, s1, s2, s3) = (
        M::cos(x / 2.0),
        M::cos(y / 2.0),
        M::cos(z / 2.0),
        M::sin(x / 2.0),
        M::sin(y / 2.0),
        M::sin(z / 2.0),
    );
    // Quaternion.setFromEuler: motion turns use ZYX, every aim uses YXZ.
    if zyx {
        Q::new(
            s1 * c2 * c3 - c1 * s2 * s3,
            c1 * s2 * c3 + s1 * c2 * s3,
            c1 * c2 * s3 - s1 * s2 * c3,
            c1 * c2 * c3 + s1 * s2 * s3,
        )
    } else {
        Q::new(
            s1 * c2 * c3 + c1 * s2 * s3,
            c1 * s2 * c3 - s1 * c2 * s3,
            c1 * c2 * s3 - s1 * s2 * c3,
            c1 * c2 * c3 + s1 * s2 * s3,
        )
    }
}
pub fn axis<M: Math>(v: V, a: f64) -> Q {
    let s = M::sin(a / 2.0);
    Q::new(v.x * s, v.y * s, v.z * s, M::cos(a / 2.0))
}
// These stack-value operations mirror the exact Three methods named at their
// call sites. Returning a value avoids mutable scratch aliases across views.
// Rotation uses applyQuaternion's expanded products without normalization.
// Camera motion depends on preserving even small authored quaternion errors.
// Addition and scaled addition keep Vector3's multiply-then-add expression.
// LLVM uses ordinary f64 arithmetic here, without a fast-math contract.
// This also keeps signed zeros observable in the camera output records.
pub fn rotate(v: V, q: Q) -> V {
    let mut v = v;
    v.apply_quaternion(&q);
    v
}
pub fn add(a: V, b: V) -> V {
    V::new(a.x + b.x, a.y + b.y, a.z + b.z)
}
pub fn sub(a: V, b: V) -> V {
    V::new(a.x - b.x, a.y - b.y, a.z - b.z)
}
pub fn scale(mut v: V, s: f64) -> V {
    v.multiply_scalar(s);
    v
}
pub fn plus(mut a: V, b: V, s: f64) -> V {
    a.add_scaled_vector(&b, s);
    a
}
pub fn mix(mut a: V, b: V, t: f64) -> V {
    a.lerp(&b, t);
    a
}
// Three distinguishes lerp and lerpVectors at several camera call sites.
// Keep those method choices visible rather than replacing them with a new
// interpolation formula or applying an endpoint clamp not present in JS.
// Death handoffs, flight blends and follower results retain their own clocks.
// Neither operation advances or caches an interpolation parameter.
pub fn mix_vectors(mut a: V, b: V, t: f64) -> V {
    let from = a;
    a.lerp_vectors(&from, &b, t);
    a
}
pub fn norm(mut v: V) -> V {
    v.normalize();
    v
}
pub fn inv(mut q: Q) -> Q {
    q.invert();
    q
}
pub fn mul(mut a: Q, b: Q) -> Q {
    a.multiply(&b);
    a
}
pub fn normalize(mut q: Q) -> Q {
    q.normalize();
    q
}
pub fn cross(mut a: V, b: V) -> V {
    a.cross(&b);
    a
}
pub fn transformed(mut v: V, m: &Matrix4) -> V {
    v.apply_matrix4(m);
    v
}
pub fn column(m: &Matrix4, i: usize) -> V {
    V::new(
        m.elements[i * 4],
        m.elements[i * 4 + 1],
        m.elements[i * 4 + 2],
    )
}

// DeathShot's FollowTurn and the flight handoff use Quaternion.slerp.
// Endpoints are returned verbatim, before shortest-arc sign selection.
// Negating the target instead of the source matters to the endpoint bits.
// Near-coincident quaternions take the normalized linear interpolation path.
// The general path uses atan2 for the half angle, as Three did.
// Replacing this with acos changes its rounding near a shared heading.
// No smoothing clock lives here; each caller supplies its own blend fraction.
pub fn slerp<M: Math>(a: Q, b: Q, t: f64) -> Q {
    // Quaternion.slerp keeps its endpoint shortcuts and the near-coincident
    // normalization, including the order of each multiply and add.
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return b;
    }
    let mut b = b;
    let mut cos = a.w * b.w + a.x * b.x + a.y * b.y + a.z * b.z;
    if cos < 0.0 {
        b = Q::new(-b.x, -b.y, -b.z, -b.w);
        cos = -cos;
    }
    if cos >= 1.0 {
        return a;
    }
    let sq = 1.0 - cos * cos;
    if sq <= f64::EPSILON {
        let s = 1.0 - t;
        return normalize(Q::new(
            s * a.x + t * b.x,
            s * a.y + t * b.y,
            s * a.z + t * b.z,
            s * a.w + t * b.w,
        ));
    }
    let sin = sq.sqrt();
    let half = M::atan2(sin, cos);
    let ra = M::sin((1.0 - t) * half) / sin;
    let rb = M::sin(t * half) / sin;
    Q::new(
        a.x * ra + b.x * rb,
        a.y * ra + b.y * rb,
        a.z * ra + b.z * rb,
        a.w * ra + b.w * rb,
    )
}
pub fn slerp_vectors<M: Math>(a: Q, b: Q, t: f64) -> Q {
    slerp::<M>(a, b, t)
}
// Camera.lookAt in the world DeathCamera used Matrix4.lookAt directly.
// The matrix helper retains its degenerate-target and parallel-up handling.
// A rigid quaternion is extracted with the same trace/diagonal branch order.
// This helper intentionally differs from the vehicle lookBasis fallbacks.
// Sharing the two would change vertical or coincident-target camera records.
pub fn look(position: V, target: V, up: V) -> Q {
    let mut m = Matrix4::default();
    m.look_at(&position, &target, &up);
    let mut q = Q::default();
    q.set_from_rotation_matrix(&m);
    q
}
// Ports lookBasis from vehicles.js and props.js, whose positive z is forward.
// Callers reverse that vector for a camera looking down local negative z.
// Authored up may become parallel to forward on vertical flight or a turret.
// The z cross product is the original first fallback, then the x axis.
// Keep the two strict length tests and normalize only after those choices.
// Matrix-to-quaternion conversion keeps Three's branch order and zero signs.
// A camera's pitched up is not substituted for a building's placed up.
pub fn look_basis(back: V, up: V) -> Q {
    let back = norm(back);
    let mut side = cross(up, back);
    if side.length_sq() < 1e-10 {
        side = cross(V::new(0.0, 0.0, 1.0), back)
    }
    if side.length_sq() < 1e-10 {
        side = V::new(1.0, 0.0, 0.0)
    }
    side = norm(side);
    let above = cross(back, side);
    let mut m = Matrix4::default();
    m.make_basis(&side, &above, &back);
    let mut q = Q::default();
    q.set_from_rotation_matrix(&m);
    q
}
// Three updated a camera's Euler fields whenever its quaternion changed.
// Publishing those fields here keeps retained Camera readers consistent
// without invoking a second JavaScript quaternion-to-matrix conversion.
// YXZ is the camera order established by SoldierCamera's constructor.
// The singularity threshold and alternate yaw are Three's original ones.
// The host writes private scalar fields without firing transform callbacks.
// Matrix consumers therefore see one canonical pose from the native phase.
pub fn angles<M: Math>(q: Q) -> V {
    let mut m = Matrix4::default();
    m.make_rotation_from_quaternion(&q);
    let m = m.elements;
    let x = M::asin(-clamp(m[9], -1.0, 1.0));
    if m[9].abs() < 0.9999999 {
        V::new(x, M::atan2(m[8], m[10]), M::atan2(m[1], m[5]))
    } else {
        V::new(x, M::atan2(-m[2], m[0]), 0.0)
    }
}
