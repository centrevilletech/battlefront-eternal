//! UnitFactory's cold construction and the native host's remaining tick
//! inputs (src/units.js, f3-sim.js preparePhase and PlayersBridge.actions).
//! The store and F3's factory order are the source of identity here: stock
//! waiting for a birth and the dormant viewpoint never contribute a row.
use crate::gen::{PlayerActions, PlayerAdmission};
use crate::world::owner::World;

impl World {
    /// PlayersBridge.actions' typed native entry. The lane retains the same
    /// epoch, life, duplicate and capacity checks as the module export.
    /// Discrete jump/crouch/use presses belong here when separateActions is
    /// set; the input mailbox intentionally strips its action list then.
    pub fn player_actions(&mut self, handle: u32, input: &PlayerActions) -> Option<PlayerAdmission> {
        self.sim.calls.result(PlayerActions::WORDS, |row, heap| input.put(row, heap));
        let words = crate::players::module::players_actions_in(self, handle)?;
        let stride = words[crate::gen::BlockHeader::STRIDE] as usize;
        let start = words[crate::gen::BlockHeader::HEAP] as usize;
        let length = words[crate::gen::BlockHeader::HEAP_WORDS] as usize;
        let row = &words[crate::store::block::HEADER..crate::store::block::HEADER + stride];
        let heap = &words[start..start + length];
        Some(PlayerAdmission::take(row, heap))
    }
}

#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
impl crate::abi::context::Context {
    /// preparePhase's current foreign-row count, under the owning borrow.
    /// A host computes this immediately before stamping the world tick.
    pub fn world_units(&self) -> Result<u32, crate::abi::context::Error> {
        self.owned(|world| world.foreign_unit_rows())
    }

    /// Reliable discrete input for a connected local player. Hosts intern
    /// action names once at admission and reuse the typed row between ticks.
    pub fn world_player_actions(&self, handle: u32, input: &PlayerActions)
        -> Result<PlayerAdmission, crate::abi::context::Error>
    {
        self.owned(|world| world.player_actions(handle, input))?.ok_or_else(|| {
            crate::abi::context::Error::new(crate::abi::context::ErrorKind::InvalidKind, "invalid player actions")
        })
    }
}

/// The admission sequence and a native host share preparePhase's count.
pub fn units(world: &World) -> Result<u32, super::AdmitError> {
    Ok(world.foreign_unit_rows())
}

impl World {
    /// UnitFactory.build's canonical body installation. Class definitions,
    /// clips, weapon services and holder owners must be admitted first.
    /// The caller registers a warm body before this call, but leaves cold
    /// stock outside the mixed registry until the factory activates it.
    pub fn install_spawn_body(&mut self, body: super::bodies::Body, droideka_class: Option<u32>,
        riders: &[crate::gen::F2RiderBirth]) -> Result<crate::gen::F3Adoption, super::AdmitError>
    {
        use crate::gen::*;
        use crate::units::{f3, live_pose, module};
        let fail = |call: &str| super::AdmitError::Refused { stage: super::Stage::Player, call: call.into() };
        if !f3::active_in(&self.units) { return Err(fail("f3_init")); }
        let key = &body.adoption.key;
        if self.sim.store.unit.reference(key.slot).is_some() || body.unit.generation != key.life
            || body.unit.walker != droideka_class.is_some() {
            return Err(fail("spawn body identity"));
        }
        if riders.iter().any(|r| r.unit != key.reference() || r.life != key.life) {
            return Err(fail("f2_rider_birth"));
        }
        let reference = self.sim.store.unit.insert_at(key.slot, key.generation, body.unit);
        let c = body.combat;
        crate::combat::module::cmb_new_in(self, key.slot, c.class.0, 0, c.max_health, c.chunks, c.scale.0)
            .ok_or_else(|| fail("cmb_new"))?;
        // CombatFacade.deferMuzzles was set before attach; no live setter
        // is needed while this newly inserted row still belongs to construction.
        self.sim.store.unit.get_mut(reference).ok_or_else(|| fail("cmb_new"))?.combat.defer_muzzles = droideka_class.is_some();
        self.sim.calls.result(F3BodySkeleton::WORDS, |row, heap| body.skeleton.put(row, heap));
        if live_pose::f3_body_skeleton_in(self) == 0 { return Err(fail("f3_body_skeleton")); }
        if let Some(class) = droideka_class {
            module::units_droideka_attach_in(self, key.slot, class);
            let muzzles = body.muzzles.ok_or_else(|| fail("units_muzzle_attach"))?;
            self.sim.calls.result(muzzles.words.len(), |row, _| row.copy_from_slice(&muzzles.words));
            module::units_muzzle_attach_in(self, key.slot, class);
        }
        body.adoption.put(f3::command_row_in(&mut self.units), &mut Vec::new());
        if f3::f3_adopt_in(self) == 0 { return Err(fail("f3_adopt")); }
        for rider in riders {
            self.sim.calls.result(F2RiderBirth::WORDS, |row, heap| rider.put(row, heap));
            crate::vehicles::module::f2::riders::f2_rider_birth_in(self);
        }
        Ok(body.adoption)
    }

    /// F3Factory.prepare's final transfer. The prepared body has no live
    /// registry entry; the next birth moves its resources without rebuilding.
    pub fn stage_spawn_body(&mut self, stage: &crate::gen::F3ColdStage) -> Result<(), super::AdmitError> {
        if stage.adoption.key.validate(&self.sim.store).is_err()
            || self.sim.store.units.by_id(stage.adoption.key.slot).is_some() {
            return Err(super::AdmitError::Refused { stage: super::Stage::Player, call: "f3_factory_stage".into() });
        }
        self.sim.calls.result(crate::gen::F3ColdStage::WORDS, |row, heap| stage.put(row, heap));
        crate::units::f3_construction::f3_factory_stage_in(self);
        Ok(())
    }
}

