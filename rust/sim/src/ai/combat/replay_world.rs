//! Foreign calls served in order, with their arguments and writable state
//! checked before applying the fields the foreign implementation owns.
use super::*;

impl Host<'_> {
    fn effects(&mut self, call: &Call) {
        let before = call.extra().get("before");
        let after = call.extra().get("after");
        assert!(!before.is_nullish(), "{} needs the pre-call contract", call.name);
        let pattern = matches!(call.name.as_str(), "vehicles.enterPattern" | "vehicles.exitPattern" | "vehicles.tickPattern");
        let dispatch = call.name == "vehicles.dispatch";
        let arsenal = dispatch || call.name == "arsenal.select";
        let helper = call.name.starts_with("helper.") || call.name.starts_with("nav.jetHints.") || pattern || call.name == "vehicles.sidestep";
        for v in before.get("ctls").items() {
            let want = self.read.ctl(v);
            equal(&format!("{} controller before", call.name), &self.ctls[&want.obj], &want);
        }
        for v in before.get("units").items() { self.add_unit(v); }
        for v in before.get("agents").items() {
            let want = self.read.agent(v);
            let got = &self.agents[&want.obj];
            if pattern || call.name == "interrupts.tick" { equal(&format!("{} agent before", call.name), got, &want); }
            else { equal(&format!("{} helper before", call.name), &got.helper, &want.helper); }
        }
        for v in after.get("ctls").items() {
            let want = self.read.ctl(v);
            let mut got = self.ctls[&want.obj].clone();
            // Vehicle pattern face() calls Agent.aim; the aircraft hook
            // also raises fireA. No other perception/aim state is restored.
            if pattern {
                got.fire_a = want.fire_a; got.stance = want.stance; got.nav_stance = want.nav_stance;
                got.aim_state = want.aim_state; got.aim_point = want.aim_point; got.aim_target = want.aim_target;
            }
            // Vehicle dispatch can call switchWeapon. Its fire reset and
            // channel are the same callback surface as the JS dispatcher.
            if dispatch {
                if let (Some(g), Some(w)) = (&mut got.perception, &want.perception) {
                    g.fire.channel = w.fire.channel; g.fire.sniper_active = w.fire.sniper_active;
                    g.fire.last_release = w.fire.last_release; g.fire.charging = w.fire.charging; g.fire.burst.state = w.fire.burst.state;
                }
            }
            if call.name == "nav.jetHints.tick" { got.jet = want.jet; }
            equal(&format!("{} controller after", call.name), &got, &want);
            self.ctls.insert(got.obj, got);
        }
        for v in after.get("units").items() {
            let mut want = self.read.unit(v);
            let mut got = self.units[&want.obj].clone();
            if v.get("arsenal").str() == Some("omitted") { want.arsenal = got.arsenal.clone(); }
            if arsenal {
                if let (Some(g), Some(w)) = (&mut got.arsenal, &want.arsenal) {
                    g.selected = w.selected.clone();
                    assert_eq!(g.weapons.len(), w.weapons.len());
                    for (g,w) in g.weapons.iter_mut().zip(&w.weapons) {
                        g.def = w.def.clone(); g.reloading = w.reloading; g.charging = w.charging; g.held = w.held;
                    }
                }
            }
            equal(&format!("{} unit after", call.name), &got, &want);
            self.units.insert(got.obj, got);
        }
        for v in after.get("agents").items() {
            let want = self.read.agent(v);
            let mut got = self.agents[&want.obj].clone();
            if helper { got.helper = want.helper.clone(); }
            if pattern {
                // setDelay/defaultDelay only write the countdown fields.
                if let (Some(g), Some(w)) = (&mut got.combat, &want.combat) {
                    g.tracker.delay = w.tracker.delay; g.tracker.counting = w.tracker.counting; g.tracker.deadline = w.tracker.deadline;
                }
            }
            if pattern || call.name == "interrupts.tick" { equal(&format!("{} agent after", call.name), &got, &want); }
            self.agents.insert(got.obj, got);
        }
    }
    fn helper_call(&mut self, name: &str, agent: Obj) -> &Call {
        let call = self.call(name);
        let helper = self.agents[&agent].helper.as_ref().expect("agent helper").obj;
        self.check_obj(call.arg(0), Some(helper));
        call
    }
    fn pattern_call(&mut self, name: &str, agent: Obj) -> &Call {
        let call = self.call(name);
        let pattern = self.agents[&agent].patterns_obj.expect("captured pattern identity");
        self.check_obj(call.arg(0), Some(pattern));
        call
    }
    fn no_nav(&self, name: &str) -> Option<bool> { assert!(!self.bound.get(name).bool(), "uncaptured navigation extension {name}"); None }
}

