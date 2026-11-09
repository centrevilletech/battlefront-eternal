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
    /// Whether the definition names a channel at all (a seat's weapon
    /// without one takes its index in the seat, src/seats.js seatArsenal).
    pub named_channel: bool,
}

/// Who owns an explosion, by the name the content knows it by.
#[derive(Clone, Copy, Debug)]
pub enum ExplosionOwner {
    /// A unit class's death explosion (`def.explosion`).
    Class(crate::store::StrId),
    /// A hull class's explosion (`def.explosion`) or, `last`, its final one
    /// (`def.finalExplosion || def.explosion`).
    Hull {
        class: crate::store::StrId,
        last: bool,
    },
    /// A building's explosion, by its index in LiveObjects.buildings.
    Building(u32),
    /// A recon droid class's explosion (`def.explosion`), by class index.
    Recon(u32),
}

/// An owned explosion and its index.
#[derive(Clone, Copy, Debug)]
pub struct OwnedExplosion {
    pub owner: ExplosionOwner,
    pub explosion: u32,
}

/// An explosion an ordnance or item definition names: the definition's
/// combat registry id, the field naming the explosion, its index.
#[derive(Clone, Copy, Debug)]
pub struct ExplosionField {
    pub definition: f64,
    pub field: &'static str,
    pub explosion: u32,
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
    /// The authored turret's current registry life for a seat request.
    pub fn turret_life(&self, index: usize) -> Result<Option<f64>, Error> {
        // matrix-b
        self.world(|w| {
            crate::vehicles::module::f2::live_seats::wire_turret_in(
                &w.fleet,
                &w.units,
                &w.battle,
                index,
                &w.sim.store,
                [crate::store::StrId(0); 3],
            )
            .map(|t| t.life)
        }) // matrix-b
    }

    /// The wire registry's combat definition id for a recon class.
    pub fn wire_remote_id(&self, source: u32, class: u32) -> Result<f64, Error> {
        // matrix-b
        self.world(|w| {
            w.wire
                .as_deref()
                .and_then(|owner| owner.sources.get(source as usize))
                .and_then(|s| s.capture.remote_id(class))
                .unwrap_or(0.0)
        }) // matrix-b
    }

