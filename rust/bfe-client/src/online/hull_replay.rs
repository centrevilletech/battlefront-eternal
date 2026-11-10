//! Detached hull sessions join ClientPrediction without creating a World.
//! Cold constructors and collision shapes are shared with authority; frames
//! lend correction bytes and pending input to those prepared private owners.
use super::{input::Sender, receiver::Receiver};
use bfe_content::install::CollisionKernel;
use bfe_content_native::BattleContent;
use bfe_sim::{
    client::{hulls::Session, ClientPrediction},
    collision::CollisionOwner,
    flight::{config::Tunings, Environment},
    gen::{ClientFrame, ClientHull, ClientHullSeat, ClientHullState, ClientMode, ClientResult},
    store::{Ref, Strings},
    vehicles::{
        build::{build, HullSource},
        native::{Collider, Collision, MapRules},
    },
};
use three::Matrix4;
// A private hull session contains the same movement owner used by the page.
// It has class constants, collision services and a mutable continuation, but
// no unit registry, spawning owner or authoritative battle scheduler.
// Restoring a checkpoint always replaces its authority baseline.
struct Hull {
    owner: ClientPrediction,
    arsenal: super::checkpoint::Mounted,
}
pub struct Fleet {
    world: u32,pub bodies:super::collision::Bodies,
    hulls: Vec<Hull>,
    classes: Vec<Option<usize>>,
    pub state: Option<ClientHullState>,
    pub identity: Option<(f64, f64)>,
    pub walk: [f64; 4],
    pub aim: [f64; 2],
    pub ticks: u64,
    active: Option<usize>,
    frozen_weapons: bool,
    water:Option<bfe_sim::units::water::WaterState>,last_water:f64,
}
impl Fleet {
    pub fn new(content: &BattleContent, collision: &mut CollisionOwner, world: u32) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let mut classes = vec![None; defs.registry.classes.len() + 1];let mut geometries=Vec::new();
        let mut hulls = Vec::new();let water=super::water::new(content);
        let height=content.content.battle.world.get("mission").and_then(|m|m.get("maxFlyHeight")).and_then(bfe_content::Value::as_f64).unwrap_or(200.);
        let mut flight=bfe_content::Object::new();flight.set(std::rc::Rc::from("ceiling"),bfe_content::Value::Number(height));
        let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];for tree in std::iter::once(&content.buildings_bvh).chain(content.terrain_bvh.iter()){for root in &tree.roots{if root.len()<6{continue;}for i in 0..3{lo[i]=lo[i].min(f32::from_bits(root[i])as f64);hi[i]=hi[i].max(f32::from_bits(root[i+3])as f64);}}}
        if lo[0].is_finite(){let mut bounds=bfe_content::Object::new();bounds.set(std::rc::Rc::from("min"),bfe_content::Value::Array(std::rc::Rc::new(vec![lo[0],-300.,lo[2]].into_iter().map(bfe_content::Value::Number).collect())));bounds.set(std::rc::Rc::from("max"),bfe_content::Value::Array(std::rc::Rc::new(vec![hi[0],height,hi[2]].into_iter().map(bfe_content::Value::Number).collect())));flight.set(std::rc::Rc::from("bounds"),bfe_content::Value::Object(std::rc::Rc::new(bounds)));}
        if let Some(authored)=content.content.battle.world.get("flight").and_then(bfe_content::Value::as_object){for(key,value)in authored.entries(){flight.set(std::rc::Rc::from(key),value.clone());}}
        let mut environment=Environment::from_content(&bfe_content::Value::Object(std::rc::Rc::new(flight)),height);environment.assists=true;
        let mut map=content.content.battle.world.as_object().ok_or("missing battle map")?.clone();if let(Some(water),Some(authored))=(&water,map.get("water").and_then(bfe_content::Value::as_object)){let mut authored=authored.clone();authored.set(std::rc::Rc::from("cells"),bfe_content::Value::Array(std::rc::Rc::new(water.cells.iter().map(|n|bfe_content::Value::Number(f64::from_bits(*n))).collect())));map.set(std::rc::Rc::from("water"),bfe_content::Value::Object(std::rc::Rc::new(authored)));}let map=bfe_content::Value::Object(std::rc::Rc::new(map));

        // ClientHull.configure resolves shapes and class constants while
        // loading. Each class owns one private solver; live wire identities
        // select it later. These query bodies stay disabled in the replica
        // collision world, preventing a second copy from blocking movement.
        for class in &defs.vehicles {
            let (Some(id), Some(template)) = (class.registry_id, class.template) else {
                continue;
            };
            let index = hulls.len();
            let t = &content.content.vehicle_templates[template];
            let geometry =
                bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&t.shape.positions), &t.shape.indices);
            let positions = collision.positions(&t.shape.positions);
            let tree = collision.tree(positions, &t.shape.indices, &geometry);
            // createBodyShape installs polygon identities as well as its BVH.
            // Rays need only triangles, but sphere gathering also groups those
            // triangles by authored polygon when the player touches a hull.
            collision.state.trees[tree as usize].polygons.extend_from_slice(&t.shape.polygons);
            let kernel = collision.body_new(tree, 1) as u32;
            let (lo, hi) = bfe_sim::vehicles::build::local_bounds(&t.shape.positions, &t.shape.indices);
            let bounds = [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]];
            geometries.push((id as usize,tree,bounds));
            let matrix = Matrix4::default().elements;
            let io = collision.io_mut();
            io[20..26].copy_from_slice(&bounds);
            io[26..42].copy_from_slice(&matrix);
            io[42..58].copy_from_slice(&matrix);
            collision.body_set(kernel);collision.enable_body(kernel,false);
            let w = &mut collision.state.worlds[world as usize];
            w.order.push(kernel);
            w.version += 1;
            let name = defs.names.text(class.name);
            let source = HullSource {
                name,
                def: &class.record,
                template: t,
                slot: index,
                slot_matrix: &matrix,
            };
            let (mut hull, _) = build(&source, index, Ref::new(index as u32, 0), index as u32);
            let mut strings = Strings::default();
            strings.intern_str("");
            if hull.flying {
                hull.flight.init(
                    bfe_sim::flight::config::flight_class(name),
                    bfe_sim::flight::Config::of(&class.record),
                    &three::Quaternion::default(),
                );
            }
            let mut row = hull.initial_record(&mut strings);
            row.name = strings.intern_str(name);
            let texts = bfe_sim::flight::store::FlightTextIds::new(&mut strings);
            hull.flight.to_store_cached(&mut row, &texts);
            let mut queries = Collision::new(world as usize, None);
            queries.add_collider(
                index as u32,
                Collider {
                    kernel,
                    local_bounds: bounds,
                    walls: vec![-1; hull.probes.len()],
                    springs: vec![],
                },
            );
            queries.prepare(&collision.state);
            let rules = MapRules::from_json(&map, content.content.collision.kill_y);
            let owner=Session::new(hull,row,strings,queries,rules,environment.clone(),Tunings::default());
            let mut prediction = ClientPrediction::default();
            prediction.install_hull(owner);
            prediction.reserve_native(4096, 32);
            let arsenal = super::checkpoint::Mounted::new(&defs, &mut prediction, Some(id as usize), &[])?;
            classes[id as usize] = Some(index);
            hulls.push(Hull {
                owner: prediction,
                arsenal,
            });
        }
        // Remote hulls contribute query geometry only. All private solvers
        // bind the same prepared collider bank, then reserve their query
        // scratch once against its final size. Adding it after the first
        // correction would allocate inside the first replayed movement step.
        let bodies=super::collision::Bodies::new(collision,world,&geometries,hulls.len()as u32);
        for hull in &mut hulls {for(body,collider)in bodies.colliders(){hull.owner.bind_native_collider(body,collider);}
            hull.owner.prepare_native_collision(&mut collision.state);
        }
        Ok(Self {
            world,bodies,
            hulls,
            classes,
            state: None,
            identity: None,
            walk: [0.; 4],
            aim: [0.; 2],
            ticks: 0,
            active: None,
            frozen_weapons: false,
            water, last_water: 0.,
        })
    }
    pub fn weapons(&self) -> impl Iterator<Item = bfe_sim::gen::Weapon> + '_ {
        let owner = self
            .identity
            .and_then(|_| self.state.as_ref())
            .and_then(|_| self.active_owner());
        let frozen = self.active.filter(|_| self.frozen_weapons && self.identity.is_some())
            .map_or(&[][..], |i| self.hulls[i].arsenal.rows.as_slice());
        let output = owner.map_or(&[][..], |o| o.output.as_slice());
        let at = if output.len() >= ClientResult::WORDS {
            ClientResult::take(output, &[]).weapons_at as usize
        } else {
            0
        };
        let count = output.get(at).copied().unwrap_or(0.) as usize;
        output
            .get(at + 1..)
            .unwrap_or(&[])
            .chunks_exact(bfe_sim::gen::Weapon::WORDS)
            .take(count)
            .map(|row| bfe_sim::gen::Weapon::take(row, &[]))
            .chain(frozen.iter().cloned())
    }
    fn active_owner(&self) -> Option<&ClientPrediction> {
        self.active.map(|i| &self.hulls[i].owner)
    }
    // ClientPrediction replays the acknowledged command tail from the exact
    // own body checkpoint. Datagram receipt ACKs are deliberately insufficient:
    // the server can have received an input it has not yet applied.
    // The displayed remote hull and private movement result remain distinct.
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        sender: &Sender,
        words: &mut Vec<f64>,
        collision: &mut CollisionOwner,
    ) -> Result<(), &'static str> {
        self.bodies.frame(receiver,collision,self.world);
        if let Some(water)=&mut self.water{water.step((receiver.clock.shown-self.last_water).max(0.)/60.);}self.last_water=receiver.clock.shown;
        self.state = None;
        self.identity = None;
        self.active = None;
        self.frozen_weapons = false;
        if receiver.own.vehicle == 0. || ![7, 10].contains(&receiver.own.body_schema) {
            return Ok(());
        }
        let Some(row) = receiver.active().find(|row| {
            row[bfe_presentation::network_layout::net_record::ID] == receiver.own.vehicle
                && row[bfe_presentation::network_layout::net_record::GENERATION] == receiver.own.vehicle_generation
        }) else {
            return Ok(());
        };
        let class = row[bfe_presentation::network_layout::net_record::CLASS_ID] as usize;
        let Some(index) = self.classes.get(class).copied().flatten() else {
            return Err("missing private hull class");
        };
        let Hull { owner, arsenal } = &mut self.hulls[index];
        words.clear();
        words.resize(ClientFrame::WORDS, 0.);
        let hull_at = owner.native_hull_frame(words, receiver.own.body_schema as u32, self.water.as_ref().map_or(f64::NAN,|w|w.level()))?;
        // Collision presence follows the current snapshot's qualified hull
        // identities. The piloted hull is excluded from its own query world;
        // other live hulls remain available to all replayed collision steps.
        // Static map geometry is shared with presentation queries.
        let flags_at = words.len();
        let flags_count = collision
            .state
            .worlds
            .get(self.world as usize)
            .map_or(0, |w| w.order.len());
        words.extend_from_slice(&self.bodies.flags);
        let correction_at = words.len();
        for bytes in receiver.body_bytes[..receiver.body_length].chunks(4) {
            let mut word = [0; 4];
            word[..bytes.len()].copy_from_slice(bytes);
            words.push(u32::from_le_bytes(word) as f64);
        }
        arsenal.restore(receiver)?;
        // Vehicle.fireSeat freezes timers at the authority flight state, even
        // when private movement predicts the following takeoff transition.
        self.frozen_weapons = receiver.own.flight.is_some_and(|state| state != bfe_sim::gen::FlightState::Flying as u8);
        let (weapons_at, weapons_words, mounts_at, mounts_count) = arsenal.write(words, !self.frozen_weapons);
        let (commands_at, commands_count) = sender.history.write(words);
        let frame = ClientFrame {
            mode: ClientMode::Hull,
            epoch: receiver.epoch,
            identity: receiver.own.id,
            generation: receiver.own.generation,
            source: receiver.own.vehicle_generation * 65536. + receiver.own.vehicle,
            authority_tick: receiver.own.tick,
            ignored: -1.,
            hull_at,
            flags_at: flags_at as u32,
            flags_count: flags_count as u32,
            correction_at: correction_at as u32,
            correction_bytes: receiver.body_length as u32,
            weapons_at,
            weapons_words,
            mounts_at,
            mounts_count,
            commands_at,
            commands_count,
            mouse_scale: 1.,
            ..Default::default()
        };
        frame.put(words, &mut Vec::new());
        owner.predict_session(words, &mut collision.state)?;
        let result = ClientResult::take(&owner.output, &[]);
        let hull = ClientHull::take(&owner.output[result.hull_at as usize..], &[]);
        self.state = Some(ClientHullState::take(&owner.output[hull.state_at as usize..], &[]));
        self.walk = [
            hull.walk_phase,
            hull.walk_blend,
            hull.walk_from_phase,
            hull.walk_from_blend,
        ];
        if hull.seats_count > 0 {
            let seat = ClientHullSeat::take(&owner.output[hull.seats_at as usize..], &[]);
            self.aim = [seat.aim_yaw, seat.pitch];
        }
        self.identity = Some((receiver.own.vehicle, receiver.own.vehicle_generation));
        self.active = Some(index);
        self.ticks += result.ticks as u64;
        Ok(())
    }
}
