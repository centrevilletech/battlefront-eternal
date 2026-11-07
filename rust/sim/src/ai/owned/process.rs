//! The source keeps weapon registration and aircraft attack-run distances
//! across battles in one process. Battle-local controllers and queries reset.
use crate::ai::{
    combat::weapons_ai::{Shared, WeaponAiTable},
    vehicles::{patterns::AttackRun, VehicleAI},
};
#[derive(Default)]
pub struct Process {
    retained: Option<Retained>,
    active: bool,
}
#[derive(Default)]
struct Retained {
    weapons: WeaponAiTable,
    run: AttackRun,
}
impl Process {
    /// Move the retained authorities into the new battle. There is only one
    /// active battle per simulation instance; previous service scopes must end.
    pub fn begin(&mut self) -> (Shared, VehicleAI) {
        assert!(!self.active, "native AI battle is already active");
        let mut retained = self.retained.take().unwrap_or_default();
        retained.weapons.resume();
        self.active = true;
        let shared = Shared {
            weapons: std::rc::Rc::new(retained.weapons),
            ..Shared::default()
        };
        let mut vehicles = VehicleAI::new();
        vehicles.run = retained.run;
        (shared, vehicles)
    }
    /// Call after all native world/service views have been dropped. The
    /// registration table moves back without copying its definitions or names.
    pub fn end(&mut self, shared: Shared, vehicles: VehicleAI) {
        assert!(self.active, "native AI battle is not active");
        let weapons = std::rc::Rc::try_unwrap(shared.weapons)
            .expect("native AI service views must end before the battle");
        self.retained = Some(Retained {
            weapons,
            run: vehicles.run,
        });
        self.active = false;
    }
}
