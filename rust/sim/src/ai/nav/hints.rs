//! Tactical hints as the world loader builds them, and the generic hint
//! query (nav/hints.js). Command posts are reached through `HintPosts`
//! (their names and positions with the caller's identity tokens for
//! them), since the posts themselves are the battle package's.

use super::math::{f32, Vec3};
use crate::ai::random::Random;

/// hints.js:21.
pub const POOL_CAPACITY: f64 = 350.0;

/// The stance numbers (hints.js:22).
pub mod stance {
    pub const STAND: f64 = 0.0;
    pub const CROUCH: f64 = 1.0;
    pub const PRONE: f64 = 2.0;
}

/// The hint kind bits (hints.js:20).
pub fn hint_bit(kind: &str) -> u32 {
    match kind {
        "snipe" => 1,
        "patrol" => 2,
        "cover" => 4,
        "jetjump" => 8,
        "minefield" => 16,
        "land" => 32,
        _ => 0,
    }
}

/// A hint record of ai-nav.json as buildHints reads it: `properties` are
/// (hash, value) pairs, entries 0 and 2 of each authored property.
#[derive(Clone, Debug)]
pub struct HintRecord {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub xfrm: [f64; 12],
    pub properties: Vec<(String, String)>,
}

/// What buildHints asks of the command posts (hints.js:101,119).
pub trait HintPosts {
    fn by_name(&self, name: &str) -> Option<u64>;
    /// (token, position) of every post, in list order.
    fn list(&self) -> &[(u64, Vec3)];
}

/// One loaded hint (hints.js:45). `link` and `next` are the same ring
/// neighbour, as written; both are kept because different readers use
/// each. `reserved_by` is the reserving owner's identity token.
#[derive(Clone, Debug)]
pub struct Hint {
    pub index: usize,
    pub source: usize,
    pub copy: i32,
    pub kind: String,
    pub bit: u32,
    pub name: Option<String>,
    pub position: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub primary_stance: i32,
    pub secondary_stance: i32,
    pub secondary_choices: i32,
    pub mode: f64,
    pub radius_enabled: bool,
    pub radius: f64,
    pub command_post: Option<u64>,
    pub command_post_name: Option<String>,
    pub link: Option<usize>,
    pub next: Option<usize>,
    pub siblings: Vec<usize>,
    pub reserved_at: f64,
    pub reserved_by: Option<u64>,
    pub register_radius: f64,
    // The names the stance and cover code uses (hints.js:131).
    pub primary_mask: i32,
    pub secondary_mask: i32,
    pub side_mask: i32,
}

/// sscanf("%d") into a target that holds `fallback` when nothing parses
/// (hints.js:33): leading whitespace, an optional sign, digits, through
/// Number.parseInt and `| 0`.
fn scan_int(text: &str, fallback: i32) -> i32 {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let start = i;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    let digits = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return fallback;
    }
    let text: String = chars[start..i].iter().collect();
    // Number.parseInt of a digit string is the f64 value of it.
    let value: f64 = text.parse().unwrap_or(f64::NAN);
    v8math::to_int32(value)
}

/// hints.js:38: the %f shape its regex accepts, then
/// f32(Number.parseFloat(...)), or the fallback untouched.
fn scan_float(text: &str, fallback: f64) -> f64 {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let start = i;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    let mut digits = 0;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        let mut j = i + 1;
        let mut frac = 0;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
            frac += 1;
        }
        // \d+\.?\d* takes the dot with digits before it; \.\d+ needs a
        // digit after it.
        if digits > 0 || frac > 0 {
            i = j;
            digits += frac;
        }
    }
    if digits == 0 {
        return fallback;
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        let exp_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    let text: String = chars[start..i].iter().collect();
    let value: f64 = text.parse().unwrap_or(f64::NAN);
    f32(value)
}

use super::math::three_normalize;

/// three.js Vector3.crossVectors(a, b).
fn three_cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

