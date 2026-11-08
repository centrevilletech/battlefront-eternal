//! Read-only observations for a host that reproduces the page's bookkeeping
//! beside the native world (the world-hash matrix worker): facts the page
//! read from its own objects and the native world keeps privately.
use super::super::context::{Context, Error};
use crate::store::DefId;

/// One charge level of a weapon class as the page's `WeaponFacade.levels`
/// saw it: whether it dispenses an item (`def.item`) and its class name.
#[derive(Clone, Debug, Default)]
pub struct WeaponLevelFacts {
    pub item: bool,
    pub class: Option<Vec<u16>>,
    /// `def.channel` (0 when the definition names none).
    pub channel: f64,
}

/// A registered building target (a turret or an objective) as the page's
/// `TurretTarget` read it: its registry id, whether it stands, its team,
/// its position and its life (the building's target generation).
#[derive(Clone, Debug, Default)]
pub struct PropTargetFacts {
    pub id: f64,
    pub alive: bool,
    pub team: f64,
    pub position: [f64; 3],
    pub life: f64,
}

impl Context {
    /// The F3 random entries the bare AI generators were bound to
    /// (`bind_ai_stream_in`), in binding order: the Classic generator, then
    /// the Enhanced one. The page's `randomState()` reads their seeds.
    pub fn ai_random_bindings(&self) -> Result<Vec<u32>, Error> {
        self.world(|w| crate::units::f3::shared_bindings_in(&w.units).to_vec())
    }

    /// A weapon class's levels (`WeaponClass.levels`), for the dispenser
    /// bindings the page made per unit life (`F3Items.flush`).
    pub fn weapon_levels(&self, weapon: u32) -> Result<Vec<WeaponLevelFacts>, Error> {
        self.world(|w| {
            let defs = crate::combat::module::defs_in(&mut w.combat);
            if !defs.weapons.has(DefId(weapon)) {
                return Vec::new();
            }
            defs.weapons
                .class(DefId(weapon))
                .levels
                .iter()
                .map(|level| WeaponLevelFacts {
                    item: level.obj.get("item").truthy(),
                    class: level.obj.get("class").str().map(|s| s.encode_utf16().collect()),
                    channel: level.obj.get("channel").or(0.0),
                })
                .collect()
        })
    }

    /// Every registered building target, by registry id.
    pub fn prop_targets(&self) -> Result<Vec<PropTargetFacts>, Error> {
        self.world(|w| {
            crate::units::f3_props::target_facts_in(w)
                .into_iter()
                .map(|(id, alive, team, position, life)| PropTargetFacts { id, alive, team, position, life })
                .collect()
        })
    }

    /// Every building's three bodies (built, destroyed, unbuilt) as kernel
    /// slots, by building index.
    pub fn prop_body_kernels(&self) -> Result<Vec<(usize, [Option<u32>; 3])>, Error> {
        self.world(|w| crate::units::f3_props::prop_body_kernels_in(w))
    }

    /// Each posed hull's seats by hull slot: yaw rate, target, and per
    /// weapon its barrel and aimers (yaw, pitch, on target, in limits,
    /// facing), as the page's vehicle objects mirrored them.
    pub fn fleet_seat_poses(&self) -> Result<Vec<(u32, Vec<crate::vehicles::module::SeatPoseFacts>)>, Error> {
        self.world(|w| crate::vehicles::module::seat_pose_facts_in(&w.fleet))
    }

    /// The combat registry's id for an ordnance definition
    /// (`registry.combat.definitionId(def)`), from the store source.
    pub fn wire_ordnance_id(&self, source: u32, def: u32) -> Result<f64, Error> {
        self.world(|w| {
            w.wire
                .as_deref()
                .and_then(|owner| owner.sources.get(source as usize))
                .map_or(0.0, |s| s.ordnance_id(DefId(def)))
        })
    }

    /// The wire registry's id for a weapon name (`registry.weaponIds.get`),
    /// 0 for an unknown one, from the store source's protocol maps.
    pub fn wire_weapon_id(&self, source: u32, name: crate::store::StrId) -> Result<f64, Error> {
        self.world(|w| {
            w.wire
                .as_deref()
                .and_then(|owner| owner.sources.get(source as usize))
                .map_or(0.0, |s| s.capture.weapon_name_id(name))
        })
    }
}