impl World {
    /// F3Factory.reserve precedes all stock preparation and body acquisition.
    pub fn reserve_spawn_bodies(&mut self, limit: u32) {
        crate::units::f3_construction::f3_factory_reserve_in(self, limit);
    }
    /// The already installed passive viewpoint retains its class resources
    /// under this cookie for future PlayerBodies class changes.
    pub fn resident_spawn_body(&mut self, stage: &crate::gen::F3ColdStage) -> bool {
        if stage.adoption.key.validate(&self.sim.store).is_err() { return false; }
        self.sim.calls.result(crate::gen::F3ColdStage::WORDS, |row, heap| stage.put(row, heap));
        crate::units::f3_construction::f3_factory_resident_in(self);
        true
    }
    /// F3Factory.bindPlayer after the Character and its streams are admitted.
    pub fn bind_spawn_player(&mut self, input: &crate::gen::F3PlayerBinding) -> bool {
        if !self.sim.store.character.contains(input.character) { return false; }
        self.sim.calls.result(crate::gen::F3PlayerBinding::WORDS, |row, heap| input.put(row, heap));
        crate::units::f3_construction::f3_factory_player_bind_in(self);
        true
    }
    /// HumanSpawn's external spawn, shared with F4's native realization.
    pub fn spawn_player_body(&mut self, input: &crate::gen::F3PlayerSpawn) -> Option<crate::store::Ref<crate::gen::Unit>> {
        self.sim.calls.result(crate::gen::F3PlayerSpawn::WORDS, |row, heap| input.put(row, heap));
        let slot = crate::units::f3_construction::f3_factory_player_spawn_in(self);
        if slot < 0 { return None; }
        self.sim.store.unit.reference(slot as u32)
    }
    /// UnitFactory.acquire uses its warm LIFO class pool before cold stock.
    pub fn acquire_spawn_body(&mut self, class: crate::store::StrId) -> Option<crate::store::Ref<crate::gen::Unit>> {
        let slot = crate::units::f3_construction::f3_factory_acquire_in(self, class.0);
        if slot < 0 { return None; }
        self.sim.store.unit.reference(slot as u32)
    }
    /// Old local class resources retire after the outer publication returns.
    pub fn finish_spawn_bodies(&mut self) {
        crate::units::f3_construction::f3_factory_finish_in(&mut self.units);
    }
    /// AnimatedProp's explicit held-body command uses the same qualified row
    /// and clip identities as F3's ordinary pose entry.
    pub fn spawn_body_pose(&mut self, input: &crate::gen::F3PoseCommand) -> bool {
        input.put(crate::units::f3::command_row_in(&mut self.units), &mut Vec::new());
        crate::units::f3::f3_pose_in(self) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gen::*, store::{Ref, StrId}, units::f3};

    #[test]
    fn staged_body_moves_buffers_into_factory_identity_and_reuses_its_pool() {
        use crate::world::admit::bodies::{Body, CombatInit};
        use crate::store::DefId;
        let mut w = World::new();
        let positions = w.collision.positions_new(0, 0);
        let tree = w.collision.tree_new(positions, 0, 0);
        let collision = w.collision.world_new(tree, -1, -1);
        f3::f3_init_in(&mut w, collision, f64::NAN, 0);
        w.sim.store.units.next_id = 1.;
        w.reserve_spawn_bodies(8);
        let class = w.sim.strings.intern_str("infantry");
        let health = w.sim.strings.intern_str("PersonScale");
        // An empty registered loadout is still a real CombatDefs class.
        // cmb_new owns all weapon/health defaults when the body is installed.
        w.sim.calls.result(2, |row, _| row.copy_from_slice(&[7., 0.]));
        crate::combat::module::cmb_class_in(&mut w, 1);
        let key = F3UnitKey { slot: 1, generation: 0, life: 0. };
        let adoption = F3Adoption { key: key.clone(), has_actor: true, ..Default::default() };
        let soldier = crate::units::class::soldier_body(&bfe_content::Value::Null, 200., 0., 0.);
        let input = Body { unit: Unit { class_name: class, class_def: DefId(1), removed: true,
            soldier: UnitBody::Soldier(soldier), root_scale: 1., shapes:vec![HitShape::default();2], spheres_tick:-1., ..Default::default() },
            adoption: adoption.clone(), skeleton: F3BodySkeleton { key: key.clone(), bones: vec![] },
            combat: CombatInit { class: DefId(1), max_health: 300., chunks: 0., scale: health }, muzzles: None };
        w.install_spawn_body(input, None, &[]).unwrap();
        let stage = F3ColdStage { cookie: 0, adoption };
        w.stage_spawn_body(&stage).unwrap();
        assert!(w.sim.store.unit.reference(key.slot).is_none());
        assert_eq!(w.sim.store.units.list.len(), 0);
        assert_eq!(w.foreign_unit_rows(), 0);
        let active = w.acquire_spawn_body(class).unwrap();
        assert_eq!((active.slot, active.gen), (1, 1));
        assert_eq!(w.sim.store.units.list, vec![Member::Unit(active)]);
        let body = w.sim.store.unit.get_mut(active).unwrap();
        assert_eq!((body.combat.health, body.combat.max_health), (300., 300.));
        body.removed = false; body.alive = true;
        assert_eq!(w.foreign_unit_rows(), 1);
        let body = w.sim.store.unit.get_mut(active).unwrap();
        body.removed = true; body.alive = false;
        f3::with_owner_rows(&mut w, |store, owner| {
            owner.removed(store, &F3UnitKey::of(active, store.unit.get(active).unwrap())).unwrap();
        });
        assert_eq!(w.acquire_spawn_body(class), Some(active));
        assert!(w.acquire_spawn_body(class).is_none());
        w.finish_spawn_bodies();
        live_player_actions(&mut w,active,class);
    }

