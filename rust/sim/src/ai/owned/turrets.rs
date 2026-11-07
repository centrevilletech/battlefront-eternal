//! Stationary AI reads F2 crew aim and F4 building life at each observation.
//! Only authored limits and reverse indices are retained here. Reading a
//! scalar view never poses a rig; origin is a separate lazy operation.
use super::entities::{Entities, Entity};
use crate::ai::{
    combat::world::{LeaseView, Sphere},
    vehicles::{
        service::prepare_mount_limits,
        world::{Obj, TurretView},
    },
};
use crate::{
    battle::{live_phase::Building, ownership::Rows},
    gen::{F4Handle, SeatLease, Store, Turret},
    seats::turrets::TurretSide,
    store::Ref,
    vehicles::f2_seats::Seats,
};
use std::rc::Rc;
use three::Vector3;
struct Binding {
    reference: Ref<Turret>,
    owner: F4Handle,
    crew: usize,
    entry: Obj,
    yaw: Option<Rc<str>>,
    pitch: Option<Rc<str>>,
    limits: [[f64; 2]; 2],
    target: Option<Target>,
    target_class: Option<crate::ai::vehicles::world::ClassView>,
    control_class: Rc<crate::ai::control::world::ClassSnap>,
    combat_class: crate::ai::combat::world::ClassView,
    vehicle_class: crate::ai::vehicles::world::ClassView,
}
/// Authored registry metadata. Health, life, crew and aim remain with their
/// F4/F2 owners; these class handles are shared after construction.
pub struct Target {
    pub member: crate::gen::Member,
    pub team: f64,
    pub name: Rc<str>,
    pub policy: Rc<crate::ai::control::world::ClassSnap>,
    pub combat: crate::ai::combat::world::ClassView,
}
struct Control {
    reference: Ref<crate::gen::SeatUnit>,
    lease: Ref<SeatLease>,
    turret: Ref<Turret>,
    life: f64,
    id: f64,
    hero: bool,
    controller: Option<Obj>,
    steer: Option<crate::ai::vehicles::world::Steer>,
}
pub struct Turrets {
    rows: Vec<Option<Binding>>,
    controls: Vec<Option<Control>>,
    stand: Rc<str>,
}
impl Turrets {
    pub fn new(buildings: usize, controls: usize) -> Self {
        Self {
            rows: Vec::with_capacity(buildings),
            // Controls share slots with hull seats. A later turret rider can
            // have a slot above the number of authored turret buildings.
            controls: Vec::with_capacity(controls),
            stand: Rc::from("stand"),
        }
    }
    /// Construction order is shared with F2. A building handle retains its
    /// storage generation while current() follows rebuilds of that object.
    pub fn register(
        &mut self,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &mut Entities,
        reference: Ref<Turret>,
        owner: F4Handle,
        yaw: Option<Rc<str>>,
        pitch: Option<Rc<str>>,
    ) {
        assert!(buildings.get(&owner).is_some(), "prepared turret building");
        let crew = seats
            .crews
            .turrets
            .iter()
            .position(|t| t.turret == reference)
            .expect("prepared F2 turret crew");
        entities.register(Entity::Turret(reference));
        let entry = entities.register(Entity::TurretEntry(reference));
        let t = &seats.crews.turrets[crew];
        let max_health = buildings.get(&owner).unwrap().settings.max_health;
        let limits = prepare_mount_limits(yaw.as_deref(), pitch.as_deref(), false);
        let slot = reference.slot as usize;
        if self.rows.len() <= slot {
            self.rows.resize_with(slot + 1, || None);
        }
        self.rows[slot] = Some(Binding {
            reference,
            owner,
            crew,
            entry,
            yaw,
            pitch,
            limits,
            target: None,
            target_class: None,
            control_class: Rc::new(crate::ai::control::world::ClassSnap {
                base: Some("armedbuilding".into()),
                max_speed: Some(0.),
                forward_speed: Some(0.),
                strafe_speed: Some(0.),
                turn_rate: Some(t.desc.turn_rate),
                pitch_rate: Some(t.desc.turn_rate),
                max_health: Some(max_health),
                prepared_travel_mask: Some(1.),
                prepared_role: Some(6.),
                prepared_health_type: Some(0.),
                ..Default::default()
            }),
            combat_class: crate::ai::combat::world::ClassView {
                fields:Rc::new(crate::ai::combat::world::ClassFields {
                max_speed: crate::combat::defs::Js::Num(0.),
                forward_speed: crate::combat::defs::Js::Num(0.),
                max_health: crate::combat::defs::Js::Num(max_health),
                ..Default::default()
                }),
                prepared_role: Some(6.),
                prepared_category: Some(0.),
            },
            vehicle_class: crate::ai::vehicles::world::ClassView {
                max_speed: Some(0.),
                forward_speed: Some(0.),
                strafe_speed: Some(0.),
                turn_rate: Some(t.desc.turn_rate),
                pitch_rate: Some(t.desc.turn_rate),
                role: 6.,
                ..Default::default()
            },
        });
    }
    /// A seat control captures occupant identity and hero status at acquisition.
    /// A new life must acquire a new lease/control, even on the same turret.
    pub fn bind_control(
        &mut self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &mut Entities,
        reference: Ref<crate::gen::SeatUnit>,
        lease: Ref<SeatLease>,
        turret: Ref<Turret>,
        controller: Option<Obj>,
        occupant: super::seats::Occupant,
    ) -> bool {
        let Some((_, t, b)) = self.current(seats, buildings, turret) else {
            return false;
        };
        let Some(l) = store.seat_lease.get(lease) else {
            return false;
        };
        if t.lease != Some(lease)
            || l.turret != Some(turret)
            || l.controllable != Some(reference)
            || l.index != 0.
        {
            return false;
        }
        let Some(life) = b.state.building.target_generation else {
            return false;
        };
        let id=occupant.id;let hero=occupant.hero;
        entities.register(Entity::Control(reference));
        entities.register(Entity::Lease(lease));
        let slot = reference.slot as usize;
        if self.controls.len() <= slot {
            self.controls.resize_with(slot + 1, || None);
        }
        self.controls[slot] = Some(Control {
            reference,
            lease,
            turret,
            life,
            id,
            hero,
            controller,
            steer: None,
        });
        true
    }
    fn control(&self, reference: Ref<crate::gen::SeatUnit>) -> Option<&Control> {
        self.controls
            .get(reference.slot as usize)?
            .as_ref()
            .filter(|c| c.reference == reference)
    }
    fn live_control<'a>(
        &'a self,
        store: &'a Store,
        seats: &'a Seats,
        buildings: &'a Rows<Building>,
        reference: Ref<crate::gen::SeatUnit>,
    ) -> Option<(
        &'a Control,
        &'a Binding,
        &'a TurretSide,
        &'a Building,
        &'a crate::gen::SeatLease,
    )> {
        let c = self.control(reference)?;
        let (b, t, building) = self.current(seats, buildings, c.turret)?;
        let l = store.seat_lease.get(c.lease)?;
        if building.state.building.target_generation != Some(c.life)
            || t.lease != Some(c.lease)
            || l.turret != Some(c.turret)
            || l.controllable != Some(reference)
            || l.index != 0.
        {
            return None;
        }
        Some((c, b, t, building, l))
    }
    pub fn policy_control(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        reference: Ref<crate::gen::SeatUnit>,
        character: &impl Fn(Ref<crate::gen::Character>) -> usize,
    ) -> Option<crate::ai::control::world::AiUnit> {
        use crate::ai::{
            control::world::{AiUnit, SoldierSnap},
            nav::math::Vec3,
        };
        let (c, b, t, building, l) = self.live_control(store, seats, buildings, reference)?;
        let position = self.position(seats, buildings, c.turret)?;
        Some(AiUnit {
            key: entities.find(Entity::Control(reference))?,
            id: c.id,
            generation: 1.,
            kind: 7.,
            team: l.team,
            alive: building.state.building.alive()
                && crate::seats::lease::live(l, c.lease, &store.unit),
            hero: c.hero,
            targetable: Some(false),
            position: Vec3::new(position.x, position.y, position.z),
            velocity: Some(Vec3::default()),
            soldier: Some(SoldierSnap {
                aim_yaw: Some(t.desc.facing + t.yaw),
                aim_pitch: Some(t.pitch),
                max_speed: Some(0.),
                max_strafe: Some(0.),
                max_turn: b.control_class.turn_rate,
                max_pitch: b.control_class.pitch_rate,
                posture: Some(crate::gen::Posture::Stand),
                ..Default::default()
            }),
            class_def: Some(b.control_class.clone()),
            class_name: Some(b.target.as_ref()?.name.clone()),
            combat_health: Some(building.state.building.health),
            combat_max_health: Some(building.settings.max_health),
            seat: true,
            has_lease: true,
            lease_is_self: true,
            is_seat_unit: true,
            has_turret: true,
            character: l
                .unit
                .and_then(|r| store.unit.get(r))
                .and_then(|u| u.character)
                .map(character),
            ..Default::default()
        })
    }
    /// Scalar SeatUnit observations keep the same turret/lease life checks.
    /// A policy view also requires its prepared registry class name.
    pub fn scalars(&self,store:&Store,seats:&Seats,buildings:&Rows<Building>,reference:Ref<crate::gen::SeatUnit>,policy:bool)->Option<crate::ai::control::world::UnitScalars> {
        let (c,b,_,building,l)=self.live_control(store,seats,buildings,reference)?;
        if policy {b.target.as_ref()?;}
        let p=self.position(seats,buildings,c.turret)?;
        Some(crate::ai::control::world::UnitScalars {generation:1.,kind:7.,team:l.team,
            alive:building.state.building.alive()&&crate::seats::lease::live(l,c.lease,&store.unit),
            position:crate::ai::nav::math::Vec3::new(p.x,p.y,p.z),has_lease:true,lease_is_self:true,hero:c.hero,human:false})
    }
    /// A stationary control keeps its rider's captured parity identity.
    pub fn motion(&self,store:&Store,seats:&Seats,buildings:&Rows<Building>,reference:Ref<crate::gen::SeatUnit>)->Option<crate::ai::control::world::UnitMotion> {
        let (c,b,_,_,_)=self.live_control(store,seats,buildings,reference)?;b.target.as_ref()?;
        let p=self.position(seats,buildings,c.turret)?;
        Some(crate::ai::control::world::UnitMotion {id:c.id,position:crate::ai::nav::math::Vec3::new(p.x,p.y,p.z),velocity:Some(Default::default())})
    }
    /// A turret's navigation control is created by scheduler input, after
    /// its stationary helper. The source SeatUnit keeps the rider's body life
    /// and its own independent planner identity through that construction.
    pub fn navigation(&self,store:&Store,seats:&Seats,buildings:&Rows<Building>,reference:Ref<crate::gen::SeatUnit>)->Option<(crate::gen::AiLifeHandle,Ref<SeatLease>,crate::ai::vehicles::world::ClassView)> {
        let (control,binding,_,_,lease)=self.live_control(store,seats,buildings,reference)?;
        let unit=lease.unit?;let body=store.unit.get(unit)?;
        Some((crate::gen::AiLifeHandle {kind:crate::gen::KIND_UNIT.into(),slot:unit.slot,generation:unit.gen,life:body.generation},control.lease,binding.vehicle_class.clone()))
    }
    pub fn vehicle_control(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        reference: Ref<crate::gen::SeatUnit>,
    ) -> Option<crate::ai::vehicles::world::UnitView> {
        use crate::ai::vehicles::world::UnitView;
        let (c, b, t, building, l) = self.live_control(store, seats, buildings, reference)?;
        let lease = entities.find(Entity::Lease(c.lease))?;
        Some(UnitView {
            obj: entities.find(Entity::Control(reference))?,
            id: c.id,
            generation: 1.,
            kind: 7.,
            team: l.team,
            alive: building.state.building.alive()
                && crate::seats::lease::live(l, c.lease, &store.unit),
            position: self.position(seats, buildings, c.turret)?,
            body_yaw: t.desc.facing + t.yaw,
            class: Some(b.vehicle_class.clone()),
            class_name: Some(b.target.as_ref()?.name.clone()),
            soldier: true,
            soldier_max_speed: Some(0.),
            seat: Some(lease),
            lease: Some(lease),
            turret: entities.find(Entity::Turret(c.turret)),
            controller: c.controller,
            character: l
                .unit
                .and_then(|r| store.unit.get(r))
                .and_then(|u| u.character)
                .map(|r| {
                    entities
                        .find(Entity::Character(r))
                        .expect("prepared turret rider Character")
                }),
            ..Default::default()
        })
    }
    pub fn lease_reference(&self,store:&Store,seats:&Seats,buildings:&Rows<Building>,reference:Ref<crate::gen::SeatUnit>)->Option<Ref<crate::gen::SeatLease>> {
        Some(self.live_control(store,seats,buildings,reference)?.0.lease)
    }
    pub fn combat_control(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        reference: Ref<crate::gen::SeatUnit>,
    ) -> Option<crate::ai::combat::world::UnitView> {
        use crate::{
            ai::combat::world::{SoldierView, UnitView},
            combat::defs::Js,
        };
        let (c, b, _, building, l) = self.live_control(store, seats, buildings, reference)?;
        Some(UnitView {
            obj: entities.find(Entity::Control(reference))?,
            id: Some(c.id),
            generation: 1.,
            kind: 7.,
            team: l.team,
            alive: building.state.building.alive()
                && crate::seats::lease::live(l, c.lease, &store.unit),
            hero: c.hero,
            targetable_false: true,
            position: self.position(seats, buildings, c.turret)?,
            velocity: Some(Vector3::default()),
            class: Some(b.combat_class.clone()),
            combat_max_health: Js::Num(building.settings.max_health),
            soldier: Some(SoldierView {
                posture: Js::Str(self.stand.clone()),
                max_speed: Js::Num(0.),
                ..Default::default()
            }),
            controller: c.controller,
            character: l
                .unit
                .and_then(|r| store.unit.get(r))
                .and_then(|u| u.character)
                .map(|r| {
                    entities
                        .find(Entity::Character(r))
                        .expect("prepared turret rider Character")
                }),
            turret: entities.find(Entity::Turret(c.turret)),
            seat: Some(lease_view(store, entities, self, c.lease)?),
            has_lease: true,
            has_origin: true,
            intent: true,
            intent_buttons: true,
            ..Default::default()
        })
    }
    pub fn command(
        &self,
        store: &Store,
        seats: &mut Seats,
        buildings: &Rows<Building>,
        reference: Ref<crate::gen::SeatUnit>,
        command: super::intent::Command,
    ) -> bool {
        let Some((c, _, _, building, l)) = self.live_control(store, seats, buildings, reference)
        else {
            return false;
        };
        if !building.state.building.alive() || !crate::seats::lease::live(l, c.lease, &store.unit) {
            return false;
        }
        let Some(control) = seats
            .crews
            .controls
            .get_mut(&crate::seats::lease::control_key(reference))
        else {
            return false;
        };
        command.seat(&mut control.intent);
        true
    }
    pub fn edit_steer<R>(
        &mut self,
        reference: Ref<crate::gen::SeatUnit>,
        edit: impl FnOnce(&mut Option<crate::ai::vehicles::world::Steer>) -> R,
    ) -> Option<R> {
        let c = self
            .controls
            .get_mut(reference.slot as usize)?
            .as_mut()
            .filter(|c| c.reference == reference)?;
        Some(edit(&mut c.steer))
    }
    pub fn link_controller(&mut self,control:Ref<crate::gen::SeatUnit>,controller:Obj,bound:bool)->Option<bool> {
        let row=self.controls.get_mut(control.slot as usize)?.as_mut().filter(|b|b.reference==control)?;
        let before=row.controller;
        if bound {row.controller=Some(controller);}else if row.controller==Some(controller) {row.controller=None;}
        Some(before!=row.controller)
    }

    pub fn controller_key(&self,control:Ref<crate::gen::SeatUnit>)->Option<Obj> {self.control(control)?.controller}
    pub fn retire_control(&mut self, reference: Ref<crate::gen::SeatUnit>) -> bool {
        if self.control(reference).is_none() {
            return false;
        }
        self.controls[reference.slot as usize] = None;
        true
    }
    pub fn target(&mut self, reference: Ref<Turret>, mut target: Target) -> Option<Obj> {
        assert!(
            matches!(target.member, crate::gen::Member::TurretTarget(_)),
            "turret registry identity"
        );
        let row = self
            .rows
            .get_mut(reference.slot as usize)?
            .as_mut()
            .filter(|b| b.reference == reference)?;
        target.combat.prepared_role = Some(crate::ai::combat::agent::role_of(Some(&target.combat)));
        target.combat.prepared_category = Some(4.);
        row.target_class=Some(super::content::vehicle_class(&target.policy,0.));
        row.target = Some(target);
        Some(row.entry)
    }
    pub fn policy_target(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<crate::ai::control::world::AiUnit> {
        let (b, t, building) = self.current(seats, buildings, reference)?;
        let target = b.target.as_ref()?;
        let position = self.position(seats, buildings, reference)?;
        Some(crate::ai::control::world::AiUnit {
            key: b.entry,
            id: crate::units::registry::target_id(target.member) as f64,
            generation: building.state.building.target_generation?,
            kind: 8.,
            team: t
                .lease
                .and_then(|r| store.seat_lease.get(r))
                .map_or(target.team, |l| l.team),
            alive: building.state.building.alive(),
            position: crate::ai::nav::math::Vec3::new(position.x, position.y, position.z),
            velocity: Some(crate::ai::nav::math::Vec3::default()),
            class_def: Some(target.policy.clone()),
            class_name: Some(target.name.clone()),
            combat_health: Some(building.state.building.health),
            combat_max_health: Some(building.settings.max_health),
            has_turret: true,
            ..Default::default()
        })
    }
    pub fn combat_target(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        reference: Ref<Turret>,
    ) -> Option<crate::ai::combat::world::UnitView> {
        use crate::{ai::combat::world::UnitView, combat::defs::Js};
        let (b, t, building) = self.current(seats, buildings, reference)?;
        let target = b.target.as_ref()?;
        Some(UnitView {
            obj: b.entry,
            id: Some(crate::units::registry::target_id(target.member) as f64),
            generation: building.state.building.target_generation?,
            kind: 8.,
            category: Some(4.),
            team: t
                .lease
                .and_then(|r| store.seat_lease.get(r))
                .map_or(target.team, |l| l.team),
            alive: building.state.building.alive(),
            position: self.position(seats, buildings, reference)?,
            velocity: Some(Vector3::default()),
            class: Some(target.combat.clone()),
            combat_max_health: Js::Num(building.settings.max_health),
            turret: entities.find(Entity::Turret(reference)),
            has_origin: true,
            ..Default::default()
        })
    }
    fn binding(&self, reference: Ref<Turret>) -> Option<&Binding> {
        self.rows
            .get(reference.slot as usize)?
            .as_ref()
            .filter(|b| b.reference == reference)
    }
    fn current<'a>(
        &'a self,
        seats: &'a Seats,
        buildings: &'a Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<(&'a Binding, &'a TurretSide, &'a Building)> {
        let b = self.binding(reference)?;
        let t = seats
            .crews
            .turrets
            .get(b.crew)
            .filter(|t| t.turret == reference)?;
        let current = buildings.current(&b.owner)?;
        Some((b, t, buildings.get(&current)?))
    }
    pub fn vehicle_target(&self,store:&Store,seats:&Seats,buildings:&Rows<Building>,entities:&Entities,reference:Ref<Turret>)->Option<crate::ai::vehicles::world::UnitView> {
        let (b,t,building)=self.current(seats,buildings,reference)?;let target=b.target.as_ref()?;
        Some(crate::ai::vehicles::world::UnitView {obj:b.entry,id:crate::units::registry::target_id(target.member) as f64,generation:building.state.building.target_generation?,kind:8.,
            team:t.lease.and_then(|r|store.seat_lease.get(r)).map_or(target.team,|l|l.team),alive:building.state.building.alive(),position:self.position(seats,buildings,reference)?,
            class:Some(b.target_class.as_ref()?.clone()),class_name:Some(target.name.clone()),turret:entities.find(Entity::Turret(reference)),targetable:true,..Default::default()})
    }
    pub fn view(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        reference: Ref<Turret>,
    ) -> Option<TurretView> {
        let (b, t, building) = self.current(seats, buildings, reference)?;
        Some(TurretView {
            obj: entities.find(Entity::Turret(reference))?,
            entry: Some(b.entry),
            alive: building.state.building.alive(),
            gunner: t.gunner.is_some(),
            lease: t.lease.map(|r| {
                entities
                    .find(Entity::Lease(r))
                    .expect("prepared turret lease")
            }),
            facing: t.desc.facing,
            yaw_limits_setting: b.yaw.clone(),
            pitch_limits_setting: b.pitch.clone(),
            pitch_limits: Some(t.desc.pitch_limits),
            yaw_limits: t.desc.yaw_limits,
            has_hold_yaw: true,
            prepared_limits: Some(b.limits),
        })
    }
    pub fn life(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<f64> {
        self.current(seats, buildings, reference)?
            .2
            .state
            .building
            .target_generation
    }
    pub fn sphere(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<Sphere> {
        let (_, _, b) = self.current(seats, buildings, reference)?;
        let (center, radius) = b.state.building.sphere(&b.settings);
        Some(Sphere { center, radius })
    }
    pub fn position(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<Vector3> {
        let (_, _, b) = self.current(seats, buildings, reference)?;
        let mut point = Vector3::default();
        point.set_from_matrix_position(&b.settings.matrix);
        Some(point)
    }
    pub fn hold_yaw(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
        yaw: f64,
    ) -> Option<f64> {
        Some(self.current(seats, buildings, reference)?.1.hold_yaw(yaw))
    }
    pub fn forward(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<Vector3> {
        let (_, t, _) = self.current(seats, buildings, reference)?;
        Some(t.view_direction(t.yaw, t.pitch))
    }
    pub fn origin(
        &self,
        seats: &mut Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<Vector3> {
        let (b, _, _) = self.current(seats, buildings, reference)?;
        let index = b.crew;
        Some(seats.crews.turrets[index].origin())
    }
    /// objectiveCrew observes the current lease even before building life is
    /// queried. A released or respawned rider is filtered by the caller.
    pub fn crew_lease(&self,seats:&Seats,reference:Ref<Turret>)->Option<Ref<SeatLease>> {
        let b=self.binding(reference)?;
        seats.crews.turrets.get(b.crew).filter(|t|t.turret==reference)?.lease
    }
    pub fn lease_count(
        &self,
        seats: &Seats,
        buildings: &Rows<Building>,
        reference: Ref<Turret>,
    ) -> Option<usize> {
        self.current(seats, buildings, reference)?;
        Some(1)
    }
    pub fn lease(
        &self,
        store: &Store,
        seats: &Seats,
        buildings: &Rows<Building>,
        entities: &Entities,
        owners: &dyn super::combat::ControlOwners,
        reference: Ref<Turret>,
    ) -> Option<Option<LeaseView>> {
        let (_, t, _) = self.current(seats, buildings, reference)?;
        match t.lease {
            Some(r) => Some(Some(lease_view(store, entities, owners, r)?)),
            None => Some(None),
        }
    }
}
fn lease_view(
    store: &Store,
    entities: &Entities,
    owners: &dyn super::combat::ControlOwners,
    r: Ref<SeatLease>,
) -> Option<LeaseView> {
    let l = store.seat_lease.get(r)?;
    Some(LeaseView {
        obj: entities.find(Entity::Lease(r))?,
        human: l.human,
        released: l.released,
        live_false: !crate::seats::lease::live(l, r, &store.unit),
        controllable_controller: l.controllable.and_then(|r| owners.controller(r)),
        unit_controller: l
            .unit
            .and_then(|r| store.unit.get(r))
            .and_then(|u| u.controller)
            .map(|r| {
                entities
                    .find(Entity::Controller(r))
                    .expect("prepared turret rider controller")
            }),
    })
}

impl super::combat::ControlOwners for Turrets {
    fn controller(&self, reference: Ref<crate::gen::SeatUnit>) -> Option<Obj> {
        self.control(reference)?.controller
    }
}
