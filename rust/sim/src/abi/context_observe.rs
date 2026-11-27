//! Read-only observations for a host that reproduces the client's bookkeeping
//! beside the native world (the world-hash matrix worker): facts the client
//! read from its own objects and the native world keeps privately.
use super::super::context::{Context, Error};
use crate::store::DefId;

/// One charge level of a weapon class as the client's `WeaponFacade.levels`
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
    /// A guided rocket class's critical explosion (`def.explosionCritical`,
    /// taken above the boost speed threshold), by class index.
    ReconCritical(u32),
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

/// A registered building target (a turret or an objective) as the client's
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
    /// Living units by team, sampled from the authoritative unit store.
    pub fn world_living_units(&self) -> Result<[usize; 3], Error> {
        self.world(|w| {
            let mut counts=[0;3];
            for (_,unit) in w.sim.store.unit.iter() {
                if unit.alive && !unit.removed {counts[(unit.team as usize).min(2)]+=1;}
            }
            counts
        })
    }

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
    /// the Enhanced one. The client's `randomState()` reads their seeds.
    pub fn ai_random_bindings(&self) -> Result<Vec<u32>, Error> {
        self.world(|w| crate::units::f3::shared_bindings_in(&w.units).to_vec())
    }

    /// A weapon class's levels (`WeaponClass.levels`), for the dispenser
    /// bindings the client made per unit life (`F3Items.flush`).
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

    /// Every collision body's slot, moving flag, and world-space AABB
    /// `[min_x, min_y, min_z, max_x, max_y, max_z]`.
    pub fn collision_body_bounds(&self) -> Result<Vec<(u32, bool, [f64; 6])>, Error> {
        self.world(|w| {
            w.collision
                .state
                .bodies
                .iter()
                .enumerate()
                .map(|(slot, body)| (slot as u32, body.moving, body.bounds))
                .collect()
        })
    }

    /// Snapshot of every unit slot in the authoritative unit store:
    /// `(slot, team, alive, [x, y, z], health, class_name)`.
    pub fn world_unit_states(&self) -> Result<Vec<(u32, f64, bool, [f64; 3], f64, String)>, Error> {
        self.world(|w| {
            w.sim
                .store
                .unit
                .iter()
                .map(|(r, unit)| {
                    let class = String::from_utf16_lossy(w.sim.strings.units(unit.class_name));
                    let p = unit.position();
                    (
                        r.slot,
                        unit.team,
                        unit.alive && !unit.removed && !unit.dead,
                        [p.x, p.y, p.z],
                        unit.combat.health,
                        class,
                    )
                })
                .collect()
        })
    }

    /// Each posed hull's seats by hull slot: yaw rate, target, and per
    /// weapon its barrel and aimers (yaw, pitch, on target, in limits,
    /// facing), as the client's vehicle objects mirrored them.
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
    /// class's by its index (a guided rocket's critical explosion too), a
    /// hull's explosion and final explosion.
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
                    if let Some(explosion) = droid.guided.and_then(|guided| guided.explosion_critical) {
                        out.push(OwnedExplosion {
                            owner: ExplosionOwner::ReconCritical(class as u32),
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

    /// Snapshot of every admitted explosion definition `(damage, radius_inner, radius_outer)`.
    pub fn world_explosion_defs(&self) -> Result<Vec<(f64, f64, f64)>, Error> {
        self.world(|w| crate::ordnance::module::explosion_defs_in(&w.ordnance))
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

    // ----- 2005 navigation controls and diagnostics -----

    /// Deterministic, zero-allocation reachability and walkable distance
    /// between two world positions.
    pub fn nav_route_distance(
        &self,
        from: [f64; 3],
        to: [f64; 3],
        radius: f64,
        mask: u32,
        can_jet: bool,
    ) -> Result<crate::ai::nav::route::RouteQuery, Error> {
        self.world(|w| w.nav_route_distance(from, to, radius, mask, can_jet))
    }

    /// Whether a point lies outside the loaded navigation boundary polygons.
    pub fn nav_outside(&self, point: [f64; 3]) -> Result<bool, Error> {
        self.world(|w| {
            w.ai.battle().is_some_and(|b| {
                b.state
                    .navigation
                    .nav
                    .outside(crate::ai::nav::math::Vec3::new(point[0], point[1], point[2]))
            })
        })
    }

    pub fn block_planning_graph_arcs(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.block_planning_graph_arcs(name))
    }

    pub fn unblock_planning_graph_arcs(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.unblock_planning_graph_arcs(name))
    }

    pub fn toggle_planning_graph_arcs(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.toggle_planning_graph_arcs(name))
    }

    pub fn enable_barriers(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.enable_barriers(name))
    }

    pub fn disable_barriers(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.disable_barriers(name))
    }

    pub fn toggle_barriers(&self, name: &str) -> Result<bool, Error> {
        self.world(|w| w.toggle_barriers(name))
    }

    /// Inspect the loaded 2005 planning graph, dynamic groups, and validation
    /// summary on an admitted world.
    pub fn swbf2_nav_info(&self) -> Result<Option<Swbf2NavInfo>, Error> {
        self.world(|w| {
            let battle = w.ai.battle()?;
            let nav = &battle.state.navigation.nav;
            let swbf2 = nav.swbf2.as_ref()?;
            let graph = swbf2.graph.as_ref()?;
            Some(Swbf2NavInfo {
                hubs: graph
                    .hubs
                    .iter()
                    .map(|h| Swbf2HubInfo {
                        name: h.name.clone().unwrap_or_default(),
                        position: [h.x, h.y, h.z],
                        radius: h.radius,
                        mask: h.mask,
                        valid: h.valid,
                    })
                    .collect(),
                arcs: graph
                    .arcs
                    .iter()
                    .map(|a| Swbf2ArcInfo {
                        name: a.name.clone().unwrap_or_default(),
                        start: a.start,
                        end: a.end,
                        filter_flags: a.filter_flags,
                        type_flags: a.type_flags,
                        blocked_by_group: a.blocked_by_group,
                        blocked_manual: a.blocked_manual,
                        degenerate: a.degenerate,
                    })
                    .collect(),
                group_arc_counts: graph.groups.iter().map(|g| g.arc_indices.len()).collect(),
                group_state: graph.group_state,
                delta_block_count: graph.delta_block_count(),
                dangling_arcs: graph.validation.dangling_arcs.len(),
                bad_hub_slots: graph.validation.bad_hub_slots.len(),
                non_finite_hubs: graph.validation.non_finite_hubs.len(),
                zero_length_arcs: graph.validation.zero_length_arcs.len(),
                weight_mismatches: graph.validation.weight_mismatches.len(),
                bad_delta_blocks: graph.validation.bad_delta_blocks.len(),
                soldier_components: graph.validation.components_by_table[0]
                    .iter()
                    .map(Vec::len)
                    .collect(),
                barriers: nav.barriers.list.len(),
                hints: nav.hints.list.len(),
                boundary_polygons: swbf2.boundary_polygon_count,
                words_hash: {
                    let mut h = bfe_wide::Fnv::new();
                    for &wrd in &graph.words {
                        h.bytes(&wrd.to_le_bytes());
                    }
                    h.finish()
                },
            })
        })
    }

    /// Drive a real soldier mover through the admitted world's static + prop +
    /// terrain collision along a 2005 navigation route from `from` to `to`.
    pub fn swbf2_walk_route(
        &self,
        from: [f64; 3],
        to: [f64; 3],
        max_ticks: u32,
        seed: u32,
    ) -> Result<Swbf2WalkReport, Error> {
        use crate::ai::nav::{
            goto::{GotoMode, GotoWorld, Resolved},
            math::Vec3,
            swbf2::{Swbf2GotoWorld, Swbf2GroundGoto},
            units::{kind, Air, ClassDef, NavUnit, Posture, SizeType, SoldierView, UnitRef},
        };
        use crate::ai::random::{ClassicRandom, Random};
        use crate::movement::soldier::{Air as MoveAir, Input as MoveInput, Posture as MovePosture, V3};

        struct WalkWorld<'a> {
            w: &'a mut crate::world::owner::World,
            time: f64,
            ra: ClassicRandom,
            rb: ClassicRandom,
        }
        impl GotoWorld for WalkWorld<'_> {
            fn time(&self) -> f64 {
                self.time
            }
            fn random_a(&mut self) -> &mut dyn Random {
                &mut self.ra
            }
            fn random_b(&mut self) -> &mut dyn Random {
                &mut self.rb
            }
            fn resolve(&mut self, _r: UnitRef) -> Option<Resolved> {
                None
            }
            fn leader(&mut self, _r: UnitRef) -> Option<NavUnit> {
                None
            }
            fn remembered(&mut self, _target: u64) -> Option<Vec3> {
                None
            }
            fn clear_above(&mut self, _from: Vec3, _unit: &NavUnit) -> bool {
                true
            }
            fn follow_far(&mut self) -> f64 {
                15.0
            }
        }
        impl Swbf2GotoWorld for WalkWorld<'_> {
            fn foot(&mut self, point: Vec3) -> Option<Vec3> {
                crate::ai::owned::live::supported_floor(self.w, point, 0.32, 1.25)
            }
            fn step_floor(&mut self, point: Vec3) -> Option<Vec3> {
                crate::ai::owned::live::supported_floor(self.w, point, 0.65, 0.75)
            }
            fn crosses(&mut self, from: Vec3, to: Vec3, _unit: &NavUnit, flags: u8) -> bool {
                crate::ai::owned::live::crosses(self.w, from, to, flags)
            }
            fn try_crosses(
                &mut self,
                from: Vec3,
                to: Vec3,
                _unit: &NavUnit,
                flags: u8,
                kind: crate::ai::nav::swbf2::ProbeKind,
            ) -> Option<bool> {
                crate::ai::owned::live::try_crosses(self.w, from, to, flags, kind)
            }
            fn observe_navigation(&mut self, nav: &mut crate::ai::nav::route::MapNavigation) -> bool {
                crate::ai::owned::live::observe_navigation(self.w, nav)
            }
            fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld {
                self
            }
            fn line_of_walk(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit) -> bool {
                crate::ai::owned::live::crosses(self.w, a, b, 0)
            }
        }

        self.world(|w| {
            let world_id = crate::units::f3::collision_world_in(&w.units);
            let ground_snap = |w: &mut crate::world::owner::World, p: [f64; 3]| {
                let origin = three::Vector3::new(p[0], p[1] + 0.25, p[2]);
                let down = three::Vector3::new(0., -1., 0.);
                crate::units::native::cast_static_in(w, world_id, &origin, &down, 2., true)
                    .filter(|hit| hit.normal.y > 0.55)
                    .map(|hit| [p[0], hit.point.y, p[2]])
            };
            let (Some(start_p), Some(dest_p)) = (ground_snap(w, from), ground_snap(w, to)) else {
                return Swbf2WalkReport { failed: true, ..Default::default() };
            };
            let Some(battle) = w.ai.battle() else { return Swbf2WalkReport::default() };
            let loaded = &battle.state.navigation.nav;
            let mut nav = crate::ai::nav::route::MapNavigation::from(crate::ai::nav::Nav::empty(Default::default()));
            nav.swbf2 = loaded.swbf2.clone();
            if let Some(state) = &mut nav.swbf2 {
                state.prepare_isolated_walk();
            }
            // Cloning a Vec discards spare capacity. Rebuild the isolated grid so
            // its reusable candidate buffer can hold every admitted barrier.
            nav.barriers = crate::ai::nav::barriers::BarrierSet::with_cell(
                loaded.barriers.list.clone(), loaded.barriers.cell,
            );
            let mut movement = crate::movement::MovementOwner::new();
            movement.flags_mut().copy_from_slice(w.movement.flags_mut());
            let mover_id = movement.new_mover(world_id, |m| {
                m.max_speed = 6.5;
                m.max_strafe = 4.5;
                m.max_jump = 7.5;
                m.max_turn = 6.0;
                m.max_pitch = 4.0;
                m.accel = 70.0;
                m.pitch_limits = [-1.2, 1.2];
                m.position = V3::from(start_p);
                m.settled = V3::from(start_p);
                m.grounded = true;
            });
            movement.reserve_mover(mover_id, 256, 16, 512);
            movement.prepare_world_gathers(&w.collision.state, world_id);

            let control = nav.new_control();
            let mut helper = Swbf2GroundGoto::new(control);
            let mut unit = NavUnit::new(0xffff_0001, kind::SOLDIER, Vec3::new(start_p[0], start_p[1], start_p[2]));
            unit.generation = Some(1.0);
            unit.nav_radius = Some(0.5);
            unit.class_def = Some(ClassDef {
                ai_size_type: SizeType::Number(1.0),
                max_speed: Some(6.5),
                max_strafe_speed: Some(4.5),
                max_turn_speed: Some(6.0),
                max_pitch_speed: Some(4.0),
                ..ClassDef::default()
            });
            unit.soldier = Some(SoldierView {
                aim_yaw: Some(0.0),
                aim_pitch: Some(0.0),
                max_speed: Some(6.5),
                max_strafe: Some(4.5),
                max_turn: Some(6.0),
                max_pitch: Some(4.0),
                avoidance: Some(Vec3::default()),
                ..SoldierView::default()
            });

            let dest_v = Vec3::new(dest_p[0], dest_p[1], dest_p[2]);
            w.ai.begin_probe_tick(0);
            let mut ww = WalkWorld {
                w,
                time: 0.0,
                ra: ClassicRandom::new(f64::from(seed)),
                rb: ClassicRandom::new(f64::from(seed ^ 0x9e37_79b9)),
            };
            nav
                .swbf2_ground_goto(&mut helper, &mut unit, &mut ww, dest_v, GotoMode::Route);
            nav.service(1.0);

            let dt = 1.0 / 60.0;
            let mut samples = vec![[start_p[0], start_p[2]]];
            let mut distance_m = 0.0f64;
            let mut stuck_ticks = 0u32;
            let mut max_recoveries = 0u8;
            let mut jumps = 0u32;
            let mut arrived = false;
            let mut ticks_run = 0u32;
            let mut sec_anchor = [start_p[0], start_p[2]];

            for tick in 1..=max_ticks {
                if let Some(state) = &mut nav.swbf2 {
                    state.begin_tick(tick);
                }
                ww.w.ai.begin_probe_tick(tick);
                ticks_run = tick;
                ww.time = f64::from(tick) * dt;
                // Sync mover state into NavUnit before nav tick + input.
                {
                    let m = movement.mover(mover_id);
                    unit.position = Vec3::new(m.position.x, m.position.y, m.position.z);
                    unit.velocity = Vec3::new(m.velocity.x, m.velocity.y, m.velocity.z);
                    let yaw = m.aim_yaw;
                    unit.origin = Some(unit.position);
                    unit.forward = Some(Vec3::new(v8math::sin(yaw), 0.0, v8math::cos(yaw)));
                    if let Some(s) = unit.soldier.as_mut() {
                        s.aim_yaw = Some(m.aim_yaw);
                        s.aim_pitch = Some(m.aim_pitch);
                        s.avoidance = Some(Vec3::new(m.avoidance.x, m.avoidance.y, m.avoidance.z));
                        s.posture = match m.posture {
                            MovePosture::Stand => Posture::Stand,
                            MovePosture::Crouch => Posture::Crouch,
                            MovePosture::Prone => Posture::Prone,
                        };
                        s.air = match m.air {
                            MoveAir::None => Air::None,
                            MoveAir::Jump => Air::Jump,
                            MoveAir::Jet => Air::Jet,
                            _ => Air::Other,
                        };
                        s.lock = m.lock;
                    }
                }

                nav.swbf2_ground_tick(&mut helper, &mut unit, &mut ww, dt);
                nav.service(1.0);
                let intent = nav.swbf2_navigation_input(control, &mut unit, dt, None, true, &mut ww);
                if intent.jump {
                    jumps += 1;
                }
                if let Some(c) = helper.cursor.as_ref() {
                    max_recoveries = max_recoveries.max(c.recovery_attempts);
                }

                // Write consumed avoidance back and step the real soldier mover.
                let consumed_avoid = unit.soldier.and_then(|s| s.avoidance).unwrap_or_default();
                {
                    let m = movement.mover(mover_id);
                    m.avoidance = V3::new(consumed_avoid.x, consumed_avoid.y, consumed_avoid.z);
                }
                let move_in = MoveInput {
                    forward: intent.forward,
                    right: intent.right,
                    turn: intent.turn,
                    pitch: intent.pitch,
                    jump: intent.jump,
                    crouch: intent.crouch,
                    prone: intent.prone,
                    jet_direction: intent.jet_direction.flatten().map(|v| V3::new(v.x, v.y, v.z)),
                };
                let prev = {
                    let m = movement.mover(mover_id);
                    [m.position.x, m.position.z]
                };
                movement.advance(&mut ww.w.collision.state, mover_id, &move_in, dt);
                let cur = {
                    let m = movement.mover(mover_id);
                    [m.position.x, m.position.z]
                };
                distance_m += v8math::hypot(cur[0] - prev[0], cur[1] - prev[1]);
                if tick % 15 == 0 {
                    samples.push(cur);
                }
                if tick % 60 == 0 {
                    if v8math::hypot(cur[0] - sec_anchor[0], cur[1] - sec_anchor[1]) < 0.75 {
                        stuck_ticks += 60;
                    }
                    sec_anchor = cur;
                }
                let rem = v8math::hypot(cur[0] - dest_p[0], cur[1] - dest_p[2]);
                let mover = movement.mover(mover_id);
                if nav.swbf2_ground_arrived(&helper) {
                    arrived = mover.grounded && rem <= 1. && (mover.position.y - dest_p[1]).abs() < 0.55;
                    samples.push(cur);
                    break;
                }
                if helper.failed {
                    samples.push(cur);
                    break;
                }
            }

            let final_pos = {
                let m = movement.mover(mover_id);
                [m.position.x, m.position.y, m.position.z]
            };
            let final_dist_xz = v8math::hypot(final_pos[0] - dest_p[0], final_pos[2] - dest_p[2]);
            let failed = helper.failed;
            nav.control_dispose(control);


            Swbf2WalkReport {
                arrived,
                failed,
                ticks: ticks_run,
                elapsed_s: f64::from(ticks_run) * dt,
                distance_m,
                stuck_s: f64::from(stuck_ticks) * dt,
                recoveries: max_recoveries,
                jumps,
                final_pos,
                final_dist_xz,
                samples,
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct Swbf2HubInfo {
    pub name: String,
    pub position: [f64; 3],
    pub radius: f64,
    pub mask: u32,
    pub valid: bool,
}

#[derive(Clone, Debug)]
pub struct Swbf2ArcInfo {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub filter_flags: u32,
    pub type_flags: u8,
    pub blocked_by_group: bool,
    pub blocked_manual: bool,
    pub degenerate: bool,
}

#[derive(Clone, Debug)]
pub struct Swbf2NavInfo {
    pub hubs: Vec<Swbf2HubInfo>,
    pub arcs: Vec<Swbf2ArcInfo>,
    pub group_arc_counts: Vec<usize>,
    pub group_state: u32,
    pub delta_block_count: usize,
    pub dangling_arcs: usize,
    pub bad_hub_slots: usize,
    pub non_finite_hubs: usize,
    pub zero_length_arcs: usize,
    pub weight_mismatches: usize,
    pub bad_delta_blocks: usize,
    pub soldier_components: Vec<usize>,
    pub barriers: usize,
    pub hints: usize,
    pub boundary_polygons: usize,
    pub words_hash: bfe_wide::Wide,
}

#[derive(Clone, Debug, Default)]
pub struct Swbf2WalkReport {
    pub arrived: bool,
    pub failed: bool,
    pub ticks: u32,
    pub elapsed_s: f64,
    pub distance_m: f64,
    pub stuck_s: f64,
    pub recoveries: u8,
    pub jumps: u32,
    pub final_pos: [f64; 3],
    pub final_dist_xz: f64,
    pub samples: Vec<[f64; 2]>,
}