    // Admit and spawn a real connection before entering the native host's
    // discrete lane. The mailbox remains separate from reliable presses.
    fn live_player_actions(w: &mut World, body: Ref<Unit>, class: StrId) {
        use crate::battle::{f4_core,f4_tail,f4_connections};
        let character=w.sim.store.character.insert(Character {human:true,team:1.,class_name:Some(class),..Default::default()});
        w.sim.store.battle.characters.push(character);
        let live=F4LiveSetup {unit_capacity:8,registry_capacity:8,hull_capacity:0,..Default::default()};
        w.sim.calls.result(F4LiveSetup::WORDS,|r,h|live.put(r,h));crate::battle::f4_live::f4_live_init_in(w);
        let setup=F4BattleSetup {capacity:8,..Default::default()};
        w.sim.calls.result(F4BattleSetup::WORDS,|r,h|setup.put(r,h));f4_core::f4_battle_init_in(w);
        let handle=F4Handle::take(f4_core::f4_character_handle_in(w,0),&[]);
        let post_name=w.sim.strings.intern_str("home");
        let post=w.sim.store.post.insert(Post {name:post_name,team:1.,active:true,
            ai:Some(PostAi {spawn_path:Some(SpawnPath {present:true,points:vec![[0.;4]]}),..Default::default()}),..Default::default()});
        w.sim.store.command_posts.posts.push(post);
        let setup=F4TailSetup {registry_capacity:8,victim_capacity:8,command_capacity:8,point_capacity:4,
            rosters:vec![F4RosterClasses::default(),F4RosterClasses {classes:vec![class]}],classes:vec![class],..Default::default()};
        w.sim.calls.result(F4TailSetup::WORDS,|r,h|setup.put(r,h));f4_tail::f4_tail_init_in(w);
        let binding=F3PlayerBinding {character,body:Some(body),combat_random:NO_RANDOM_STREAM,
            weapon_random:NO_RANDOM_STREAM,..Default::default()};
        assert!(w.bind_spawn_player(&binding));
        f4_connections::f4_connections_init_in(w,8);
        let mut connection=F4ConnectionCommand {target:handle.clone(),class_name:Some(class),post:Some(post_name),..Default::default()};
        w.sim.calls.result(F4ConnectionCommand::WORDS,|r,h|connection.put(r,h));f4_connections::f4_connection_command_in(w);
        connection.op=F4ConnectionOp::Spawn;
        w.sim.calls.result(F4ConnectionCommand::WORDS,|r,h|connection.put(r,h));f4_connections::f4_connection_command_in(w);
        let view=f4_connections::wire_view_in(&w.battle,&w.sim.store,&handle).unwrap();
        let body=view.unit.expect("native connection spawned");
        assert!(w.sim.store.unit.get(body).unwrap().alive);
        crate::players::module::players_init_in(w);
        let mailbox=crate::input::owner::Mailbox::new(1.,8.,vec![],&mut w.sim.strings).unwrap();
        let mailbox=w.input.mailboxes.insert(mailbox).unwrap() as u32;
        let setup=PlayerSetup {mailbox,character:Some(handle),separate_actions:true,..Default::default()};
        w.sim.calls.result(PlayerSetup::WORDS,|r,h|setup.put(r,h));
        let player=crate::players::module::players_create_in(w);
        let actions=["jump","crouch","use"].map(|name|w.sim.strings.intern_str(name));
        let generation=w.sim.store.unit.get(body).unwrap().generation;
        let mut input=PlayerActions {entry:IaEntry {id:1.,tick:1.,generation,actions:actions.to_vec(),..Default::default()},
            tick:1.,same_epoch:true,payload_valid:true,human_valid:true,..Default::default()};
        assert!(w.player_actions(player,&input).unwrap().accepted);
        assert!(!w.player_actions(player,&input).unwrap().accepted);
        input.entry.id=2.;input.entry.generation=generation+1.;
        assert!(!w.player_actions(player,&input).unwrap().accepted);
        let mut taken=Vec::new();let mut human=Vec::new();
        let lane=&mut w.input.players.pool.get_mut(player as usize).unwrap().actions;
        crate::input::actions::take_into(lane,1.,generation,&mut taken,&mut human,&mut w.sim.strings).unwrap();
        assert_eq!(taken,actions);assert!(lane.queue.is_empty());assert_eq!(lane.ack,1.);
        assert_eq!(w.foreign_unit_rows(),0);
    }