fn base_hint(record: &HintRecord, kind: &str, index: usize) -> Hint {
    let m = &record.xfrm;
    Hint {
        index,
        source: index,
        copy: -1,
        kind: kind.to_string(),
        bit: hint_bit(kind),
        name: record.name.clone(),
        position: Vec3::new(m[9], m[10], m[11]),
        right: Vec3::new(m[0], m[1], m[2]),
        up: Vec3::new(m[3], m[4], m[5]),
        forward: Vec3::new(m[6], m[7], m[8]),
        primary_stance: 1,
        secondary_stance: 1,
        secondary_choices: 0,
        mode: 0.0,
        radius_enabled: false,
        radius: -1.0,
        command_post: None,
        command_post_name: None,
        link: None,
        next: None,
        siblings: Vec::new(),
        reserved_at: -10.0,
        reserved_by: None,
        register_radius: 1.0,
        primary_mask: 0,
        secondary_mask: 0,
        side_mask: 0,
    }
}

fn apply_property(hint: &mut Hint, hash: &str, value: &str, posts: Option<&dyn HintPosts>) {
    match hash {
        "bf0f1cd1" => {
            let v = scan_int(value, 0xbf0f1cd1u32 as i32);
            if v > 0 {
                hint.primary_stance = v & 0xff;
            }
        }
        "92c05a59" => {
            let v = scan_int(value, 0x92c05a59u32 as i32);
            if v & 7 != 0 {
                hint.secondary_stance = v & 7;
            }
            hint.secondary_choices = (v & 0x18) >> 3;
        }
        "ec6ee012" => {
            let v = scan_int(value, 0xec6ee012u32 as i32);
            hint.mode = if v == 3 { 0.0 } else { v as f64 };
        }
        "0dba4cb3" => {
            hint.radius = scan_float(value, hint.radius);
            if hint.radius > 0.0 {
                hint.radius_enabled = true;
            }
        }
        "844ccdb4" => {
            if !value.is_empty() {
                hint.command_post_name = Some(value.to_string());
                hint.command_post = posts.and_then(|p| p.by_name(value));
            }
        }
        _ => {}
    }
}

/// The loaded hint list (hints.js:113). `capacity` is the JavaScript's
/// options.capacity, defaulted by the caller.
pub fn build_hints(records: &[HintRecord], posts: Option<&dyn HintPosts>, capacity: f64) -> Vec<Hint> {
    let mut out: Vec<Hint> = Vec::new();
    let mut used = 0.0;
    let register = |out: &mut Vec<Hint>, mut hint: Hint| {
        if hint.kind == "land" && hint.command_post.is_none() {
            if let Some(posts) = posts {
                if !posts.list().is_empty() {
                    let mut best = f64::INFINITY;
                    for (token, position) in posts.list() {
                        let d = super::math::d2(*position, hint.position);
                        if d < best {
                            best = d;
                            hint.command_post = Some(*token);
                        }
                    }
                }
            }
        }
        hint.register_radius = if hint.kind == "minefield" { hint.radius } else { 1.0 };
        hint.primary_mask = hint.primary_stance;
        hint.secondary_mask = hint.secondary_stance;
        hint.side_mask = hint.secondary_choices;
        hint.index = out.len();
        out.push(hint);
    };
    for (source, record) in records.iter().enumerate() {
        let Some(kind) = record.kind.as_deref().filter(|k| hint_bit(k) != 0) else {
            continue;
        };
        let kind = kind.to_string();
        if used >= capacity {
            continue;
        }
        let mut hint = base_hint(record, &kind, source);
        used += 1.0;
        for (hash, value) in &record.properties {
            apply_property(&mut hint, hash, value, posts);
        }
        if hint.radius_enabled && hint.kind != "minefield" {
            if used + 4.0 <= capacity {
                let mut ring = Vec::new();
                for i in 0..4 {
                    let a = i as f64 * f32(std::f64::consts::PI / 2.0);
                    let offset = Vec3::new(v8math::sin(a) * hint.radius, 0.0, v8math::cos(a) * hint.radius);
                    let mut copy = hint.clone();
                    copy.copy = i;
                    copy.position = Vec3::new(hint.position.x + offset.x, hint.position.y + offset.y, hint.position.z + offset.z);
                    let mut f = Vec3::new(-offset.x, 0.0, -offset.z);
                    if f.x * f.x + f.y * f.y + f.z * f.z == 0.0 {
                        f = Vec3::new(-1.0, 0.0, 0.0);
                    }
                    three_normalize(&mut f);
                    copy.forward = f;
                    copy.right = three_cross(Vec3::new(0.0, 1.0, 0.0), f);
                    three_normalize(&mut copy.right);
                    copy.up = three_cross(f, copy.right);
                    three_normalize(&mut copy.up);
                    used += 1.0;
                    register(&mut out, copy);
                    ring.push(out.len() - 1);
                }
                for i in 0..4 {
                    let next = ring[(i + 1) % 4];
                    out[ring[i]].link = Some(next);
                    out[ring[i]].next = Some(next);
                    out[ring[i]].siblings = ring.clone();
                }
            }
            used -= 1.0;
            continue;
        }
        register(&mut out, hint);
    }
    out
}

