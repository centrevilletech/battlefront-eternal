//! Impact-time facts for 2005 mission callbacks and scoreboard awards.
use crate::gen::{Attacker, Character, Store, Unit};
use crate::store::Ref;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Credit {
    pub character: Option<Ref<Character>>,
    pub team: i32,
    pub human: bool,
}

impl Credit {
    pub(crate) fn capture(s: &Store, local: Option<Ref<Unit>>, attacker: Attacker) -> Option<Self> {
        if let Attacker::OwnerSnapshot(r) = attacker {
            let owner = s.owner_snapshot.get(r)?;
            return Some(Self {
                character: owner.character,
                team: owner.team as i32,
                human: owner.human,
            });
        }
        let r = if let Attacker::Vehicle(r) = attacker {
            s.vehicle.get(r)?.seats.iter().find_map(|seat| {
                s.seat_lease
                    .get(seat.lease?)?
                    .unit
                    .and_then(|u| s.unit.get(u)?.character)
            })?
        } else {
            super::super::native_actions::character(s, local, attacker)?
        };
        let c = s.character.get(r)?;
        Some(Self {
            character: Some(r),
            team: c.team as i32,
            human: c.human,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destroyed {
    Soldier { human: bool, hero: bool },
    OccupiedVehicle { victim_kind: u8, killer_kind: u8 },
    EligibleEmptyVehicle,
    SelfPilotedTurret,
    Other,
}

impl Destroyed {
    pub fn points(self) -> i32 {
        match self {
            Self::Soldier { hero: true, .. } => 5,
            Self::Soldier { human: true, .. } => 2,
            Self::Soldier { .. } | Self::SelfPilotedTurret | Self::EligibleEmptyVehicle => 1,
            Self::OccupiedVehicle { victim_kind: 8, .. } => 10,
            Self::OccupiedVehicle {
                victim_kind,
                killer_kind: k @ 1..=3,
            } => {
                if victim_kind < k {
                    1
                } else {
                    2
                }
            }
            Self::OccupiedVehicle { .. } => 3,
            Self::Other => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemTransition {
    Damage,
    Kill,
    Respawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersonalScore {
    pub character: Ref<Character>,
    pub points: i32,
}

use super::events::{Event, ObjectHandle, ObjectProperty, ObjectState, RuleError, RuleResult};
use crate::world::owner::World;

impl World {
    fn require_mission(&self) -> RuleResult<()> {
        self.battle
            .rule_events
            .as_ref()
            .filter(|q| q.mission_events())
            .map(|_| ())
            .ok_or(RuleError::NotAvailable)
    }
    /// The current life of a placed, named mission object.
    pub fn mission_system(&self, name: &str) -> RuleResult<ObjectHandle> {
        self.require_mission()?;
        let h = self
            .event_rules()
            .ok_or(RuleError::NotAvailable)?
            .named_objects()
            .find(name)?;
        super::super::native_actions::current(self, &self.sim.store, h)
    }
    /// Resolve a retained goal subject to its present life at the same placement.
    pub fn current_mission_object(&self, subject: &ObjectHandle) -> RuleResult<ObjectHandle> {
        self.require_mission()?;
        super::super::native_actions::current(self, &self.sim.store, subject)
    }
    pub fn mission_system_state(&self, h: &ObjectHandle) -> RuleResult<ObjectState> {
        self.require_mission()?;
        super::super::native_actions::object_state(self, &self.sim.store, h)
    }
    pub fn mission_system_shields(&self, h: &ObjectHandle) -> RuleResult<(f64, f64, f64)> {
        self.require_mission()?;
        super::super::native_actions::shields(self, &self.sim.store, h)
    }
    /// Change a native building's ownership or shielding on its current life.
    pub fn change_mission_system(
        &mut self,
        h: &ObjectHandle,
        property: ObjectProperty,
    ) -> RuleResult<()> {
        self.mission_system_state(h)?;
        let ObjectHandle::Prop(handle) = h else {
            return Err(RuleError::NotAvailable);
        };
        match property {
            ObjectProperty::Team(n) if !(0.0..8.0).contains(&n) || n.fract() != 0.0 => {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::Team(_)
            | ObjectProperty::CurrentShield(_)
            | ObjectProperty::MaxShield(_)
            | ObjectProperty::AddShield(_) => (),
            _ => return Err(RuleError::NotAvailable),
        }
        let index = crate::units::f3_props::building_of_in(&self.units, handle)
            .ok_or(RuleError::NotAvailable)?;
        crate::units::f3_props::rule_property(self, index, property)
    }
    /// Capture the attacker while the impact and its owner are still current.
    pub fn mission_credit(&self, attacker: Attacker) -> Option<Credit> {
        Credit::capture(
            &self.sim.store,
            super::super::f4_core::local_unit(&self.battle),
            attacker,
        )
    }
    /// Submit a transition from an additional native object owner before recycling its life.
    pub fn mission_object_event(
        &mut self,
        object: ObjectHandle,
        transition: SystemTransition,
        team: i32,
        kind: Destroyed,
        credit: Option<Credit>,
    ) -> RuleResult<()> {
        self.require_mission()?;
        if !(0..8).contains(&team) {
            return Err(RuleError::InvalidValue);
        }
        if super::super::native_actions::current(self, &self.sim.store, &object)? != object {
            return Err(RuleError::StaleHandle);
        }
        self.battle
            .rule_events
            .as_mut()
            .unwrap()
            .push(Event::MissionObject {
                object,
                transition,
                team,
                kind,
                credit,
                absorbed: 0.0,
            })
            .then_some(())
            .ok_or(RuleError::LimitReached)
    }
}