impl Draws for Host<'_> {
    fn uniform_a(&mut self) -> f64 { ((self.log.draw("A").unwrap() as u32) >> 9) as f64 / 8388608.0 }
    fn uniform_b(&mut self) -> f64 { ((self.log.draw("B").unwrap() as u32) >> 9) as f64 / 8388608.0 }
    fn uniform_c(&mut self) -> f64 { ((self.log.draw("C").unwrap() as u32) >> 9) as f64 / 8388608.0 }
}

impl World for Host<'_> {
    fn time(&self) -> f64 { self.time }
    fn difficulty(&self) -> DifficultyView { self.difficulty }
    fn mission_stay_in_turrets(&self) -> bool { self.bound.get("mission").get("stayInTurrets").bool() }
    fn mission_no_jet_obstacle_test(&self) -> bool { self.bound.get("mission").get("noJetObstacleTest").bool() }
    fn ai_stay_in_turrets(&mut self) -> bool { self.call("ai.stayInTurrets").result().bool() }
    fn unit(&self, u: Obj) -> &UnitView { self.units.get(&u).unwrap_or_else(|| panic!("unit {u} has no captured view")) }
    fn unit_mut(&mut self, u: Obj) -> &mut UnitView { self.units.get_mut(&u).unwrap_or_else(|| panic!("unit {u} has no captured view")) }
    fn origin(&mut self, u: Obj) -> Vector3 { let call = self.call("unit.origin"); self.check_obj(call.arg(0), Some(u)); vec(call.out(1)) }
    fn forward(&mut self, u: Obj) -> Vector3 { let call = self.call("unit.forward"); self.check_obj(call.arg(0), Some(u)); vec(call.out(1)) }
    fn aim_point(&mut self, u: Obj) -> Vector3 { let call = self.call("unit.aimPoint"); self.check_obj(call.arg(0), Some(u)); vec(call.out(1)) }
    fn sphere(&mut self, u: Obj) -> Sphere { let call = self.call("unit.sphere"); self.check_obj(call.arg(0), Some(u)); Sphere { center: vec(call.result().get("center")), radius: call.result().get("radius").num() } }
    fn set_intent_buttons(&mut self, u: Obj, b0: f64, b1: f64) { self.writes.get_mut(&u).unwrap().buttons = Some([b0,b1]); }
    fn set_intent_aim(&mut self, u: Obj, aim: Option<Vector3>) { self.writes.get_mut(&u).unwrap().aim = aim; }
    fn arsenal_select(&mut self, u: Obj, index: usize) -> bool {
        if self.unit(u).arsenal.as_ref().unwrap().kind == ArsenalKind::Seat {
            // seatArsenal.select is local and always acknowledges its member.
            return self.unit(u).arsenal.as_ref().unwrap().weapons.get(index).is_some();
        }
        let call = self.call("arsenal.select");
        self.check_obj(call.arg(0), Some(self.writes[&u].weapons[index]));
        self.effects(call); call.result().bool()
    }
    fn use_classic_random(&mut self, u: Obj) { for b in &mut self.writes.get_mut(&u).unwrap().random { *b = true; } }
    fn query(&mut self, center: &Vector3, radius: f64, filter: QueryFilter) -> Vec<Obj> {
        let call = self.call("units.query"); self.vector(call.arg(0), center); self.num(call.arg(1), radius);
        for v in call.extra().get("tried").items() {
            let u = self.read.obj(v.get("unit"));
            let answer = filter.keeps(u, v.get("team").num(), &mut |a,b| self.relation(a,b));
            self.boolean(v.get("answer"), answer);
        }
        for v in call.extra().get("views").items() { self.add_unit(v); }
        for v in call.extra().get("ctls").items() {
            let c = self.read.ctl(v);
            if let Some(old) = self.ctls.get(&c.obj) { equal("queried controller", old, &c); }
            else { self.ctls.insert(c.obj, c); }
        }
        call.result().items().map(|v| self.read.obj(v)).collect()
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        if a == b { return if a != 0.0 { 1.0 } else { 0.0 }; }
        if a == 0.0 || b == 0.0 { return 0.0; }
        let (a,b) = if a < b { (a,b) } else { (b,a) };
        let key = format!("{}:{}", jsrt::numfmt::number_to_string(a), jsrt::numfmt::number_to_string(b));
        for pair in self.teams.get("alliances").items() { if pair.at(0).str() == Some(&key) { return pair.at(1).num(); } }
        self.teams.get("relations").at(a as usize).at(b as usize).opt_num().unwrap_or(-1.0)
    }
    fn living(&mut self) -> Vec<Obj> { self.living.clone() }
    fn los(&mut self, from: &Vector3, to: &Vector3, a: Option<Obj>, b: Option<Obj>) -> bool {
        let call = self.call("world.los"); self.vector(call.arg(0), from); self.vector(call.arg(1), to); self.check_obj(call.arg(2), a); self.check_obj(call.arg(3), b); call.result().bool()
    }
    fn curves_for(&mut self, team: f64) -> Option<Curves> { let c = self.call("ai.curvesFor"); self.num(c.arg(0),team); curves(c.result()) }
    fn character_team(&self, c: Obj) -> f64 { self.characters[&c].2 }
    fn ctl(&self, c: Obj) -> &Ctl { self.ctls.get(&c).unwrap_or_else(|| panic!("controller {c} has no captured view")) }
    fn ctl_mut(&mut self, c: Obj) -> &mut Ctl { self.ctls.get_mut(&c).unwrap_or_else(|| panic!("controller {c} has no captured view")) }
    fn agent_controller(&self, a: Obj) -> Obj { self.agents[&a].controller }
    fn tracker(&mut self, a: Obj) -> Option<&mut TargetTracker> { let a=self.agents.get_mut(&a).unwrap(); match &mut a.combat { Some(c)=>Some(&mut c.tracker),None=>a.tracker.as_mut() } }
    fn is_controller(&self, c: Obj) -> bool { self.ctls.contains_key(&c) }
    fn agent_accepts_alert(&mut self, a: Obj, known: bool, point: &Vector3) -> bool {
        let c = self.call("agent.acceptsAlert"); self.check_obj(c.arg(0),Some(a)); self.boolean(c.arg(1),known); self.vector(c.arg(2),point); c.result().bool()
    }
    fn agent_accepts_hazard(&mut self, a: Obj) -> bool { let c = self.call("agent.acceptsHazard"); self.check_obj(c.arg(0),Some(a)); c.result().bool() }
    fn agent_accepts_acoustic(&mut self, a: Obj, source: Obj) -> bool { let c = self.call("agent.acceptsAcoustic"); self.check_obj(c.arg(0),Some(a)); self.check_obj(c.arg(1),Some(source)); c.result().bool() }
    fn shared_target(&mut self, c: Obj) -> Option<Obj> {
        if !self.bound.get("sharedTarget").bool() { return None; }
        let call = self.call("vehicles.sharedTarget"); self.check_obj(call.arg(0),Some(c));
        if !call.extra().get("view").is_nullish() { self.add_unit(call.extra().get("view")); }
        self.read.opt_obj(call.result())
    }
    fn has_vehicle_dispatch(&self) -> bool { self.bound.get("dispatchHook").bool() }
    fn vehicle_dispatch(&mut self, c: Obj, u: Obj, target: Obj, switch: bool) -> Engagement {
        let call = self.call("vehicles.dispatch"); self.check_obj(call.arg(0),Some(c)); self.check_obj(call.arg(1),Some(u)); self.check_obj(call.arg(2),Some(target)); self.boolean(call.arg(3),switch);
        self.effects(call);
        let out = call.out(4);
        let weapon = self.read.opt_obj(out.get("weapon")).and_then(|w| self.writes[&u].weapons.iter().position(|id| *id == w));
        Engagement { pattern: out.get("pattern").num(), probability: out.get("probability").num(), aux: out.get("aux").bool(), action: out.get("action").num(), in_band: out.get("inBand").bool(), weapon }
    }
    fn has_mount_reach(&self) -> bool { self.bound.get("mountReach").bool() }
    fn mount_reach(&mut self, u: Obj, point: &Vector3) -> bool { let c=self.call("vehicles.mountReach"); self.check_obj(c.arg(0),Some(u)); self.vector(c.arg(1),point); c.result().bool() }
    fn current_controllable(&mut self, character: Obj) -> (bool,Option<Obj>) {
        if !self.bound.get("currentControllable").bool() { return (false,None); }
        let c=self.call("vehicles.currentControllable"); self.check_obj(c.arg(0),Some(character));
        if !c.extra().get("view").is_nullish() { self.add_unit(c.extra().get("view")); }
        (true,self.read.opt_obj(c.result()))
    }
    fn character_remote(&self, c: Obj) -> Option<Obj> { self.characters[&c].1 }
    fn character_unit(&self, c: Obj) -> Option<Obj> { self.characters[&c].0 }
    fn is_unit(&self, o: Obj) -> bool { self.units.contains_key(&o) }
    fn enter_pattern(&mut self, a: Obj, state: f64) { let c=self.pattern_call("vehicles.enterPattern",a).clone(); self.num(c.arg(1),state); self.effects(&c); }
    fn exit_pattern(&mut self, a: Obj, state: f64) { let c=self.pattern_call("vehicles.exitPattern",a).clone(); self.num(c.arg(1),state); self.effects(&c); }
    fn tick_pattern(&mut self, a: Obj, state: f64, dt: f64) { let c=self.pattern_call("vehicles.tickPattern",a).clone(); self.num(c.arg(1),state); self.num(c.arg(2),dt); self.effects(&c); }
    fn sidestep(&mut self, a: Obj) { let c=self.pattern_call("vehicles.sidestep",a).clone(); self.effects(&c); }
    fn cover_hints(&mut self, center: &Vector3, half: f64) -> Vec<HintView> {
        if !self.bound.get("hintsQuery").bool() { assert!(!self.bound.get("queryHints").bool(),"queryHints fallback contract is missing"); return Vec::new(); }
        let c=self.call("nav.hints.query"); self.num(c.arg(1),4.0); self.vector(c.arg(2),center); self.num(c.arg(3),half); self.num(c.arg(4),30.0);
        c.extra().get("hints").items().map(|h| self.read.hint(h).unwrap()).collect()
    }
    fn has_jet_hints(&self) -> bool { self.bound.get("jetHints").bool() }
    fn jet_hints_excluded(&mut self, controller: Obj) -> bool { let c=self.call("nav.jetHints.excluded"); self.check_obj(c.arg(0),Some(controller)); c.result().bool() }
    fn jet_hints_find(&mut self, controller: Obj) -> Option<Obj> { let c=self.call("nav.jetHints.find"); self.check_obj(c.arg(0),Some(controller)); self.effects(c); let i=c.result().num(); (i>=0.0).then(|| Read::hint_obj(i)) }
    fn jet_hints_enter(&mut self, controller: Obj, hint: Option<Obj>) { let c=self.call("nav.jetHints.enter"); self.check_obj(c.arg(0),Some(controller)); self.num(c.arg(1),hint.map_or(-1.0,|h|(h & !(1<<63)) as f64)); self.effects(c); }
    fn jet_hints_exit(&mut self, controller: Obj, hint: Option<Obj>) { let c=self.call("nav.jetHints.exit"); self.check_obj(c.arg(0),Some(controller)); self.num(c.arg(1),hint.map_or(-1.0,|h|(h & !(1<<63)) as f64)); self.effects(c); }
    fn jet_hints_tick(&mut self, controller: Obj, hint: Option<Obj>, dt: f64) -> bool { let c=self.call("nav.jetHints.tick"); self.check_obj(c.arg(0),Some(controller)); self.num(c.arg(1),hint.map_or(-1.0,|h|(h & !(1<<63)) as f64)); self.num(c.arg(2),dt); self.effects(c); c.result().opt_bool().unwrap_or(true) }
    fn nav_point_clear(&mut self, _: &Vector3, _: Obj) -> Option<bool> { self.no_nav("pointClear") }
    fn nav_segment_blocked(&mut self, _: &Vector3, _: &Vector3, _: Obj) -> Option<bool> { self.no_nav("segmentBlocked") }
    fn nav_direct_clear(&mut self, _: Obj, _: &Vector3) -> Option<bool> { self.no_nav("directClear") }
    fn nav_in_danger_region(&mut self, _: Obj) -> Option<bool> { self.no_nav("inDangerRegion") }
    fn agent_sync(&mut self, a: &CombatAgent) { let v=self.agents.get_mut(&a.obj).unwrap(); v.combat=Some(a.clone()); }
    fn agent_resync(&mut self, a: &mut CombatAgent) { *a=self.agents[&a.obj].combat.clone().unwrap(); }
    fn interrupts_tick(&mut self, _: Obj) -> bool { let c=self.call("interrupts.tick"); self.effects(c); c.result().bool() }
    fn agent_board_policy(&mut self, _: Obj, candidate: Obj) -> bool { let c=self.call("agent.boardPolicy"); self.check_obj(c.arg(0),Some(candidate)); c.result().bool() }
    fn has_helper(&self, a: Obj) -> bool { self.agents[&a].helper.is_some() }
    fn helper_arrived(&mut self, a: Obj) -> bool { self.agents[&a].helper.as_ref().unwrap().arrived }
    fn helper_flying(&mut self, a: Obj) -> bool { self.agents[&a].helper.as_ref().unwrap().flying }
    fn helper_has_follow(&self, a: Obj) -> bool { self.agents[&a].helper.as_ref().is_some_and(|h| h.follow) }
    fn helper_goto(&mut self, a: Obj, point: &Vector3, mode: Option<GotoMode>) {
        let c=self.helper_call("helper.goto",a).clone(); self.vector(c.arg(1),point);
        match mode { None => assert!(c.arg(2).is_nullish()), Some(GotoMode::Direct) => assert_eq!(c.arg(2).get("mode").str(),Some("direct")), Some(GotoMode::Route) => assert_eq!(c.arg(2).get("mode").str(),Some("route")) }
        self.effects(&c);
    }
    fn helper_stop(&mut self, a: Obj) { let c=self.helper_call("helper.stop",a).clone(); self.effects(&c); }
    fn helper_tick(&mut self, a: Obj, dt: f64) { let c=self.helper_call("helper.tick",a).clone(); self.num(c.arg(1),dt); self.effects(&c); }
    fn helper_follow(&mut self, a: Obj, target: Obj) { let c=self.helper_call("helper.follow",a).clone(); self.check_obj(c.arg(1),Some(target)); self.num(c.arg(2).get("mode"),0.0); self.num(c.arg(2).get("follow"),1.0); self.effects(&c); }
    fn helper_terminate(&mut self, a: Obj) { let name=if self.agents[&a].helper.as_ref().unwrap().terminate { "helper.terminate" } else { "helper.stop" }; let c=self.helper_call(name,a).clone(); self.effects(&c); }
    fn request_jet(&mut self, c: Obj, vector: &Vector3) { self.ctl_mut(c).jet=JetRequest { pending:true, vector:*vector }; }
    fn emit_vo(&mut self, u: Obj, event: &str, property: &str) {
        let c=self.call("events.emit"); assert_eq!(c.arg(0).str(),Some("vo")); let fields=c.arg(1);
        self.check_obj(fields.get("source").get("unit"),Some(u)); self.num(fields.get("source").get("generation"),self.unit(u).generation);
        self.num(fields.get("team"),self.unit(u).team); self.vector(fields.get("position"),&self.unit(u).position); self.boolean(fields.get("human"),self.unit(u).human);
        assert_eq!(fields.get("event").str(),Some(event)); assert_eq!(fields.get("property").str(),Some(property));
    }
}
