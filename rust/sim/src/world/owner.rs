//! One battle lifetime for canonical rows, phase services and page resources.
//! Cold storage exposes fixed WASM addresses without allocating. Warm admission
//! reserves the exchange core at the same first Sim access as the host bridge.
use crate::abi::Sim;

// One initializer list checks COLD's completeness and initializes its boxed
// twin field by field. Adding an owner cannot leave uninitialized heap data.
macro_rules! cold_fields {
    ($build:ident) => {
        $build! {
            sim: Sim::cold(), tick: super::Tick::cold(),
            input: crate::input::owner::Owner::new(),
            ai: crate::ai::owned::owner::Owner::new(),
            wire: None, page: crate::client::resources::Resources::new(),
            collision: crate::collision::CollisionOwner::new(),
            movement: crate::movement::MovementOwner::new(),
            pose: crate::pose::State::new(),
            units: crate::units::owner::Units::new(),
            fleet: crate::vehicles::module::Fleet::new(),
            battle: crate::battle::owner::BattleOwner::new(),
            combat: crate::combat::module::CombatOwner::new(),
            ordnance: crate::ordnance::module::OrdnanceOwnerState::new(),
            cameras: crate::cameras::Owner::new(),
            page_body: crate::units::page_body::Owner::new(), warmed: false,
        }
    };
}
macro_rules! cold_value {
    ($($(#[$attribute:meta])* $field:ident: $value:expr,)*) => {
        Self { $($(#[$attribute])* $field: $value,)* }
    };
}
macro_rules! cold_box {
    ($($(#[$attribute:meta])* $field:ident: $value:expr,)*) => {{
        let mut allocation = Box::<Self>::new_uninit();
        let pointer = allocation.as_mut_ptr();
        // SAFETY: the same exhaustive list builds COLD. Every field is written
        // once by const constructors that neither allocate nor panic.
        unsafe {
            $($(#[$attribute])* std::ptr::addr_of_mut!((*pointer).$field).write($value);)*
            allocation.assume_init()
        }
    }};
}

pub struct World {
    pub(crate) sim: Sim,
    pub(crate) tick: super::Tick,
    pub(crate) input: crate::input::owner::Owner,
    pub(crate) ai: crate::ai::owned::owner::Owner,
    // The cold root cannot contain a Box before admission. Publication retains
    // its existing take/return loan until its callers receive explicit borrows.
    pub(crate) wire: Option<Box<crate::wire::owner::Owner>>,
    pub(crate) page: crate::client::resources::Resources,
    pub(crate) collision: crate::collision::CollisionOwner,
    pub(crate) movement: crate::movement::MovementOwner,
    pub(crate) pose: crate::pose::State,
    // Every phase belongs to this battle on both host targets.
    pub(crate) units: crate::units::owner::Units,
    pub(crate) fleet: crate::vehicles::module::Fleet,
    pub(crate) battle: crate::battle::owner::BattleOwner,
    pub(crate) combat: crate::combat::module::CombatOwner,
    pub(crate) ordnance: crate::ordnance::module::OrdnanceOwnerState,
    pub(crate) cameras: crate::cameras::Owner,
    pub(crate) page_body: crate::units::page_body::Owner,
    pub(crate) warmed: bool,
}
impl World {
    pub const COLD: Self = cold_fields!(cold_value);
    /// Sim's old construction order: tick output, wire box, kind blocks, events.
    /// Repeated admission leaves every retained bank and logical identity intact.
    pub fn warm(&mut self) {
        if self.warmed { return; }
        self.tick.warm();
        self.wire = Some(Box::new(crate::wire::owner::Owner::new()));
        self.sim.warm();
        self.warmed = true;
    }
    pub fn new() -> Box<Self> {
        // The inline collision banks are large. Initialize their final heap
        // allocation rather than copying a World through native call stacks.
        let mut world = cold_fields!(cold_box);
        world.warm();
        world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gen::{Store, KINDS}, store::block::Block};

    #[test]
    fn cold_store_matches_every_derived_singleton_default() {
        // Const construction must preserve optional tags and collection order,
        // including records added to the schema after this owner was introduced.
        let mut cold = Store::cold();
        let mut default = Store::default();
        for kind in 1..=KINDS {
            let (mut a, mut b) = (Block::new(), Block::new());
            assert_eq!(cold.export(kind as u16, &mut a, 1), default.export(kind as u16, &mut b, 1));
            assert_eq!(a.words.iter().map(|n| n.to_bits()).collect::<Vec<_>>(),
                b.words.iter().map(|n| n.to_bits()).collect::<Vec<_>>(), "kind {kind}");
        }
    }

    #[test]
    fn warming_retains_banks_and_worlds_keep_independent_rows() {
        let mut a = World::new();
        let mut b = World::new();
        let blocks = a.sim.blocks.as_ptr();
        let wire = a.wire.as_deref().unwrap() as *const _;
        a.sim.store.battle.stats.time = 23.0;
        a.warm();
        assert_eq!(a.sim.blocks.as_ptr(), blocks);
        assert_eq!(a.wire.as_deref().unwrap() as *const _, wire);
        assert_eq!(a.sim.events.len(), 0);
        assert_eq!(a.sim.blocks.len(), KINDS + 1);
        assert_eq!(a.sim.store.battle.stats.time, 23.0);
        assert_eq!(b.sim.store.battle.stats.time, 0.0);
        b.sim.store.battle.stats.time = 9.0;
        assert_eq!(a.sim.store.battle.stats.time, 23.0);
    }
}