/// A stance from a stance mask (hints.js:179).
pub fn pick_stance(mask: i32, random: &mut dyn Random) -> f64 {
    if mask & 7 == 0 {
        return stance::STAND;
    }
    for _ in 0..64 {
        let s = random.int(0.0, 2.0);
        if (mask >> (s as i32)) & 1 != 0 {
            return s;
        }
    }
    stance::STAND
}

/// A secondary action from the two-bit choice mask (hints.js:189).
pub fn pick_secondary(choices: i32, random: &mut dyn Random) -> f64 {
    if choices & 3 == 0 {
        return -1.0;
    }
    for _ in 0..64 {
        let s = random.int(0.0, 1.0);
        if (choices >> (s as i32)) & 1 != 0 {
            return s;
        }
    }
    -1.0
}

/// HintSet (hints.js:198).
#[derive(Clone, Debug, Default)]
pub struct HintSet {
    pub list: Vec<Hint>,
}

impl HintSet {
    pub fn new(hints: Vec<Hint>) -> Self {
        HintSet { list: hints }
    }

    /// Hints of `mask` whose registration sphere meets the box
    /// center +/- half, at most `cap`, in load order (hints.js:215).
    pub fn query(&self, mask: u32, center: Vec3, half: f64, cap: f64, owner: Option<u64>, now: f64) -> Vec<usize> {
        let mut out = Vec::new();
        self.query_into(mask, center, half, cap, owner, now, &mut out);
        out
    }

    /// The phase supplies reserved query storage; every query replaces its result.
    pub fn query_into(&self, mask: u32, center: Vec3, half: f64, cap: f64, owner: Option<u64>, now: f64, out: &mut Vec<usize>) {
        out.clear();
        for h in &self.list {
            if h.bit & mask == 0 {
                continue;
            }
            let r = h.register_radius;
            let p = h.position;
            if v8math::abs(p.x - center.x) > half + r || v8math::abs(p.y - center.y) > half + r || v8math::abs(p.z - center.z) > half + r {
                continue;
            }
            if owner.is_some() && !self.available(h.index, now) {
                continue;
            }
            out.push(h.index);
            if out.len() as f64 >= cap {
                break;
            }
        }
    }

    /// hints.js:229.
    pub fn available(&self, hint: usize, now: f64) -> bool {
        !(now < self.list[hint].reserved_at + 4.0)
    }

    /// Mark a hint and its ring siblings reserved (hints.js:234).
    pub fn reserve(&mut self, hint: usize, owner: Option<u64>, now: f64) {
        let mut h = Some(hint);
        while let Some(i) = h {
            self.list[i].reserved_at = now;
            self.list[i].reserved_by = owner;
            h = self.list[i].link;
            if h == Some(hint) {
                break;
            }
        }
    }
}
