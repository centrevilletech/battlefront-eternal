//! preparePhase's foreign-row count comes from the F3 construction registry,
//! never the mixed entity registry or the previous tick's stamped count.
use crate::world::owner::World;

impl World {
    /// The number `preparePhase` returns for WorldTickInput.units. Removed
    /// corpses stay in the factory but supply no input; connected humans
    /// and native controllers supply their rows through their native owners.
    /// Counting the mixed registry would also count hulls and the viewpoint.
    pub fn foreign_unit_rows(&self) -> u32 {
        if !crate::units::f3::active_in(&self.units) { return 0; }
        crate::units::f3::built_in(&self.units).iter().filter(|&&reference| {
            self.sim.store.unit.get(reference).is_some_and(|unit| {
                !unit.removed
                    && !(unit.network_human == Some(true) && self.input.players.active)
                    && !crate::units::f3::native_intent_owned_in(&self.units, reference)
            })
        }).count() as u32
    }
}

// The counter consumes membership and ownership facts independently of
// pose/combat setup. Build the owning F3 registry from those inputs here,
// keeping the archived count outside the World.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gen::Unit,store::Ref,units::f3};
    use std::io::BufRead;
    #[test]
    fn recorded_foreign_registry_words() {
        // CI also exercises the fixture reader: native intent, a removed
        // body and a human before/after the players owner becomes active.
        let file:Box<dyn BufRead>=if let Ok(path)=std::env::var("BFE_F3_REGISTRY_ORACLE") {
            Box::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        } else {
            Box::new(std::io::Cursor::new(concat!(
                r#"{"reset":true,"players":false,"changes":[{"slot":0,"generation":0,"life":0,"removed":false,"human":null,"native":false},{"slot":1,"generation":0,"life":0,"removed":false,"human":null,"native":true},{"slot":2,"generation":0,"life":0,"removed":false,"human":true,"native":false},{"slot":3,"generation":0,"life":0,"removed":true,"human":null,"native":false}],"units":"4000000000000000"}"#,"\n",
                r#"{"reset":false,"players":true,"changes":[],"units":"3ff0000000000000"}"#,"\n")))
        };
        let mut world=World::new();let mut rows=0;let mut differences=0;let mut first=None;
        for line in file.lines() {
            let frame:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
            if frame["reset"].as_bool().unwrap() {world=World::new();f3::f3_init_in(&mut world,0,f64::NAN,0);}
            world.input.players.active=frame["players"].as_bool().unwrap();
            for change in frame["changes"].as_array().unwrap() {
                let slot=u32::try_from(change["slot"].as_u64().unwrap()).unwrap();
                let generation=u32::try_from(change["generation"].as_u64().unwrap()).unwrap();
                let reference=Ref::<Unit>::new(slot,generation);
                // Registry identities come from the preceding F3 binding,
                // including slab reuse; gameplay life never replaces that key.
                if world.sim.store.unit.get(reference).is_none() {
                    f3::reserve_native_slots_in(&mut world,slot as usize+1);
                    world.sim.store.unit.insert_at(slot,generation,Unit::default());
                    f3::with_owner_rows(&mut world,|store,owner|owner.adopt(store,reference).unwrap());
                }
                let unit=world.sim.store.unit.get_mut(reference).unwrap();
                unit.generation=change["life"].as_f64().unwrap();unit.removed=change["removed"].as_bool().unwrap();
                unit.network_human=if change["human"].is_null(){None}else{Some(change["human"].as_bool().unwrap())};
                f3::native_intent_in(&mut world,reference,change["native"].as_bool().unwrap());
            }
            // Read the expected word only after applying the independent
            // membership/ownership inputs. It is never written into World.
            let recorded=u64::from_str_radix(frame["units"].as_str().unwrap(),16).unwrap();
            let computed=world.foreign_unit_rows();
            if (computed as f64).to_bits()!=recorded {
                differences+=1;
                first.get_or_insert_with(||serde_json::json!({"row":rows,"computed":computed,"recorded":frame["units"]}));
            }
            rows+=1;
        }
        println!("REGISTRY_ORACLE {}",serde_json::json!({"rows":rows,"differences":differences,"first":first}));
        assert!(rows>0);assert_eq!(differences,0);
    }
}