    #[test]
    fn context_prepares_services_and_stages_reusable_class_bodies() {
        use bfe_content::{battle::{Battle,Loader},content::Content,defs::Defs,files::MemFiles};
        use crate::world::admit::{Options,rows::{Cx,battle::Facts}};
        let mut files=MemFiles::default();
        for path in ["maps/test/map.json","common/common.json","common/effects.json","sides/rep/weapons.json"] {files.insert(path,b"{}".to_vec());}
        files.insert("common/anims.json",br#"{"fps":30,"clips":{"idle":{"frames":1,"tracks":[],"travel":[0,0,0]}}}"#.to_vec());
        files.insert("sides/rep/actors/person.json",br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":2}]}"#.to_vec());
        files.insert("sides/rep/units.json",br#"{"units":{"a":{"model":"person","base":"soldier","MaxHealth":125,"weapons":[{"class":"dispenser","base":"launcher","RoundsPerClip":3,"ammo":9,"ordnance":{"base":"grenade","Damage":5},"item":{"base":"mine","MaxHealth":30}}],"drops":[{"class":"mine","probability":0.5}]}},"items":{"mine":{"base":"mine"}}}"#.to_vec());
        for path in ["sides/rep/actors/person_index.bin","common/anims.bin","maps/test/collision.bin","maps/test/collision_index.bin","maps/test/collision_poly.bin","maps/test/terrain_collision.bin","maps/test/terrain_collision_index.bin","sides/rep/weapons.bin","sides/rep/weapons_index.bin","common/fxmodels.bin","common/fxmodels_index.bin"] {files.insert(path,Vec::new());}
        let vertices:[f32;18]=[-1.,0.,-2.,0.,0.,0.,0.,0.,0.,3.,2.,4.,0.,0.,0.,0.,0.,0.];
        files.insert("sides/rep/actors/person.bin",vertices.iter().flat_map(|v|v.to_le_bytes()).collect());
        let options=Options {map:"test".into(),side:"rep".into(),enhanced:false,seed:1,heroes:false,unit:Some("a".into())};
        let mut content=Content::build(Battle::load(&Loader::new(&files),&options.map,&options.query()).unwrap()).unwrap();
        // A second authored name can share one definition identity. Combat
        // services stay shared while UnitFactory keeps the named pools apart.
        let mut units=content.battle.sides[0].units.as_object().unwrap().clone();
        units.set("alias".into(),units.get("a").unwrap().clone());
        content.battle.sides[0].units=bfe_content::Value::Object(std::rc::Rc::new(units));
        let defs=Defs::build(&content).unwrap();let mut world=World::new();
        let positions=world.collision.positions_new(0,0);let tree=world.collision.tree_new(positions,0,0);
        let collision=world.collision.world_new(tree,-1,-1);f3::f3_init_in(&mut world,collision,f64::NAN,0);
        world.reserve_spawn_bodies(8);
        let mut cx=Cx::new(&content,&defs,&files,&options,world.sim.strings.clone());
        cx.ids.battle=Some(Facts {gameplay:0,..Default::default()});
        let first=prepare(&mut world,&mut cx,"a").unwrap();
        assert_eq!(first.stage.cookie,0);
        let names=first.calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>();
        let at=|name|names.iter().position(|candidate|*candidate==name).unwrap();
        assert!(at("f3_body_skeleton")<at("f3_weapon_services"));
        assert!(at("f3_weapon_services")<at("f3_adopt"));
        assert!(at("f3_adopt")<at("f3_dispenser_binding"));
        assert!(at("f3_dispenser_binding")<at("f3_factory_stage"));
        let second=prepare(&mut world,&mut cx,"a").unwrap();
        assert_eq!(second.stage.cookie,1);
        assert!(!second.calls.iter().any(|(_,name,_)|matches!(*name,"cmb_class"|"cmb_weapon"|"f3_weapon_services"|"f3_unit_items"|"clip_new")));
        let alias=prepare(&mut world,&mut cx,"alias").unwrap();
        assert_eq!(alias.stage.cookie,2);
        assert!(!alias.calls.iter().any(|(_,name,_)|matches!(*name,"cmb_class"|"cmb_weapon"|"f3_weapon_services"|"f3_unit_items"|"clip_new")));
        let alias_name=cx.strings.intern_str("alias");
        let alias_body=world.acquire_spawn_body(alias_name).unwrap();
        assert_eq!(world.sim.store.unit.get(alias_body).unwrap().class_name,alias_name);
        assert!(world.acquire_spawn_body(alias_name).is_none());
        let class=cx.strings.intern_str("a");let active=world.acquire_spawn_body(class).unwrap();
        let unit=world.sim.store.unit.get(active).unwrap();
        assert_eq!(unit.combat.health,125.);assert_eq!(unit.combat.weapons.len(),1);
        assert!(unit.combat.random);
        let service=f3::weapon_service_in(&world.units,unit.combat.weapons[0].def,0.);
        assert!(service.ordnance.is_some() && service.item.is_some());
        assert_eq!(world.foreign_unit_rows(),0);
        assert!(world.acquire_spawn_body(class).is_some());assert!(world.acquire_spawn_body(class).is_none());
    }

    #[test]
    fn foreign_rows_follow_factory_membership_lives_and_native_owners() {
        let mut w = World::new();
        assert_eq!(w.foreign_unit_rows(), 0);
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        // More bodies than a map fields: count the whole construction order,
        // without a fixed page-sized limit or an allocation while counting.
        f3::reserve_native_slots_in(&mut w, 4098);
        for i in 1..=4096 {
            let key = f3::with_owner_rows(&mut w, |store, owner| owner.build(store, Unit {
                class_name: StrId(0), alive: true, ..Default::default()
            }));
            assert_eq!(key.slot, i - 1);
        }
        assert_eq!(w.foreign_unit_rows(), 4096);
        let removed = w.sim.store.unit.reference(7).unwrap();
        w.sim.store.unit.get_mut(removed).unwrap().removed = true;
        let bot = w.sim.store.unit.reference(2).unwrap();
        f3::native_intent_in(&mut w, bot, true);
        let human = w.sim.store.unit.reference(3).unwrap();
        w.sim.store.unit.get_mut(human).unwrap().network_human = Some(true);
        assert_eq!(w.foreign_unit_rows(), 4094);
        w.input.players.active = true;
        assert_eq!(w.foreign_unit_rows(), 4093);
        // A passive viewpoint and unadopted stock are in the slab, while
        // hull targets are in the mixed registry; none is a factory row.
        let passive = w.sim.store.unit.insert(Unit::default());
        f3::with_owner_rows(&mut w, |store, owner| { owner.adopt_passive(store, passive).unwrap(); });
        let stock = w.sim.store.unit.insert(Unit::default());
        w.sim.store.units.list.push(Member::VehicleTarget(Ref::new(stock.slot + 1, 0)));
        assert_eq!(w.foreign_unit_rows(), 4093);
        // A corpse still needs a foreign row until it is removed. A pooled
        // respawn with another life must lose the previous bot's authority.
        w.sim.store.unit.get_mut(removed).unwrap().removed = false;
        w.sim.store.unit.get_mut(removed).unwrap().dead = true;
        assert_eq!(w.foreign_unit_rows(), 4094);
        f3::native_intent_in(&mut w, bot, false);
        assert_eq!(w.foreign_unit_rows(), 4095);
    }
}

impl World {
    /// Holder admission precedes a crew body's F2RiderBirth rows. Prepared
    /// hull variants use the same numeric seat facts but no live fleet index.
    pub fn admit_spawn_hull(&mut self, facts: &crate::gen::SeatHullFacts, prepared: bool) {
        self.sim.calls.result(crate::gen::SeatHullFacts::WORDS, |row, heap| facts.put(row, heap));
        crate::vehicles::module::f2::live_seats::f2_seats_adopt_in(self, u32::from(prepared));
    }
    /// Stationary crew holder built by props.js Turret and F2Seats.attachTurrets.
    pub fn admit_spawn_turret(&mut self, birth: &crate::gen::F2TurretBirth) {
        self.sim.calls.result(crate::gen::F2TurretBirth::WORDS, |row, heap| birth.put(row, heap));
        crate::vehicles::module::f2::live_seats::f2_turret_adopt_in(self);
    }
}

/// UnitFactory.build with shared admission identities. Preceding class and
/// service owners must have filled Cx.ids; no identity is guessed here.
pub fn build(cx: &mut super::rows::Cx,
    class: &bfe_content::defs::ClassDef, key: crate::gen::F3UnitKey, passive: bool,
    interns: &mut super::Call) -> Result<super::bodies::Body, super::AdmitError>
{
    let content=cx.content;
    super::bodies::from_context(cx, content, class, key, passive, interns)
}

/// Append the typed cold stage at its actual construction point. The caller
/// supplies the cookie shared with F3Factory.ready, including resident slots.
pub fn cold_stage(rows: &mut super::Rows, body: &super::bodies::Body, cookie: u32) -> crate::gen::F3ColdStage {
    let stage = body.stage(cookie);
    rows.cold_stages.push(stage.clone());
    stage
}

/// The body/cookie and ordered construction calls produced by prepare.
/// The caller appends `stage` to Rows.cold_stages at this admission point.
pub struct Prepared {
    pub stage: crate::gen::F3ColdStage,
    pub calls: super::rows::Calls,
}

/// F3Factory.prepare for one connection's possible class. Unit/ordnance/item
/// services and holder identities are earlier-owner prerequisites. Cx retains
/// their live-hull, prepared-hull, turret order from F2Riders.ensureAll.
/// Assets and class definitions are the shared admission catalog; acquisition
/// only moves these already constructed resources.
pub fn prepare(world: &mut World, cx: &mut super::rows::Cx, class_name: &str) -> Result<Prepared, super::AdmitError> {
    cx.refresh_strings(&world.sim.strings);
    let holders=std::mem::take(&mut cx.holders);
    let result=prepare_body(world,cx,class_name,&holders);
    cx.holders=holders;
    result
}

fn prepare_body(world: &mut World, cx: &mut super::rows::Cx, class_name: &str,
    holders: &[super::bodies::Holder]) -> Result<Prepared, super::AdmitError>
{
    use crate::gen::*;
    use super::{rows::{Call, combat, units}, Stage};
    let gameplay=cx.ids.battle.as_ref().ok_or(super::AdmitError::Unavailable(Stage::LiveTail))?.gameplay;
    let parts = super::bodies::prepare::parts(cx,class_name)?;
    let source = world.sim.store.units.next_id;
    if !source.is_finite() || source < 0. || source.fract() != 0. || source > u32::MAX as f64 {
        return Err(super::AdmitError::Refused {stage:Stage::Player,call:"unit construction slot".into()});
    }
    let key = F3UnitKey { slot: source as u32, generation: 0, life: 0. };
    let bank = parts.walker.unwrap_or(parts.shared);
    let cookie = crate::units::f3_construction::prepared_count_in(&world.units) as u32;
    let bank_key = clip_bank_key(&parts, cookie);
    let mut calls=Vec::new();let mut clip_ids=Vec::with_capacity(bank.list.len());
    // buildBody's page body runs before CombatFacade.attach or the first
    // Unit import: a soldier's body module, or a droideka's animator and
    // muzzle samples over their own clip copies (stock.rs page_body).
    super::stock::page_body(world,cx,&parts,class_name,&mut calls)?;
    let combat_calls = combat::attach_unit(cx,key.slot,parts.def,false,Stage::Player)?;
    // Registration calls precede the full Unit import. cmb_new itself runs
    // inside install_spawn_body once the complete body has its slab identity.
    for (_,name,call) in &combat_calls {
        super::sequence::args(world,call)?;
        match *name {
            "cmb_weapon" => crate::combat::module::cmb_weapon_in(world,call.args[0] as u32),
            "cmb_class" => crate::combat::module::cmb_class_in(world,call.args[0] as u32),
            "cmb_explosion" => crate::combat::module::cmb_explosion_in(world,call.args[0] as u32),
            "cmb_new" | "cmb_set" => {},
            _ => return Err(super::AdmitError::NoBody {stage:Stage::Player,call:(*name).into()}),
        }
    }
    calls.extend(combat_calls);
    let mut interns = Call::default();
    let body = super::bodies::prepare::body(cx,&parts,class_name,key.clone(),false,&mut interns)?;
    super::sequence::args(world,&interns)?;
    let mut skeleton_call = units::call(F3BodySkeleton::WORDS,|r,h|body.skeleton.put(r,h));
    skeleton_call.strings = interns.strings;
    calls.push((Stage::Player,"f3_body_skeleton",skeleton_call));
    // prepareSkeleton registers the whole animator bank, preserving Map
    // order while sharing soldier clips by their bone-name bank identity.
    for clip in &bank.list {
        if !clip_ids.iter().any(|(name,_)|*name==clip.name) {
            clip_ids.push((clip.name.as_str(),register_clip(world,cx,clip,&bank_key,&mut calls)?));
        }
    }
    let droideka_class = if parts.walker.is_some() {
        Some(droideka_class_id(world,cx,&parts,body.combat.class,&clip_ids,&mut calls,Stage::Player)?)
    } else { None };
    let services=units::prepare_unit_services(cx,parts.class,key.clone())?;
    apply_construction_calls(world,cx,&services)?;calls.extend(services);
    let mut riders = Vec::with_capacity(holders.len());
    for holder in holders {
        riders.push(super::bodies::rider_birth(key.reference(),key.life,body.unit.root_scale,bank,holder,
            |clip|clip_ids.iter().find(|(name,_)|*name==clip.name).map_or(0,|(_,id)|*id)));
    }
    let stage = body.stage(cookie);
    if let Some(class) = droideka_class {
        calls.push((Stage::Player,"units_droideka_attach",Call {args:vec![key.slot as f64,class as f64],..Default::default()}));
        if let Some(muzzles) = &body.muzzles { calls.push((Stage::Player,"units_muzzle_attach",Call {args:vec![key.slot as f64,class as f64],row:muzzles.words.clone(),..Default::default()})); }
    }
    let adoption=world.install_spawn_body(body,droideka_class,&[])?;
    calls.push((Stage::Player,"f3_adopt",units::call(F3Adoption::WORDS,|r,h|adoption.put(r,h))));
    // F3Items.flush runs after adoption, before F4's held-frame preparation
    // and F2's rider construction. Its keys include this body's qualified life.
    let dispensers=units::dispenser_calls(cx,parts.class,key.clone())?;
    apply_construction_calls(world,cx,&dispensers)?;calls.extend(dispensers);
    for rider in &riders {
        world.sim.calls.result(F2RiderBirth::WORDS,|r,h|rider.put(r,h));
        crate::vehicles::module::f2::riders::f2_rider_birth_in(world);
        calls.push((Stage::Player,"f2_rider_birth",units::call(F2RiderBirth::WORDS,|r,h|rider.put(r,h))));
    }
    // game.js instrument runs after F3's adoption and crew preparation.
    // Assigning CombatFacade.random emits the flag setter, then refresh binds
    // its gameplay callback. Registration was earlier; this never draws it.
    for (action,field,value) in [(F3Action::CombatField,F3CombatField::Random as u32,1.),
        (F3Action::RandomBinding,0,gameplay as f64)] {
        let change=F3Change {key:key.clone(),action,field,value,vector:cx.player.command_vector,
            on_shield:cx.player.on_shield,attacker:Attacker::Null,..Default::default()};
        change.put(crate::units::f3::command_row_in(&mut world.units),&mut Vec::new());
        if crate::units::f3::f3_command_in(world)==0 {
            return Err(super::AdmitError::Refused {stage:Stage::Player,call:"f3_command".into()});
        }
        calls.push((Stage::Player,"f3_command",units::call(F3Change::WORDS,|r,h|change.put(r,h))));
    }
    cx.refresh_strings(&world.sim.strings);
    world.stage_spawn_body(&stage)?;
    calls.push((Stage::Player,"f3_factory_stage",units::call(F3ColdStage::WORDS,|r,h|stage.put(r,h))));
    Ok(Prepared {stage,calls})
}
/// The droideka class F3 registers once per class definition
/// (units_droideka_class over the walker bank's clip ids), shared by every
/// droideka body: its existing id, or the new registration's.
fn droideka_class_id(world: &mut World, cx: &mut super::rows::Cx, parts: &super::bodies::prepare::Parts, class: crate::store::DefId,
    clip_ids: &[(&str, u32)], calls: &mut super::rows::Calls, stage: super::Stage) -> Result<u32, super::AdmitError> {
    use crate::gen::F3DroidekaClass;
    use super::rows::units;
    let path=cx.ids.defs.iter().find(|(key,id)|key.starts_with("class:") && *id==class.0 as f64)
        .map(|(key,_)|key.trim_start_matches("class:").to_string())
        .ok_or_else(||super::AdmitError::Content {stage,message:"droideka has no combat class identity".into()})?;
    let name=format!("droideka:{path}");
    if let Some((_,id)) = cx.ids.classes.iter().find(|(key,_)|key == &name) { return Ok(*id as u32); }
    let row = units::droideka_class(&parts.def,|name|clip_ids.iter().find(|(key,_)|*key==name).map(|(_,id)|*id));
    let call = units::call(F3DroidekaClass::WORDS,|r,h|row.put(r,h));
    super::sequence::args(world,&call)?;
    let id = crate::units::module::units_droideka_class_in(world);
    cx.ids.classes.push((name,id as f64)); cx.returned.push(("units_droideka_class",id as f64));
    calls.push((stage,"units_droideka_class",call));
    Ok(id)
}

/// game.js 3111 headless: `unitsSim.adopt(playerUnit, true)` then
/// `unitsSim.registerViewpoint(playerUnit)` and F3Factory.resident. The
/// dormant viewpoint is the body PlayerBodies built and placed before the
/// handoff: its Unit import carries the body_spawn answer (the grounded
/// position, velocity and posture the page's spawnSoldier published onto
/// the soldier) and the placement's generation, alive flags and tick count
/// (game.js placePlayer). F3 then prepares its skeleton and whole clip bank,
/// adopts it passively (no factory order, no corpse pool), binds its combat
/// and weapon random callbacks (F3Random.refresh: every one is the gameplay
/// stream), births its riders for every holder, keeps its class resources
/// resident under the first cookie and registers it as the viewpoint.
pub fn adopt_viewpoint(world: &mut World, cx: &mut super::rows::Cx, slot: u32, stage: super::Stage) -> Result<super::rows::Calls, super::AdmitError> {
    use crate::gen::*;
    use super::rows::{Call, battle::streams, units};
    let class_name = cx.battle.policy.class_name.clone();
    let parts = super::bodies::prepare::parts(cx, &class_name)?;
    // placePlayer raised the generation once (0 to 1) when it spawned the body.
    let key = F3UnitKey { slot, generation: 0, life: 1.0 };
    // body_spawn answers its block: the header, then the output row.
    let header = crate::store::block::HEADER;
    enum Spawned { Soldier(BodySpawnOutput), Droideka(DroidekaSpawnOutput) }
    let spawned = if parts.walker.is_some() {
        cx.returned_row("body_spawn_droideka").filter(|row| row.len() >= header + DroidekaSpawnOutput::WORDS)
            .map(|row| Spawned::Droideka(DroidekaSpawnOutput::take(&row[header..], &[])))
    } else {
        cx.returned_row("body_spawn").filter(|row| row.len() >= header + BodySpawnOutput::WORDS)
            .map(|row| Spawned::Soldier(BodySpawnOutput::take(&row[header..], &[])))
    }.ok_or_else(|| super::AdmitError::Content { stage, message: "the viewpoint body was not spawned before its adoption".into() })?;
    let mut interns = Call::default();
    let mut body = super::bodies::prepare::body(cx, &parts, &class_name, key.clone(), true, &mut interns)?;
    super::sequence::args(world, &interns)?;
    // publishBodySpawnOutput (or publishDroidekaSpawnOutput) onto the
    // page body, then placePlayer's flags.
    match (&mut body.unit.soldier, spawned) {
        (UnitBody::Soldier(soldier), Spawned::Soldier(spawned)) => {
            soldier.position = spawned.position; soldier.previous = spawned.previous; soldier.settled = spawned.settled;
            // The soldier row keeps no `origin`: the page's body alone reads it.
            soldier.velocity = spawned.velocity; soldier.normal = spawned.normal;
            soldier.aim_yaw = spawned.aim_yaw; soldier.aim_pitch = spawned.aim_pitch; soldier.posture = spawned.posture;
            soldier.air = spawned.air; soldier.dive = spawned.dive; soldier.state_time = spawned.state_time;
            soldier.lock = spawned.lock; soldier.fuel = spawned.fuel; soldier.grounded = spawned.grounded;
            soldier.ground_adjustment = spawned.ground_adjustment;
        }
        (UnitBody::Droideka(droideka), Spawned::Droideka(spawned)) => {
            droideka.position = spawned.position; droideka.previous = spawned.previous; droideka.settled = spawned.settled;
            droideka.velocity = spawned.velocity; droideka.aim_yaw = spawned.aim_yaw; droideka.aim_pitch = spawned.aim_pitch;
            droideka.legs = spawned.legs; droideka.spin = spawned.spin; droideka.dead = spawned.dead;
            droideka.mode = spawned.mode; droideka.time = spawned.time; droideka.reverse = spawned.reverse;
            if let Some(shield) = droideka.shield.as_mut() { shield.value = spawned.shield_value; shield.on = spawned.shield_on; }
        }
        _ => return Err(super::AdmitError::Content { stage, message: "the viewpoint body and its spawn disagree on its base".into() }),
    }
    body.unit.alive = true; body.unit.dead = false; body.unit.removed = false; body.unit.targetable = Some(true);
    body.unit.tick_count = 1.0;
    // game.js: `new Unit({..., human: true})`, `playerUnit.team =
    // playerTeam`, and createNativeAI's `playerUnit.character = battle.human`
    // all precede the headless adoption, so its import carries them.
    body.unit.human = true;
    body.unit.team = cx.battle.policy.team;
    body.unit.character = world.sim.store.battle.characters.iter().copied()
        .find(|r| world.sim.store.character.get(*r).is_some_and(|c| c.human));
    let mut calls = Vec::new();
    let fail = |call: &str| super::AdmitError::Refused { stage, call: call.into() };
    if !crate::units::f3::active_in(&world.units) { return Err(fail("f3_init")); }
    // The Unit import (KIND_UNIT) replaces the row attachF3Combat's cmb_new
    // opened for the viewpoint's combat, so no cmb_new here, unlike
    // UnitFactory.build; the import carries the page's whole unit state.
    let reference = match world.sim.store.unit.reference(slot) {
        Some(existing) => {
            if existing.gen != 0 { return Err(fail("viewpoint slot")); }
            // The import carries the page's unit; the combat attachF3Combat
            // opened for it (cmb_new, its arsenal and settings) stays native.
            let row = world.sim.store.unit.get_mut(existing).ok_or_else(|| fail("viewpoint slot"))?;
            let combat = std::mem::take(&mut row.combat);
            *row = body.unit;
            row.combat = combat;
            existing
        }
        None => world.sim.store.unit.insert_at(slot, 0, body.unit),
    };
    // remotesActive, the import's `combat.remotes.active`: defined once the
    // combat has remotes (the player's, cmb_set Remotes at the Ai stage),
    // false until a droid is piloted.
    let piloted = crate::recon::module::active_in(world, reference).unwrap_or(false);
    if let Some(unit) = world.sim.store.unit.get_mut(reference) {
        unit.remotes_active = unit.combat.has_remotes.then_some(piloted);
    }
    let mut skeleton_call = units::call(F3BodySkeleton::WORDS, |r, h| body.skeleton.put(r, h));
    skeleton_call.strings = interns.strings;
    world.sim.calls.result(F3BodySkeleton::WORDS, |row, heap| body.skeleton.put(row, heap));
    if crate::units::live_pose::f3_body_skeleton_in(world) == 0 { return Err(fail("f3_body_skeleton")); }
    calls.push((stage, "f3_body_skeleton", skeleton_call));
    // prepareSkeleton registers the whole animator bank in Map order (a
    // droideka's is its own walker bank). The player's actor carries its own
    // animator (PlayerBodies, not UnitFactory.clipsFor): the kernel copies
    // its whole bank again.
    let bank = parts.walker.unwrap_or(parts.shared);
    let bank_key = format!("viewpoint:{}", clip_bank_key(&parts, 0));
    let mut clip_ids = Vec::with_capacity(bank.list.len());
    for clip in &bank.list { clip_ids.push((clip.name.as_str(), register_clip(world, cx, clip, &bank_key, &mut calls)?)); }
    // A droideka's class (once per definition), then its walker attachment
    // and hidden muzzle rig, before the adoption (install_spawn_body's order).
    if parts.walker.is_some() {
        let class = droideka_class_id(world, cx, &parts, body.combat.class, &clip_ids, &mut calls, stage)?;
        if let Some(unit) = world.sim.store.unit.get_mut(reference) { unit.combat.defer_muzzles = true; }
        crate::units::module::units_droideka_attach_in(world, slot, class);
        calls.push((stage, "units_droideka_attach", Call { args: vec![slot as f64, class as f64], ..Default::default() }));
        let muzzles = body.muzzles.as_ref().ok_or_else(|| fail("units_muzzle_attach"))?;
        world.sim.calls.result(muzzles.words.len(), |row, _| row.copy_from_slice(&muzzles.words));
        crate::units::module::units_muzzle_attach_in(world, slot, class);
        calls.push((stage, "units_muzzle_attach", Call { args: vec![slot as f64, class as f64], row: muzzles.words.clone(), ..Default::default() }));
    }
    // prepareWeapons and items.prepareUnit: attachF3 serviced this class
    // already, so the producer answers only what is still unsent.
    let services = units::prepare_unit_services(cx, parts.class, key.clone())?;
    apply_construction_calls(world, cx, &services)?; calls.extend(services);
    body.adoption.put(crate::units::f3::command_row_in(&mut world.units), &mut Vec::new());
    if crate::units::f3::f3_adopt_in(world) == 0 { return Err(fail("f3_adopt")); }
    calls.push((stage, "f3_adopt", units::call(F3Adoption::WORDS, |r, h| body.adoption.put(r, h))));
    cx.refresh_strings(&world.sim.strings);
    // F3Random.refresh: index 0 is combat's generator, the arsenal follows;
    // the player's arsenal holds every weapon its class lists, each with
    // the gameplay draw (the native row's arsenal is filled later).
    let weapons = parts.def.get("weapons").and_then(bfe_content::Value::as_array).map_or(0, |w| w.len());
    let gameplay = cx.streams.register(streams::GAMEPLAY) as f64;
    for index in 0..=weapons as u32 {
        let change = F3Change { key: key.clone(), action: F3Action::RandomBinding, index, value: gameplay,
            vector: cx.player.command_vector, on_shield: cx.player.on_shield, attacker: Attacker::Null, ..Default::default() };
        change.put(crate::units::f3::command_row_in(&mut world.units), &mut Vec::new());
        if crate::units::f3::f3_command_in(world) == 0 { return Err(fail("f3_command")); }
        calls.push((stage, "f3_command", units::call(F3Change::WORDS, |r, h| change.put(r, h))));
    }
    // F2Riders.ensureAll over the fleet's holders, then the resident cookie.
    let holders = std::mem::take(&mut cx.holders);
    let root_scale = world.sim.store.unit.get(reference).map_or(1.0, |u| u.root_scale);
    for holder in &holders {
        let rider = super::bodies::rider_birth(key.reference(), key.life, root_scale, bank, holder,
            |clip| clip_ids.iter().find(|(name, _)| *name == clip.name).map_or(0, |(_, id)| *id));
        world.sim.calls.result(F2RiderBirth::WORDS, |r, h| rider.put(r, h));
        crate::vehicles::module::f2::riders::f2_rider_birth_in(world);
        calls.push((stage, "f2_rider_birth", units::call(F2RiderBirth::WORDS, |r, h| rider.put(r, h))));
    }
    cx.holders = holders;
    let cookie = crate::units::f3_construction::prepared_count_in(&world.units) as u32;
    let resident = F3ColdStage { cookie, adoption: body.adoption.clone() };
    if !world.resident_spawn_body(&resident) { return Err(fail("f3_factory_resident")); }
    calls.push((stage, "f3_factory_resident", units::call(F3ColdStage::WORDS, |r, h| resident.put(r, h))));
    let viewpoint = F3Change { key, action: F3Action::Viewpoint, vector: cx.player.command_vector,
        on_shield: cx.player.on_shield, attacker: Attacker::Null, ..Default::default() };
    viewpoint.put(crate::units::f3::command_row_in(&mut world.units), &mut Vec::new());
    if crate::units::f3::f3_command_in(world) == 0 { return Err(fail("f3_command")); }
    calls.push((stage, "f3_command", units::call(F3Change::WORDS, |r, h| viewpoint.put(r, h))));
    cx.refresh_strings(&world.sim.strings);
    Ok(calls)
}

pub(crate) fn apply_construction_calls(world: &mut World, cx: &mut super::rows::Cx, calls: &super::rows::Calls) -> Result<(),super::AdmitError> {
    for (stage,name,call) in calls {
        super::sequence::args(world,call)?;
        let returned=super::dispatch::call(world,*stage,name,call)?;
        super::dispatch::record(cx,*stage,name,returned)?;
        cx.refresh_strings(&world.sim.strings);
    }
    Ok(())
}

pub(crate) fn register_clip(world: &mut World, cx: &mut super::rows::Cx, clip: &bfe_content::clips::Clip,
    bank: &str, calls: &mut super::rows::Calls) -> Result<u32,super::AdmitError>
{
    use super::{rows::Call,Stage};
    let name=format!("{bank}\0{}",clip.name);
    if let Some((_,id))=cx.ids.clips.iter().find(|(key,_)|key==&name) {
        if !id.is_finite() || *id<0. || id.fract()!=0. || *id>=world.pose.clips.len() as f64 {
            return Err(super::AdmitError::Refused {stage:Stage::Player,call:"clip identity".into()});
        }
        return Ok(*id as u32);
    }
    let id=world.pose.clips.len() as u32;
    world.pose.clips.push(bfe_pose::rig::content::clip(clip));
    cx.ids.clips.push((name,id as f64));cx.returned.push(("clip_new",id as f64));
    calls.push((Stage::Player,"clip_new",Call {args:vec![clip.frames_f64(),clip.tracks.len() as f64,clip.fps,u8::from(clip.root_motion) as f64],..Default::default()}));
    calls.push((Stage::Player,"clip_data",Call {args:vec![id as f64],scratch:clip.data().iter().flat_map(|v|v.to_le_bytes()).collect(),..Default::default()}));
    calls.push((Stage::Player,"clip_tracks",Call {args:vec![id as f64],scratch:clip.tracks.iter().flat_map(|v|(*v as i32).to_le_bytes()).collect(),..Default::default()}));
    Ok(id)
}
pub(crate) fn clip_bank_key(parts: &super::bodies::prepare::Parts, cookie: u32) -> String {
    let names = parts.actor.bones.iter().map(|b|b.name.as_str()).collect::<Vec<_>>().join(",");
    if parts.walker.is_some() {
        format!("walker:{}/{}:{cookie}:{names}",parts.side,parts.def.get("clips").and_then(bfe_content::Value::as_str).unwrap_or(""))
    } else { format!("soldier:{names}") }
}

/// UnitFactory.prepare's asset prerequisite before roster reservation. Battle
/// already fetched every infantry body; this verifies the requested class
/// without constructing stock or consuming a registry id/cookie.
pub fn load(cx: &super::rows::Cx, class_name: &str) -> Result<(), super::AdmitError> {
    super::bodies::prepare::parts(cx,class_name).map(|_| ())
}
