//! A plain JavaScript object keyed by team number (`{ 1: 150, 2: 150 }`),
//! as the battle's rules keep their per-team numbers: integer keys iterate
//! in ascending order, and a missing key reads as undefined.

#[derive(Clone, Debug, PartialEq)]
pub struct TeamMap<T> {
    entries: Vec<(f64, T)>,
}

impl<T> Default for TeamMap<T> {
    fn default() -> Self {
        TeamMap { entries: Vec::new() }
    }
}

impl<T> TeamMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `object[team]`, or None for undefined.
    pub fn get(&self, team: f64) -> Option<&T> {
        self.entries.iter().find(|(k, _)| *k == team).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, team: f64) -> Option<&mut T> {
        self.entries.iter_mut().find(|(k, _)| *k == team).map(|(_, v)| v)
    }

    /// `object[team] = value`, keeping the keys in ascending order.
    pub fn set(&mut self, team: f64, value: T) {
        if let Some(slot) = self.get_mut(team) {
            *slot = value;
            return;
        }
        let at = self.entries.iter().position(|(k, _)| *k > team).unwrap_or(self.entries.len());
        self.entries.insert(at, (team, value));
    }

    /// The entries in the object's key order.
    pub fn entries(&self) -> impl Iterator<Item = &(f64, T)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<T: Copy> TeamMap<T> {
    /// `object[team]` as a value (None: undefined).
    pub fn at(&self, team: f64) -> Option<T> {
        self.get(team).copied()
    }
}

impl<T> FromIterator<(f64, T)> for TeamMap<T> {
    fn from_iter<I: IntoIterator<Item = (f64, T)>>(iter: I) -> Self {
        let mut m = TeamMap::new();
        for (k, v) in iter {
            m.set(k, v);
        }
        m
    }
}
