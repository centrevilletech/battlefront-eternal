//! Capture views decoded without consulting frame-end state.
use super::*;

pub(super) fn vec(v: V) -> Vector3 {
    if let Some(a) = v.vec3() { return Vector3 { x: a[0], y: a[1], z: a[2] }; }
    Vector3 { x: v.get("x").num(), y: v.get("y").num(), z: v.get("z").num() }
}
pub(super) fn opt_vec(v: V) -> Option<Vector3> { (!v.is_nullish()).then(|| vec(v)) }
pub(super) fn curves(v: V) -> Option<Curves> {
    (!v.is_nullish()).then(|| Curves { a: v.get("A").num(), d: v.get("D").num(), b: v.get("B").opt_num(), s: v.get("S").opt_num() })
}
pub(super) fn difficulty(v: V) -> DifficultyView {
    DifficultyView { present: v.get("present").bool(), enemy_skill: v.get("enemySkill").opt_num(), hard: v.get("hard").opt_bool(), tier: v.get("tier").opt_num(),
        danger_row: (!v.get("dangerRow").is_nullish()).then(|| (v.get("dangerRow").at(0).num(), v.get("dangerRow").at(1).num())) }
}
pub(super) fn cluster(v: V) -> ClusterState { ClusterState { scan_at: v.get("scanAt").num(), flagged_at: v.get("flaggedAt").num(), generation: v.get("generation").num() } }
pub(super) fn danger(v: V) -> DangerState { DangerState { first_contact: v.get("firstContact").num(), last_notify: v.get("lastNotify").num() } }
pub(super) fn response(v: V) -> Option<Response> {
    (!v.is_nullish()).then(|| Response { mode: v.get("mode").num(), point: vec(v.get("point")), exit: v.get("exit").opt_bool().unwrap_or(false), alert: v.get("alert").opt_bool().unwrap_or(false) })
}

pub(super) struct Read<'a> {
    capture: &'a Capture,
    ids: HashMap<(String, String), Obj>,
    defs: HashMap<String, Rc<JsObj>>,
    pub tier: f64,
}

