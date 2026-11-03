//! The three.js r170 arithmetic the rigs are built on (rust/three: the
//! Vector3, Quaternion, Matrix4, Euler and Box3 methods and the
//! TransformTree semantics rust/sim/src/rig uses) against three itself:
//! cases tools/sim-rig/three-cases.mjs recorded under the production
//! server's Node, replayed here, every number compared by its bits. The
//! transcendental functions are rust/v8math's (through jsrt's seam); a call
//! the rigs make through rig::math is also checked against what V8 answered
//! in the recording (its oracle): a different answer is a mismatch, and a
//! call the JavaScript never made is a miss.
//!
//!   node tools/sim-rig/three-cases.mjs CASES.bfecap
//!   BFE_RIG_THREE=CASES.bfecap cargo test -p sim --features replay --test rig_three
//!
//! Without BFE_RIG_THREE the test passes without replaying.
#![cfg(feature = "replay")]

use bfe_sim::rig::math::oracle::{self, Oracle};
use three::{clamp, lerp, smoothstep, Box3, Euler, EulerOrder, Matrix4, Quaternion, Vector3};
use three::{TransformNode, TransformTree};
use sim_replay::{Reader, V};

const ORDERS: [EulerOrder; 6] = [EulerOrder::XYZ, EulerOrder::YXZ, EulerOrder::ZXY, EulerOrder::ZYX, EulerOrder::YZX, EulerOrder::XZY];

fn floats(v: V) -> Vec<f64> {
    match v.typed() {
        Some(sim_replay::Typed::F64(x)) => x.clone(),
        other => panic!("expected a Float64Array, got {:?}", other.map(|t| t.class())),
    }
}

