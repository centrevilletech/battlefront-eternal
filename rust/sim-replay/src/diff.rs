//! Compares two values (a capture's and a port's, or two captures') the way
//! the port must match: numbers bit for bit as `Object.is` compares them
//! (-0 is not 0, NaN is NaN), strings by code unit, objects by class, key
//! order and values, and identity: where one side meets the same object
//! twice, the other must too. The first difference comes back with its path.

use crate::value::{same_number, Node, Value, V};
use std::collections::HashMap;

/// Where two values first differ, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    /// As JavaScript would write it: `bolts[3].position.x`, `owned<key "x">`.
    pub path: String,
    pub left: String,
    pub right: String,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} | {}", if self.path.is_empty() { "(the value)" } else { &self.path }, self.left, self.right)
    }
}

/// The first difference between `a` and `b`, or None when they are the same.
pub fn diff(a: V, b: V) -> Option<Difference> {
    let mut d = Differ { pairs: HashMap::new(), back: HashMap::new() };
    d.value(a, b, &mut String::new())
}

/// Whether `a` and `b` are the same as `diff` sees it.
pub fn same(a: V, b: V) -> bool {
    diff(a, b).is_none()
}

struct Differ {
    // Nodes already paired: a's to b's and back, so identity must line up.
    pairs: HashMap<u32, u32>,
    back: HashMap<u32, u32>,
}

fn differ(path: &str, a: V, b: V) -> Option<Difference> {
    Some(Difference { path: path.to_string(), left: a.show(), right: b.show() })
}

impl Differ {
    fn value(&mut self, a: V, b: V, path: &mut String) -> Option<Difference> {
        match (a.value, b.value) {
            (Value::Num(x), Value::Num(y)) => (!same_number(*x, *y)).then(|| differ(path, a, b)).flatten(),
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => None,
            (Value::Bool(x), Value::Bool(y)) => (x != y).then(|| differ(path, a, b)).flatten(),
            (Value::Str(x), Value::Str(y)) => (x != y).then(|| differ(path, a, b)).flatten(),
            (Value::Str16(x), Value::Str16(y)) => (x != y).then(|| differ(path, a, b)).flatten(),
            (Value::Function(x), Value::Function(y)) => (x != y).then(|| differ(path, a, b)).flatten(),
            (Value::Omitted(x), Value::Omitted(y)) => (x != y).then(|| differ(path, a, b)).flatten(),
            (Value::Handle(k1, key1), Value::Handle(k2, key2)) => {
                // A handle's key is a number or a string: no nodes to pair.
                if k1 != k2 || diff(a.arena.view(key1), b.arena.view(key2)).is_some() {
                    return differ(path, a, b);
                }
                None
            }
            (Value::Node(i), Value::Node(j)) => {
                match (self.pairs.get(i), self.back.get(j)) {
                    (Some(pj), Some(pi)) if pj == j && pi == i => return None,
                    (None, None) => {}
                    _ => return Some(Difference { path: path.to_string(), left: format!("{} (object #{i}, met before as another)", a.show()), right: format!("{} (object #{j})", b.show()) }),
                }
                self.pairs.insert(*i, *j);
                self.back.insert(*j, *i);
                self.node(a, b, a.arena.node(*i), b.arena.node(*j), path)
            }
            _ => differ(path, a, b),
        }
    }

    fn floats(&self, x: &[f64], y: &[f64], names: &[&str], path: &str) -> Option<Difference> {
        x.iter().zip(y).enumerate().find(|(_, (p, q))| !same_number(**p, **q)).map(|(k, (p, q))| Difference {
            path: match names.get(k) {
                Some(name) => format!("{path}.{name}"),
                None => format!("{path}.elements[{k}]"),
            },
            left: crate::value::show_number(*p),
            right: crate::value::show_number(*q),
        })
    }

    fn node(&mut self, a: V, b: V, x: &Node, y: &Node, path: &mut String) -> Option<Difference> {
        let len = path.len();
        let result = match (x, y) {
            (Node::Array(p), Node::Array(q)) | (Node::Set(p), Node::Set(q)) => {
                if p.len() != q.len() {
                    return Some(Difference { path: format!("{path}.length"), left: p.len().to_string(), right: q.len().to_string() });
                }
                let set = matches!(x, Node::Set(_));
                p.iter().zip(q).enumerate().find_map(|(k, (u, w))| {
                    path.truncate(len);
                    path.push_str(&if set { format!("<set {k}>") } else { format!("[{k}]") });
                    self.value(a.arena.view(u), b.arena.view(w), path)
                })
            }
            (Node::Object { class: c1, fields: f1 }, Node::Object { class: c2, fields: f2 }) => {
                if c1 != c2 {
                    return Some(Difference { path: format!("{path} (class)"), left: format!("{c1:?}"), right: format!("{c2:?}") });
                }
                for (k, ((n1, v1), (n2, v2))) in f1.iter().zip(f2).enumerate() {
                    if n1 != n2 {
                        return Some(Difference { path: format!("{path} (key {k})"), left: n1.clone(), right: n2.clone() });
                    }
                    path.truncate(len);
                    path.push('.');
                    path.push_str(n1);
                    if let Some(d) = self.value(a.arena.view(v1), b.arena.view(v2), path) {
                        return Some(d);
                    }
                }
                path.truncate(len);
                if f1.len() != f2.len() {
                    let extra = if f1.len() > f2.len() { &f1[f2.len()].0 } else { &f2[f1.len()].0 };
                    return Some(Difference { path: format!("{path}.{extra}"), left: if f1.len() > f2.len() { "present".into() } else { "absent".into() }, right: if f1.len() > f2.len() { "absent".into() } else { "present".into() } });
                }
                None
            }
            (Node::Vec3(p), Node::Vec3(q)) => self.floats(p, q, &["x", "y", "z"], path),
            (Node::Quat(p), Node::Quat(q)) => self.floats(p, q, &["x", "y", "z", "w"], path),
            (Node::Mat4(p), Node::Mat4(q)) => self.floats(p, q, &[], path),
            (Node::Mat3(p), Node::Mat3(q)) => self.floats(p, q, &[], path),
            (Node::Box3(p), Node::Box3(q)) => self.floats(p, q, &["min.x", "min.y", "min.z", "max.x", "max.y", "max.z"], path),
            (Node::Euler(p, o1), Node::Euler(q, o2)) => {
                if o1 != o2 {
                    return Some(Difference { path: format!("{path}.order"), left: o1.clone(), right: o2.clone() });
                }
                self.floats(p, q, &["x", "y", "z"], path)
            }
            (Node::Typed(p), Node::Typed(q)) => p.same(q).map(|k| Difference {
                path: format!("{path}[{k}]"),
                left: format!("{} {:?}", p.class(), p.get(k)),
                right: format!("{} {:?}", q.class(), q.get(k)),
            }),
            (Node::Map(p), Node::Map(q)) => {
                if p.len() != q.len() {
                    return Some(Difference { path: format!("{path}.size"), left: p.len().to_string(), right: q.len().to_string() });
                }
                p.iter().zip(q).enumerate().find_map(|(k, ((k1, v1), (k2, v2)))| {
                    path.truncate(len);
                    path.push_str(&format!("<map {k} key>"));
                    if let Some(d) = self.value(a.arena.view(k1), b.arena.view(k2), path) {
                        return Some(d);
                    }
                    path.truncate(len);
                    path.push_str(&format!("<{}>", a.arena.view(k1).show()));
                    self.value(a.arena.view(v1), b.arena.view(v2), path)
                })
            }
            _ => differ(path, a, b),
        };
        path.truncate(len);
        result
    }
}