    /// The stationary gun's canonical weapon class, sampled for its shot notice.
    pub fn turret_weapon_definition(
        &self,
        turret: crate::store::Ref<crate::gen::Turret>,
        life: f64,
    ) -> Result<Option<u32>, Error> {
        // matrix-b
        self.world(|w| {
            crate::vehicles::module::f2::live_seats::with_weapon_ref(
                &w.fleet,
                turret,
                life,
                0,
                |weapon| weapon.def.0,
            )
        }) // matrix-b
    }

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
                    class: level
                        .obj
                        .get("class")
                        .str()
                        .map(|s| s.encode_utf16().collect()),
                    channel: level.obj.get("channel").or(0.0),
                    named_channel: !level.obj.get("channel").is_nullish(),
                })
                .collect()
        })
    }

    /// Every registered building target, by registry id.
    pub fn prop_targets(&self) -> Result<Vec<PropTargetFacts>, Error> {
        self.world(|w| {
            crate::units::f3_props::target_facts_in(w)
                .into_iter()
                .map(|(id, alive, team, position, life)| PropTargetFacts {
                    id,
                    alive,
                    team,
                    position,
                    life,
                })
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
    pub fn fleet_seat_poses(
        &self,
    ) -> Result<Vec<(u32, Vec<crate::vehicles::module::SeatPoseFacts>)>, Error> {
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

    /// The human spawn's class roster for a team (`world.roster(team)`, the
    /// prepared roster the spawn screen and the wire's summaries read),
    /// empty before the battle's tail is prepared.
    pub fn human_roster(&self, team: usize) -> Result<Vec<crate::store::StrId>, Error> {
        self.world(|w| crate::battle::f4_tail::human_roster_in(&w.battle, team).to_vec())
    }

    /// How many ordnance definitions the battle registered (the `ordnance`
    /// table's length).
    pub fn ordnance_definitions(&self) -> Result<usize, Error> {
        self.world(|w| crate::ordnance::module::ordnance_count_in(&w.ordnance))
    }

    /// The explosions unit classes, buildings, recon droids and hulls own,
    /// by what the content knows them by: a class's death explosion (named
    /// for any unit of the class), a building's by its index, a droid
    /// class's by its index, a hull's explosion and final explosion.
    pub fn explosion_owners(&self) -> Result<Vec<OwnedExplosion>, Error> {
        self.world(|w| {
            let mut out = Vec::new();
            let classes: Vec<_> = crate::units::f3::items_in(&mut w.units)
                .class_explosions()
                .collect();
            for (class, explosion) in classes {
                let name = w
                    .sim
                    .store
                    .unit
                    .iter()
                    .find(|(_, unit)| unit.class_def == class)
                    .map(|(_, unit)| unit.class_name);
                if let Some(name) = name {
                    out.push(OwnedExplosion {
                        owner: ExplosionOwner::Class(name),
                        explosion: explosion.0,
                    });
                }
            }
            crate::units::f3_props::building_explosions_in(&w.units, |building, explosion| {
                out.push(OwnedExplosion {
                    owner: ExplosionOwner::Building(building),
                    explosion,
                });
            });
            // RemoteDroids' classes live in the unit phase's lent services.
            crate::units::f3::with_services_in(w, |_, services| {
                let Some(services) = services else { return };
                for (class, droid) in services.recon.defs.droids.iter().enumerate() {
                    if let Some(explosion) = droid.explosion {
                        out.push(OwnedExplosion {
                            owner: ExplosionOwner::Recon(class as u32),
                            explosion,
                        });
                    }
                }
            });
            let store = &w.sim.store;
            crate::units::f3_hulls::hull_explosions_in(&w.units, |hull, explosion, last| {
                let Some(vehicle) = store.vehicle.get(hull) else {
                    return;
                };
                if let Some(explosion) = explosion {
                    out.push(OwnedExplosion {
                        owner: ExplosionOwner::Hull {
                            class: vehicle.name,
                            last: false,
                        },
                        explosion,
                    });
                }
                if let Some(explosion) = last {
                    out.push(OwnedExplosion {
                        owner: ExplosionOwner::Hull {
                            class: vehicle.name,
                            last: true,
                        },
                        explosion,
                    });
                }
            });
            out
        })
    }

    /// The explosions the ordnance and item definitions name, each with
    /// its definition's combat registry id (`registry.combat.definitionId`,
    /// 0 when the registry lacks it), the field and the explosion's index:
    /// the registry names an explosion by the object its definition holds.
    pub fn explosion_fields(&self, source: u32) -> Result<Vec<ExplosionField>, Error> {
        self.world(|w| {
            let mut out = Vec::new();
            let source = w
                .wire
                .as_deref()
                .and_then(|owner| owner.sources.get(source as usize));
            crate::ordnance::module::explosion_fields_in(
                &w.ordnance,
                |item, def, field, explosion| {
                    let definition = source.map_or(0.0, |s| {
                        if item {
                            s.capture.item_id(def)
                        } else {
                            s.ordnance_id(DefId(def))
                        }
                    });
                    out.push(ExplosionField {
                        definition,
                        field,
                        explosion,
                    });
                },
            );
            out
        })
    }

    /// The combat registry's id for an item definition (the store source's
    /// item table), 0 for one the registry lacks.
    pub fn wire_item_id(&self, source: u32, def: u32) -> Result<f64, Error> {
        self.world(|w| {
            w.wire
                .as_deref()
                .and_then(|owner| owner.sources.get(source as usize))
                .map_or(0.0, |s| s.capture.item_id(def))
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
