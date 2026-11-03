//! Interned names: class, weapon, model, side and base names as small ids,
//! so the simulation's Rust compares and keys by number where the
//! JavaScript compares strings. Ids are given in the order names are first
//! interned, so the same content interned the same way gives the same ids
//! on every host.

use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub u32);

#[derive(Clone, Debug, Default)]
pub struct Names {
    list: Vec<Rc<str>>,
    index: HashMap<Rc<str>, Name>,
}

impl Names {
    /// The name's id, made the first time it is asked for.
    pub fn intern(&mut self, text: &str) -> Name {
        if let Some(&name) = self.index.get(text) {
            return name;
        }
        let name = Name(self.list.len() as u32);
        let text: Rc<str> = Rc::from(text);
        self.list.push(text.clone());
        self.index.insert(text, name);
        name
    }

    /// The id of a name already interned.
    pub fn get(&self, text: &str) -> Option<Name> {
        self.index.get(text).copied()
    }

    pub fn text(&self, name: Name) -> &str {
        &self.list[name.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}