fn v3(a: &[f64]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn q4(a: &[f64]) -> Quaternion {
    Quaternion::new(a[0], a[1], a[2], a[3])
}
fn m16(a: &[f64]) -> Matrix4 {
    let mut m = Matrix4::default();
    m.from_array(a, 0);
    m
}
fn of_v(v: &Vector3) -> Vec<f64> {
    vec![v.x, v.y, v.z]
}
fn of_q(q: &Quaternion) -> Vec<f64> {
    vec![q.x, q.y, q.z, q.w]
}

/// The first case of a method whose answer differs in any bit.
fn compare(name: &str, case: usize, want: &[f64], got: &[f64]) -> Result<(), String> {
    assert_eq!(want.len(), got.len(), "{name}: answer length");
    for (k, (w, g)) in want.iter().zip(got).enumerate() {
        if w.to_bits() != g.to_bits() && !(w.is_nan() && g.is_nan()) {
            return Err(format!("{name} case {case} value {k}: three {w:e} ({:#018x}), Rust {g:e} ({:#018x}); all three {want:?}, Rust {got:?}", w.to_bits(), g.to_bits()));
        }
    }
    Ok(())
}

fn answer(name: &str, x: &[f64]) -> Vec<f64> {
    match name {
        "v3.applyQuaternion" => of_v(v3(x).apply_quaternion(&q4(&x[3..]))),
        "v3.applyMatrix4" => of_v(v3(x).apply_matrix4(&m16(&x[3..]))),
        "v3.transformDirection" => of_v(v3(x).transform_direction(&m16(&x[3..]))),
        "v3.normalize" => of_v(v3(x).normalize()),
        "v3.lerpVectors" => {
            let (a, b, t) = (v3(x), v3(&x[3..]), x[6]);
            let mut out = of_v(Vector3::default().lerp_vectors(&a, &b, t));
            out.extend(of_v(a.clone().lerp(&b, t)));
            out
        }
        "v3.crossVectors" => of_v(Vector3::default().cross_vectors(&v3(x), &v3(&x[3..]))),
        "q.setFromEuler" => of_q(Quaternion::default().set_from_euler(&Euler::new(x[0], x[1], x[2], ORDERS[x[3] as usize]))),
        "q.setFromAxisAngle" => of_q(Quaternion::default().set_from_axis_angle(&v3(x), x[3])),
        "q.setFromRotationMatrix" => of_q(Quaternion::default().set_from_rotation_matrix(&m16(x))),
        "q.normalize" => of_q(q4(x).normalize()),
        "q.multiplyQuaternions" => {
            let (a, b) = (q4(x), q4(&x[4..]));
            let mut out = of_q(Quaternion::default().multiply_quaternions(&a, &b));
            out.extend(of_q(a.clone().premultiply(&b)));
            out
        }
        "q.slerp" => {
            let (a, b, t) = (q4(x), q4(&x[4..]), x[8]);
            let mut out = of_q(a.clone().slerp(&b, t));
            out.extend(of_q(Quaternion::default().slerp_quaternions(&b, &a, t)));
            out
        }
        "m4.compose" => Matrix4::default().compose(&v3(x), &q4(&x[3..]), &v3(&x[7..])).elements.to_vec(),
        "m4.decompose" => {
            let (mut p, mut q, mut s) = (Vector3::default(), Quaternion::default(), Vector3::default());
            m16(x).decompose(&mut p, &mut q, &mut s);
            [of_v(&p), of_q(&q), of_v(&s)].concat()
        }
        "m4.multiplyMatrices" => Matrix4::default().multiply_matrices(&m16(x), &m16(&x[16..])).elements.to_vec(),
        "m4.invert" => {
            let m = m16(x);
            let mut out = m.clone().invert().elements.to_vec();
            out.push(m.determinant());
            out
        }
        "m4.extractRotation" => Matrix4::default().extract_rotation(&m16(x)).elements.to_vec(),
        "m4.makeBasis" => Matrix4::default().make_basis(&v3(x), &v3(&x[3..]), &v3(&x[6..])).elements.to_vec(),
        "euler.setFromRotationMatrix" => {
            let e = *Euler::default().set_from_rotation_matrix(&m16(x), Some(ORDERS[x[16] as usize]));
            vec![e.x, e.y, e.z]
        }
        "euler.setFromQuaternion" => {
            let e = *Euler::default().set_from_quaternion(&q4(x), Some(ORDERS[x[4] as usize]));
            vec![e.x, e.y, e.z]
        }
        "mathutils" => vec![clamp(x[0], x[1], x[2]), smoothstep(x[0], x[1], x[2]), lerp(x[0], x[1], x[2])],
        "box3.center" => {
            let n = x[0] as usize;
            let points: Vec<Vector3> = (0..n).map(|i| v3(&x[1 + i * 3..])).collect();
            let mut b = Box3::default();
            b.set_from_points(&points);
            let mut c = Vector3::default();
            b.get_center(&mut c);
            vec![c.x, c.y, c.z, if b.is_empty() { 1.0 } else { 0.0 }]
        }
        other => panic!("no Rust answer for {other}"),
    }
}

/// An Object3D script (three-cases.mjs OPS), run on a TransformTree; the
/// values it reads, in order.
fn run_script(parents: &[f64], ops: &[f64]) -> Vec<f64> {
    let mut tree = TransformTree::new();
    let mut nodes = Vec::new();
    for (i, &p) in parents.iter().enumerate() {
        nodes.push(tree.add_node(TransformNode::default()));
        if p >= 0.0 {
            tree.add_child(nodes[p as usize], nodes[i]);
        }
    }
    const ARGS: [usize; 24] = [3, 4, 4, 4, 4, 0, 5, 4, 1, 1, 1, 1, 0, 1, 2, 0, 0, 0, 3, 1, 16, 9, 0, 0];
    let mut reads = Vec::new();
    let mut at = 0;
    while at < ops.len() {
        let code = ops[at] as usize;
        let id = nodes[ops[at + 1] as usize];
        let a = &ops[at + 2..at + 2 + ARGS[code]];
        at += 2 + ARGS[code];
        match code {
            0 => {
                tree.node_mut(id).position.set(a[0], a[1], a[2]);
            }
            1 => tree.node_mut(id).copy_quaternion(&q4(a)),
            2 => {
                let axis = v3(a);
                tree.node_mut(id).with_quaternion(|q| {
                    q.set_from_axis_angle(&axis, a[3]);
                })
            }
            3 => tree.node_mut(id).with_quaternion(|q| {
                q.multiply(&q4(a));
            }),
            4 => tree.node_mut(id).with_quaternion(|q| {
                q.premultiply(&q4(a));
            }),
            5 => tree.node_mut(id).with_quaternion(|q| {
                q.normalize();
            }),
            6 => tree.node_mut(id).slerp_quaternion(&q4(a), a[4]),
            7 => tree.node_mut(id).set_rotation(a[0], a[1], a[2], if a[3] < 0.0 { None } else { Some(ORDERS[a[3] as usize]) }),
            8 => tree.node_mut(id).set_rotation_x(a[0]),
            9 => tree.node_mut(id).set_rotation_y(a[0]),
            10 => tree.node_mut(id).set_rotation_z(a[0]),
            11 => tree.node_mut(id).set_rotation_order(ORDERS[a[0] as usize]),
            12 => {
                let node = tree.node_mut(id);
                let e = node.rotation();
                reads.extend([e.x, e.y, e.z, ORDERS.iter().position(|&o| o == e.order).unwrap() as f64]);
            }
            13 => tree.update_matrix_world(id, a[0] != 0.0),
            14 => tree.update_world_matrix(id, a[0] != 0.0, a[1] != 0.0),
            15 => {
                let mut p = Vector3::default();
                reads.extend(of_v(&tree.get_world_position(id, &mut p)));
            }
            16 => {
                let mut q = Quaternion::default();
                reads.extend(of_q(&tree.get_world_quaternion(id, &mut q)));
            }
            17 => {
                let node = tree.node(id);
                reads.extend(node.matrix_world.elements);
                reads.push(if node.matrix_world_needs_update { 1.0 } else { 0.0 });
            }
            18 => {
                tree.node_mut(id).scale.set(a[0], a[1], a[2]);
            }
            19 => {
                let copy = tree.clone_node(id);
                if a[0] >= 0.0 {
                    tree.add_child(nodes[a[0] as usize], copy);
                }
                nodes.push(copy);
            }
            20 => tree.node_mut(id).decompose_from(&m16(a)),
            21 => tree.node_mut(id).slerp_quaternions(&q4(a), &q4(&a[4..]), a[8]),
            22 => tree.node_mut(id).euler_sync = false,
            23 => reads.extend(of_q(&tree.node(id).quaternion)),
            other => panic!("op {other}"),
        }
    }
    reads
}

#[test]
fn three_subset_matches_r170() {
    let Ok(path) = std::env::var("BFE_RIG_THREE") else {
        eprintln!("BFE_RIG_THREE unset: nothing replayed");
        return;
    };
    let bytes = std::fs::read(&path).expect("the cases");
    let mut reader = Reader::new(&bytes).expect("a BFECAP file");
    let mut records = Vec::new();
    while let Some(r) = reader.next_record() {
        records.push(r.expect("a record"));
    }
    let mut table = Oracle::new();
    for r in &records {
        if r.name == "three.oracle" {
            table.insert_table(&floats(r.root().get("table")));
        }
    }
    let answers = table.len();
    let mut failures = Vec::new();
    let mut checked = 0usize;
    let ((), table) = oracle::with(table, || {
        for r in &records {
            let name = r.name.strip_prefix("three.").unwrap_or("");
            let root = r.root();
            if name == "object3d" {
                for (k, script) in root.get("scripts").items().enumerate() {
                    let reads = run_script(&floats(script.get("parents")), &floats(script.get("ops")));
                    if let Err(e) = compare("object3d", k, &floats(script.get("reads")), &reads) {
                        failures.push(e);
                    }
                    checked += 1;
                }
                continue;
            }
            if name.is_empty() || name == "oracle" || r.kind != sim_replay::Kind::Frame {
                continue;
            }
            let n = root.get("n").num() as usize;
            let (si, so) = (root.get("inStride").num() as usize, root.get("outStride").num() as usize);
            let (inputs, outputs) = (floats(root.get("inputs")), floats(root.get("outputs")));
            for case in 0..n {
                let got = answer(name, &inputs[case * si..(case + 1) * si]);
                if let Err(e) = compare(name, case, &outputs[case * so..(case + 1) * so], &got) {
                    failures.push(e);
                    break;
                }
                checked += 1;
            }
        }
    });
    eprintln!("{checked} cases checked, {} oracle answers ({} calls agreed, {} misses, {} mismatches)", answers, table.answered, table.misses.len(), table.mismatches.len());
    for (f, a, b, v8, mine) in table.mismatches.iter().take(5) {
        eprintln!("mismatch: {}({a:e}, {b:e}): V8 {v8:e}, v8math {mine:e}", f.name());
    }
    for (f, a, b) in table.misses.iter().take(5) {
        eprintln!("miss: {}({a:e}, {b:e})", f.name());
    }
    assert!(failures.is_empty(), "{} methods differ from three r170:\n{}", failures.len(), failures.join("\n"));
    assert!(table.misses.is_empty(), "{} transcendental calls the JavaScript never made", table.misses.len());
    assert!(table.mismatches.is_empty(), "{} transcendental answers not V8's", table.mismatches.len());
}