impl<'a> Read<'a> {
    pub fn new(capture: &'a Capture) -> Self { Self { capture, ids: HashMap::new(), defs: HashMap::new(), tier: 2.0 } }
    pub fn obj(&mut self, v: V) -> Obj { self.opt_obj(v).unwrap_or_else(|| panic!("expected object handle, got {}", v.show())) }
    pub fn opt_obj(&mut self, v: V) -> Option<Obj> {
        let (kind, key) = v.handle()?;
        let key = (kind.to_string(), key.show());
        let next = self.ids.len() as Obj + 1;
        Some(*self.ids.entry(key).or_insert(next))
    }
    pub fn hint_obj(index: f64) -> Obj { assert!(index >= 0.0); (1 << 63) | index as Obj }
    pub fn js(&mut self, v: V) -> Js {
        if let Some((kind, key)) = v.handle() {
            assert_eq!(kind, "def", "a content field unexpectedly names another live object");
            let key = key.str().unwrap().to_string();
            if let Some(def) = self.defs.get(&key) { return Js::Obj(def.clone()); }
            let record = self.capture.def_of(v).expect("captured definition");
            let obj = self.object(record);
            self.defs.insert(key, obj.clone());
            return Js::Obj(obj);
        }
        match v.value {
            Value::Undefined => Js::Undefined,
            Value::Null => Js::Null,
            Value::Bool(x) => Js::Bool(*x),
            Value::Num(x) => Js::Num(*x),
            Value::Str(x) => Js::Str(x.as_str().into()),
            Value::Node(_) if matches!(v.node(), Some(Node::Array(_))) => Js::Arr(Rc::new(v.items().map(|x| self.js(x)).collect())),
            Value::Node(_) => Js::Obj(self.object(v)),
            _ => panic!("unsupported content value {}", v.show()),
        }
    }
    pub fn object(&mut self, v: V) -> Rc<JsObj> {
        if v.handle().is_some() { let Js::Obj(o) = self.js(v) else { unreachable!() }; return o; }
        Rc::new(JsObj { fields: v.fields().map(|(k, x)| (k.into(), self.js(x))).collect(), def: None })
    }
    pub fn reference(&mut self, v: V) -> Option<Ref> { (!v.is_nullish()).then(|| Ref { obj: self.obj(v.get("unit")), generation: v.get("generation").num() }) }
    fn class(&mut self, v: V) -> Option<ClassView> {
        (!v.is_nullish()).then(|| ClassView { forward_speed: self.js(v.get("ForwardSpeed")), max_speed: self.js(v.get("MaxSpeed")), max_health: self.js(v.get("MaxHealth")),
            health_type: self.js(v.get("HealthType")), jet_jump: self.js(v.get("JetJump")), jet_fuel_min_border: self.js(v.get("JetFuelMinBorder")),
            unit_type: self.js(v.get("UnitType")), role: self.js(v.get("role")), ai_role: self.js(v.get("aiRole")) })
    }
    fn soldier(&mut self, v: V) -> Option<SoldierView> {
        (!v.is_nullish()).then(|| SoldierView { air: v.get("air").bool(), posture: self.js(v.get("posture")),
            shield: (!v.get("shield").is_nullish()).then(|| { let s = v.get("shield"); ShieldView { on: s.get("on").bool(), max: s.get("max").opt_num(), value: s.get("value").opt_num() } }),
            jet: v.get("jet").bool(), jet_jump: self.js(v.get("jetJump")), jet_min_border: self.js(v.get("jetMinBorder")), fuel: self.js(v.get("fuel")), max_speed: self.js(v.get("maxSpeed")) })
    }
    fn lease(&mut self, v: V) -> Option<LeaseView> {
        (!v.is_nullish()).then(|| LeaseView { obj: self.obj(v.get("self")), human: v.get("human").bool(), released: v.get("released").bool(), live_false: v.get("liveFalse").bool(),
            controllable_controller: self.opt_obj(v.get("controllableController")), unit_controller: self.opt_obj(v.get("unitController")) })
    }
    fn weapon(&mut self, v: V) -> WeaponView {
        WeaponView { def: self.object(v.get("def")), levels: (!v.get("levels").is_nullish()).then(|| v.get("levels").items().map(|v| self.object(v)).collect()),
            channel: self.js(v.get("channel")), clip: v.get("clip").num(), reserve: v.get("reserve").num(), rounds: self.js(v.get("rounds")), bottomless: v.get("bottomless").bool(),
            heat: v.get("heat").num(), overheated: v.get("overheated").bool(), charging: v.get("charging").bool(), held: v.get("held").num(), wait: v.get("wait").num(), limit: v.get("limit").opt_num(),
            reloading: v.get("reloading").bool(), busy: v.get("busy").bool(), spread_scale: v.get("spreadScale").num(), sniper_scale: v.get("sniperScale").opt_num(),
            yaw_spread_override: v.get("yawSpreadOverride").opt_num(), kick: v.get("kick").num(), floor: v.get("floor").num(), enhanced_floor: v.get("enhancedFloor").bool() }
    }
    fn arsenal(&mut self, v: V) -> Option<ArsenalView> {
        if v.is_nullish() || v.str() == Some("omitted") { return None; }
        Some(ArsenalView { kind: if v.get("kind").str() == Some("seat") { ArsenalKind::Seat } else { ArsenalKind::Soldier },
            weapons: v.get("weapons").items().map(|w| self.weapon(w)).collect(), channels: v.get("channels").items().map(|c| c.items().map(|i| i.num() as usize).collect()).collect(),
            selected: v.get("selected").items().map(|s| s.num()).collect() })
    }
    pub fn unit(&mut self, v: V) -> UnitView {
        UnitView { obj: self.obj(v.get("self")), id: v.get("id").num(), generation: v.get("generation").num(), alive: v.get("alive").bool(), team: v.get("team").num(), kind: v.get("kind").num(),
            human: v.get("human").bool(), hero: v.get("hero").bool(), trap: v.get("trap").bool(), targetable_false: v.get("targetableFalse").bool(), collision_category: v.get("collisionCategory").opt_num(),
            category: v.get("category").opt_num(), position: vec(v.get("position")), velocity: opt_vec(v.get("velocity")), class: self.class(v.get("class")),
            combat_max_health: self.js(v.get("combatMaxHealth")), max_health: self.js(v.get("maxHealth")), soldier: self.soldier(v.get("soldier")), arsenal: self.arsenal(v.get("arsenal")),
            has_origin: v.get("hasOrigin").bool(), controller: self.opt_obj(v.get("controller")), character: self.opt_obj(v.get("character")), pilot: v.get("pilot").bool(), vehicle: self.opt_obj(v.get("vehicle")),
            tow_on: v.get("towOn").bool(), towed: self.opt_obj(v.get("towed")), turret: self.opt_obj(v.get("turret")), building_post: self.opt_obj(v.get("buildingPost")), seat: self.lease(v.get("seat")),
            has_lease: v.get("hasLease").bool(), body_leases: (!v.get("bodyLeases").is_nullish()).then(|| v.get("bodyLeases").items().map(|l| self.lease(l)).collect()),
            intent: v.get("intent").bool(), intent_buttons: v.get("intentButtons").bool() }
    }
    pub fn unit_writes(&mut self, v: V) -> UnitWrites {
        UnitWrites { buttons: (!v.get("buttons").is_nullish()).then(|| [v.get("buttons").at(0).num(), v.get("buttons").at(1).num()]), aim: opt_vec(v.get("aim")),
            random: v.get("arsenal").get("weapons").items().map(|w| w.get("random").bool()).collect(),
            weapons: v.get("arsenal").get("weapons").items().map(|w| self.opt_obj(w.get("self")).unwrap_or(0)).collect() }
    }
    fn heap<T: Clone>(&mut self, v: V, capacity: usize, read: impl Fn(&mut Self, V) -> Option<T>) -> PriorityHeap<T> {
        let mut h = PriorityHeap::new(capacity, v.get("priority").at(0).num());
        h.count = v.get("count").num() as usize; h.dropped = v.get("dropped").num();
        for (i, p) in v.get("priority").items().enumerate() { h.priority[i] = p.num() as i32; }
        for (i, x) in v.get("items").items().enumerate() { h.items[i + 1] = read(self, x); }
        h
    }
    fn fire(&mut self, v: V) -> FireControl {
        let b = |i| { let b = v.get("buttons").at(i); Button { count: b.get("count").num(), code: b.get("code").num() } };
        let burst = v.get("burst");
        FireControl { channel: v.get("channel").num(), buttons: [b(0), b(1)], last_release: v.get("lastRelease").num(), charging: v.get("charging").bool(), hold: v.get("hold").num(),
            sniper_active: v.get("sniperActive").bool(), sniper_deadline: v.get("sniperDeadline").num(), burst: Burst { state: burst.get("state").num(), heat_limit: burst.get("heatLimit").num(),
            last_end: burst.get("lastEnd").num(), saved_ammo: burst.get("savedAmmo").num(), count: burst.get("count").num() }, weapon_target: self.reference(v.get("weaponTarget")),
            protected: v.get("protected").items().map(|r| self.reference(r).unwrap()).collect() }
    }
    pub fn perception(&mut self, v: V, tier: f64) -> Option<Perception> {
        if v.is_nullish() { return None; }
        let a = v.get("aimer"); let c = a.get("curves"); let e = v.get("enhanced");
        Some(Perception { heap: self.heap(v.get("heap"), 112, Self::reference), candidate: self.reference(v.get("candidate")), candidate_point: vec(v.get("candidatePoint")),
            aimer: Aimer { direction: vec(a.get("direction")), velocity: vec(a.get("velocity")), started: a.get("started").bool(), curves: AimCurves { a: c.get("A").num(), d: c.get("D").num(), b: c.get("B").num(), s: c.get("S").num() } },
            fire: self.fire(v.get("fire")), last_scan_count: v.get("lastScanCount").num(), enhanced: (!e.is_nullish()).then(|| EnhancedAimer { profile: profile(tier), skill: e.get("skill").num(),
                direction: vec(e.get("direction")), started: e.get("started").bool(), error: [e.get("error").at(0).num(), e.get("error").at(1).num()], bias: [e.get("bias").at(0).num(), e.get("bias").at(1).num()],
                key: e.get("key").str().map(str::to_string), last_key: e.get("lastKey").str().map(str::to_string), lost_at: e.get("lostAt").num(), acquired_at: e.get("acquiredAt").num(), ready_at: e.get("readyAt").num() }) })
    }
    pub fn ctl(&mut self, v: V) -> Ctl {
        let tier = self.tier;
        let shot = v.get("shot"); let danger = v.get("danger"); let jet = v.get("jet");
        Ctl { obj: self.obj(v.get("self")), unit: self.opt_obj(v.get("unit")), generation: v.get("generation").num(), character: self.opt_obj(v.get("character")), objective: self.opt_obj(v.get("objective")),
            perception: self.perception(v.get("perception"), tier), target: self.reference(v.get("target")), fire_a: v.get("fireA").bool(), fire_b: v.get("fireB").bool(), roll: v.get("roll").num(),
            range_scale: v.get("rangeScale").opt_num(), aim_state: v.get("aimState").num(), aim_point: vec(v.get("aimPoint")), aim_target: self.reference(v.get("aimTarget")), stance: v.get("stance").num(),
            nav_stance: v.get("navStance").opt_num(), facing: v.get("facing").bool(), wanted_mode: v.get("wantedMode").num(), dodge: v.get("dodge").opt_num(), alert_anim: v.get("alertAnim").opt_bool(),
            shot: ShotAlert { pending: shot.get("pending").bool(), known: shot.get("known").bool(), point: vec(shot.get("point")), source: self.opt_obj(shot.get("source")) },
            danger: DangerAlert { pending: danger.get("pending").bool(), point: vec(danger.get("point")) }, acoustic: v.get("acoustic").items().map(|r| self.reference(r).unwrap()).collect(), difficulty: curves(v.get("difficulty")),
            jet: if jet.is_nullish() { JetRequest::default() } else { JetRequest { pending: jet.get("pending").bool(), vector: vec(jet.get("vector")) } } }
    }
    pub fn tracker(&mut self, v: V) -> Option<TargetTracker> {
        (!v.is_nullish()).then(|| TargetTracker { controller: self.obj(v.get("controller")), target: self.reference(v.get("ref")), fresh: v.get("fresh").bool(), point: vec(v.get("point")),
            sample_time: v.get("sampleTime").num(), cost: v.get("cost").num(), delay: v.get("delay").num(), frozen: v.get("frozen").bool(), counting: v.get("counting").bool(), deadline: v.get("deadline").num() })
    }
    pub fn los(&mut self, v: V) -> DeferredLos {
        let req = |r: &mut Self, x: V| (!x.is_nullish()).then(|| LosRequest { controller: r.obj(x.get("controller")), target: r.reference(x.get("target")), generation: x.get("generation").num() });
        DeferredLos { active: self.heap(v.get("active"), 200, req), scratch: self.heap(v.get("scratch"), 200, req), overflow: v.get("overflow").num(), tested: v.get("tested").num() }
    }
    pub fn helper(&mut self, v: V) -> Option<HelperView> {
        (!v.is_nullish()).then(|| HelperView { obj: self.obj(v.get("self")), arrived: v.get("arrived").bool(), flying: v.get("flying").bool(), follow: v.get("follow").bool(), terminate: v.get("terminate").bool() })
    }
    pub fn hint(&mut self, v: V) -> Option<HintView> {
        (!v.is_nullish()).then(|| HintView { obj: Self::hint_obj(v.get("index").num()), position: vec(v.get("position")), forward: vec(v.get("forward")),
            primary_mask: v.get("primaryMask").opt_num(), secondary_mask: v.get("secondaryMask").opt_num(), side_mask: v.get("sideMask").opt_num(), siblings: v.get("siblings").items().map(|i| Self::hint_obj(i.num())).collect() })
    }
    pub fn reservation(&mut self, v: V, cx: &mut CombatAi) {
        if v.is_nullish() || v.get("reservation").is_nullish() { return; }
        let id = Self::hint_obj(v.get("index").num()); let r = v.get("reservation");
        cx.reservations.by_hint.insert(id, Reservation { owner: self.opt_obj(r.get("owner")), at: r.get("at").num() });
    }
    pub fn agent(&mut self, v: V) -> AgentView {
        let obj = self.obj(v.get("self")); let controller = self.obj(v.get("controller"));
        let tracker = self.tracker(v.get("tracker")); let helper = self.helper(v.get("helper"));
        let combat = if v.get("combat").bool() {
            let p = v.get("patterns"); let c = p.get("cover");
            let patterns = Patterns { state: p.get("state").num(), pending: p.get("pending").num(),
                cover: Cover { state: c.get("state").num(), pending: c.get("pending").num(), hint: self.hint(c.get("hint")), posture: c.get("posture").num(), deadline: c.get("deadline").num(), check_at: c.get("checkAt").num(), reselect_at: c.get("reselectAt").num() },
                probability: p.get("probability").num(), cover_retry: p.get("coverRetry").num(), prior: p.get("prior").num(), path_active: p.get("pathActive").bool(), crouch: p.get("crouch").num(), paused: p.get("paused").bool(),
                pause_until: p.get("pauseUntil").num(), toward: p.get("toward").num(), side: p.get("side").num(), band_check_at: p.get("bandCheckAt").num(), step_at: p.get("stepAt").num(), facing: p.get("facing").bool(), stepped: p.get("stepped").bool(),
                lateral_at: p.get("lateralAt").num(), lateral_facing: p.get("lateralFacing").bool(), last_path: p.get("lastPath").num(), retreat_at: p.get("retreatAt").num(), cover_hint: self.hint(p.get("coverHint")), jetting: p.get("jetting").bool(),
                jet_retry: p.get("jetRetry").num(), jet_start: p.get("jetStart").num(), jet_hint: self.hint(p.get("jetHint")).map(|h| h.obj), hint_jet_retry: p.get("hintJetRetry").num(), searched: p.get("searched").bool(), skip_fire: p.get("skipFire").bool() };
            Some(CombatAgent { obj, controller, tracker: tracker.unwrap(), patterns, state: v.get("state").num(), pending: v.get("pending").num(), point: vec(v.get("point")), hazard: vec(v.get("hazard")),
                align_at: v.get("alignAt").num(), looks: v.get("looks").num(), look_angle: v.get("lookAngle").num(), aligned: v.get("aligned").bool(), hazard_until: v.get("hazardUntil").num(), prone_until: v.get("proneUntil").num(), pattern: v.get("pattern").num(), stationary: v.get("stationary").bool() })
        } else { None };
        AgentView { obj, patterns_obj: self.opt_obj(v.get("patterns").get("self")), controller, tracker, helper, combat }
    }
    pub fn goal(&mut self, v: V) -> Goal { Goal { mode: v.get("mode").opt_num(), point: opt_vec(v.get("point")), ref_a: self.reference(v.get("refA")) } }
    pub fn event(&mut self, v: V) -> CombatEvent {
        let er = |r: &mut Self, x| r.reference(x).map_or(EventRef::None, EventRef::Ref);
        if v.get("kind").str() == Some("shot") { CombatEvent::Shot { source: er(self, v.get("source")), target: er(self, v.get("target")) } }
        else { assert_eq!(v.get("kind").str(), Some("damage")); CombatEvent::Damage { target: er(self, v.get("target")), attacker: er(self, v.get("attacker")), amount: v.get("amount").opt_num(), generic: v.get("generic").opt_num(), typed: v.get("typed").opt_num(), known: v.get("known").bool(), point: opt_vec(v.get("point")) } }
    }
}
