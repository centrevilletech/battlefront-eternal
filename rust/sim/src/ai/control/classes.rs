//! The class jobs' data and pure helpers (classes.js): the repair
//! service book, health types, ammo and placement arithmetic, and
//! perception's priority heap as the snipe selection uses it (float
//! priorities; perception's own heaps store ints). The behavior is
//! the core's dispatch (mod.rs).

use super::world::{AiUnit, EntRef, QtySnap, UKey};

pub const FLT_MAX: f64 = 3.4028234663852886e38;
pub const INT_MAX: f64 = 2147483647.0;

pub const HEALTH_TYPES: [&str; 7] = ["Person", "Animal", "Droid", "Vehicle", "Building", "BuildingRebuild", "BuildingBuild"];

/// healthType (classes.js:250).
pub fn health_type(unit: &AiUnit) -> f64 {
    if let Some(c) = &unit.class_def {
        if let Some(n) = c.health_type_num {
            return n;
        }
        if let Some(name) = &c.health_type_name {
            let lower = name.to_lowercase();
            for (i, known) in HEALTH_TYPES.iter().enumerate() {
                if known.to_lowercase() == lower {
                    return i as f64;
                }
            }
            return 0.0;
        }
    }
    0.0
}

pub fn unit_health(unit: &AiUnit) -> f64 {
    unit.combat_health.or(unit.health).unwrap_or(0.0)
}

pub fn unit_maximum(unit: &AiUnit) -> f64 {
    unit.combat_max_health
        .or(unit.max_health)
        .or_else(|| unit.class_def.as_ref().and_then(|c| c.max_health))
        .unwrap_or(0.0)
}

pub fn unit_radius(unit: &AiUnit) -> f64 {
    unit.sphere_radius.or(unit.radius).unwrap_or(0.0)
}

/// remainingAmmo (classes.js:147).
pub fn remaining_ammo(weapon: &super::world::WeaponSnap) -> f64 {
    let rounds_ge = weapon.rounds.map(|r| r >= INT_MAX).unwrap_or(false);
    let reserve_ge = weapon.reserve.map(|r| r >= FLT_MAX).unwrap_or(false);
    if weapon.bottomless || rounds_ge || reserve_ge {
        return INT_MAX;
    }
    v8math::trunc(v8math::fround(
        v8math::fround(weapon.clip.unwrap_or(0.0) + weapon.reserve.unwrap_or(0.0)) * weapon.rounds.unwrap_or(1.0),
    ))
}

/// needs (classes.js:265).
pub fn needs(target: &AiUnit, ammo: bool) -> bool {
    if !ammo {
        return unit_health(target) < unit_maximum(target);
    }
    target.weapons.iter().any(|w| {
        let cap = match w.def_ammo {
            Some(a) if a > 0.0 => a,
            _ => FLT_MAX,
        };
        w.reserve.map(|r| r < cap - 1.0).unwrap_or(false)
    })
}

/// serviceAlive (classes.js:269).
pub fn service_alive(target: &AiUnit) -> bool {
    !target.removed && (target.alive || health_type(target) >= 5.0)
}

/// quantity (classes.js:274).
pub fn quantity(qty: &QtySnap, ty: f64, ammo: bool) -> f64 {
    let idx = if ty >= 0.0 && ty == v8math::trunc(ty) { ty as usize } else { return 0.0 };
    if idx >= 7 {
        return 0.0;
    }
    let svc = if ammo { qty.svc_ammo } else { qty.svc_health };
    if let Some(arr) = svc {
        return arr[idx].unwrap_or(0.0);
    }
    if ty >= 5.0 && !ammo {
        return qty.bare[idx].unwrap_or(0.0);
    }
    (if ammo { qty.named_ammo } else { qty.named_health })[idx].unwrap_or(0.0)
}

/// A repair reservation (classes.js RepairServices): the job, its
/// owner and when it was last touched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reservation {
    pub target: EntRef,
    pub ammo: bool,
    pub owner: EntRef,
    pub time: f64,
}

/// A given-up job (classes.js futile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Futile {
    pub owner: UKey,
    pub generation: f64,
    pub target: UKey,
    pub ammo: bool,
    pub until: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepairServices {
    pub reservations: Vec<Reservation>,
    pub futile: Vec<Futile>,
}

/// PriorityHeap (perception.js:66) with float priorities, as the
/// snipe hint heap uses it (classes.js:84). Capacity drop rule and
/// tie behavior as written; sentinel 0 in slot 0.
pub struct FloatHeap {
    capacity: usize,
    priority: Vec<f64>,
    items: Vec<Option<usize>>,
    count: usize,
    pub dropped: usize,
}

impl FloatHeap {
    pub fn new(capacity: usize) -> Self {
        let mut priority = vec![0.0; capacity + 1];
        priority[0] = 0.0;
        FloatHeap { capacity, priority, items: vec![None; capacity + 1], count: 0, dropped: 0 }
    }
    pub fn size(&self) -> usize {
        self.count
    }
    fn place(&mut self, mut k: usize, priority: f64, item: usize) {
        let priority = v8math::fround(priority);
        let mut parent = k >> 1;
        while self.priority[parent] < priority {
            self.priority[k] = self.priority[parent];
            self.items[k] = self.items[parent];
            k = parent;
            parent = k >> 1;
        }
        self.priority[k] = priority;
        self.items[k] = Some(item);
    }
    fn min_index(&self) -> usize {
        let mut best = self.count >> 1;
        let mut i = best + 1;
        while i <= self.count {
            if self.priority[i] < self.priority[best] {
                best = i;
            }
            i += 1;
        }
        best - 1
    }
    /// Returns the displaced item, the newcomer when dropped, or None.
    pub fn insert(&mut self, priority: f64, item: usize) -> Option<usize> {
        if self.count < self.capacity {
            self.count += 1;
            self.place(self.count, priority, item);
            return None;
        }
        if self.count == 0 {
            self.dropped += 1;
            return Some(item);
        }
        let slot = self.min_index() + 1;
        if self.priority[slot] < v8math::fround(priority) {
            let old = self.items[slot];
            self.place(slot, priority, item);
            return old;
        }
        self.dropped += 1;
        Some(item)
    }
    pub fn pop(&mut self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let top = self.items[1];
        let n = self.count;
        let vp = self.priority[n];
        let vi = self.items[n];
        self.items[n] = None;
        self.count = n - 1;
        let mut i = 1usize;
        if self.count >= 2 {
            loop {
                let mut c = i * 2;
                if c < self.count && self.priority[c] < self.priority[c + 1] {
                    c += 1;
                }
                if vp >= self.priority[c] {
                    break;
                }
                self.priority[i] = self.priority[c];
                self.items[i] = self.items[c];
                i = c;
                if !(c <= (self.count >> 1)) {
                    break;
                }
            }
        }
        if self.count >= 1 {
            self.priority[i] = vp;
            self.items[i] = vi;
        }
        top
    }
}
