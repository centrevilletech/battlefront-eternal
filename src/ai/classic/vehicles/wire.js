// Lossless numeric boundary for vehicle views. Object tokens use the
// battle's existing AiSim identity table; strings use its shared interner.
export const shapes = {
  Dispatch: [["pattern","f64"],["probability","f64"],["aux","bool"],["action","f64"],["in_band","bool"],["weapon",["option","Obj"]]],
  "Ref": [["obj","Obj"],["generation","f64"]],
  "ClassView": [["ai_size_type",["option","String"]],["forward_speed",["option","f64"]],["strafe_speed",["option","f64"]],["turn_rate",["option","f64"]],["pitch_rate",["option","f64"]],["moving_turn_only",["option","bool"]],["max_speed",["option","f64"]],["min_speed",["option","f64"]],["max_turn_speed",["option","f64"]],["max_pitch_speed",["option","f64"]],["max_yaw_speed",["option","f64"]],["vehicle_type",["option","String"]],["landing_time",["option","f64"]],["landing_speed",["option","f64"]],["landed_height",["option","f64"]],["bounds",["option",["array",["array","f64",3],2]]],["no_enter_vehicles","f64"],["role","f64"]],
  "UnitView": [["obj","Obj"],["kind","f64"],["id","f64"],["generation","f64"],["alive","bool"],["team","f64"],["position","Vector3"],["velocity","Vector3"],["body_yaw","f64"],["class",["option","ClassView"]],["class_name",["option","String"]],["soldier","bool"],["soldier_max_speed",["option","f64"]],["walker","bool"],["human","bool"],["network_human","bool"],["targetable","bool"],["seat",["option","Obj"]],["lease",["option","Obj"]],["vehicle",["option","Obj"]],["turret",["option","Obj"]],["controller",["option","Obj"]],["character",["option","Obj"]],["up_y",["option","f64"]],["own_label",["option","String"]],["own_name",["option","String"]]],
  "SeatView": [["lease",["option","Obj"]],["weapons","usize"],["yaw_limits",["option","String"]],["pitch_limits",["option","String"]]],
  "HullView": [["obj","Obj"],["entry","Obj"],["alive","bool"],["name","String"],["flying","bool"],["walking","bool"],["grounded","bool"],["flight_state",["option","String"]],["gear","f64"],["speed","f64"],["max_speed_setting","f64"],["forward_speed_setting","f64"],["label",["option","String"]],["position","Vector3"],["velocity","Vector3"],["yaw","f64"],["quaternion","three::Quaternion"],["matrix","three::Matrix4"],["bounds_min","Vector3"],["bounds_max","Vector3"],["bound_radius","f64"],["size","Vector3"],["body",["option","BodyId"]],["seats",["list","SeatView"]],["occupants","f64"],["driver","bool"],["free_seats","f64"],["occupied","bool"],["controlling_team","f64"],["tow_target","f64"],["slot_post","bool"],["decay_time",["option","f64"]],["max_health","f64"],["own_system","bool"]],
  "TurretView": [["obj","Obj"],["entry",["option","Obj"]],["alive","bool"],["gunner","bool"],["lease",["option","Obj"]],["facing","f64"],["yaw_limits_setting",["option","String"]],["pitch_limits_setting",["option","String"]],["pitch_limits",["option",["array","f64",2]]],["yaw_limits",["option",["array","f64",2]]],["has_hold_yaw","bool"]],
  "LeaseView": [["obj","Obj"],["released","bool"],["human","bool"],["live","bool"],["unit",["option","Obj"]],["controllable",["option","Obj"]],["vehicle",["option","Obj"]],["turret",["option","Obj"]],["index","f64"],["team","f64"]],
  "ControllerView": [["obj","Obj"],["unit",["option","Obj"]],["nav_control","bool"],["executor","bool"],["agent",["option","Obj"]],["executor_status","f64"],["target","bool"],["helper","bool"],["helper_has_path","bool"],["helper_state",["option","f64"]],["helper_destination",["option","Vector3"]],["is_unit_controller","bool"],["fire_channel",["option","f64"]]],
  "Settings": [["min_fly_height","f64"],["max_fly_height","f64"],["notify_radius","f64"],["dense","bool"],["stay_in_turrets","bool"],["cycle","f64"]],
  "Request": [["valid","bool"],["steering_target","Vector3"],["waypoint","Vector3"],["segment_direction","Vector3"],["mode","f64"],["lower","f64"],["upper","f64"],["blend",["option","f64"]],["throttle",["option","f64"]]],
  "Orbit": [["target",["option","Ref"]],["offset","Vector3"],["radius","f64"],["factor","f64"]],
  "StuckAt": [["x","f64"],["z","f64"]],
  "Steer": [["moving_turn_at",["option","f64"]],["loop_",["option","bool"]],["loop_at",["option","f64"]],["min_speed",["option","f64"]],["min_speed_at",["option","f64"]],["orbit",["option",["option","Orbit"]]],["land",["option","bool"]],["path_override",["option","bool"]],["path_altitude",["option","bool"]],["throttle",["option","f64"]],["recovery",["option","f64"]],["recovery_since",["option","f64"]],["last_recovery",["option","f64"]],["saved_forward",["option","Vector3"]],["hold_since",["option","f64"]],["holding",["option","bool"]],["turn_yaw",["option","f64"]],["turn_at",["option","f64"]],["walk_since",["option","f64"]],["stuck_at",["option",["option","StuckAt"]]],["stuck_count",["option","f64"]]],
  "SeatIntent": [["up","f64"],["down","f64"],["brake","bool"],["trick","bool"],["immelmann","bool"],["boost","bool"]],
  "Stick": [["forward","f64"],["right","f64"],["turn","f64"],["pitch","f64"],["up","f64"],["down","f64"],["brake","bool"],["trick","bool"],["immelmann","bool"],["boost","bool"]],
  "PilotEnv": [["has_path","bool"],["min_fly_height","f64"],["max_fly_height","f64"]],
  "Sphere": [["center","Vector3"],["radius","f64"]],
  "Choice": [["ok","bool"],["action","f64"],["in_band","bool"],["weapon",["option","Obj"]]],
  "PostView": [["obj","Obj"],["position","Vector3"],["team","f64"]],
  "HintView": [["obj","Obj"],["position","Vector3"],["forward","Vector3"],["up",["option","Vector3"]],["command_post",["option","Obj"]]],
  "WeaponView": [["base",["option","String"]],["channel","f64"]],
  "ExitOptions": [["post",["option",["option","Obj"]]],["promote",["option","bool"]],["placement",["option","bool"]]],
  "Decay": [["idle","f64"],["sample","f64"]],
  "AnalogOptions": [["controller",["option","Obj"]],["stepped","bool"]],
  "PatrolView": [["obj","Obj"],["agent","Obj"],["controller","Obj"],["post",["option","Obj"]],["support","bool"],["helper","bool"]],
  "PatternsView": [["obj","Obj"],["agent","Obj"],["controller","Obj"],["control","Obj"],["tracker_point",["option","Vector3"]],["fresh_target",["option","Obj"]],["target",["option","Obj"]],["helper","bool"],["helper_arrived","bool"]],
  "PatternState": [["toggle","bool"],["entry","Vector3"],["destination","Vector3"],["best","f64"],["progress_at","f64"],["fallback","bool"],["loop_","bool"],["inverted","bool"],["flip_a","bool"],["flip_b","bool"]],
  "AttackRun": [["a",["option","f64"]],["b",["option","f64"]]],
  "Fsm": [["state","f64"],["pending","f64"]],
  "AgentCx": [["agent","Obj"],["controller","Obj"],["unit",["option","Obj"]]],
  "BoardAgent": [["vehicle",["option","Ref"]]],
  "FlyAgent": [["hint",["option","HintView"]],["search_at","f64"],["flag","bool"],["deadline","f64"],["post",["option","Obj"]]],
  "Reservation": [["candidate","Ref"],["owner","Ref"],["time","f64"],["kind","f64"]],
  "SupportHub": [["hub","usize"],["distance","f64"]],
  "Hit": [["distance","f64"],["point","Vector3"],["normal","Vector3"],["terrain","bool"],["body",["option","BodyId"]]]
};
export function encode(type, value, sim, out = []) {
  if (Array.isArray(type)) {
    const [kind, item, size] = type;
    if (kind === 'option') { out.push(value != null ? 1 : 0); if (value != null) encode(item, Array.isArray(item) && item[0] === 'option' ? value.some : value, sim, out); }
    else { if (kind === 'list') out.push(value.length); for (let i=0; i<(size ?? value.length); i++) encode(item,value[i],sim,out); }
  } else if (shapes[type]) { for (const [key,t] of shapes[type]) encode(t,value[key],sim,out); }
  else if (type === 'Vector3') out.push(value.x,value.y,value.z);
  else if (type === 'Quaternion') out.push(value.x,value.y,value.z,value.w);
  else if (type === 'Matrix4') out.push(...value.elements);
  else if (type === 'String') out.push(sim.bridge.strings.id(value));
  else if (type === 'bool') out.push(Number(Boolean(value)));
  else out.push(Number(value));
  return out;
}
export function decoder(values, sim) {
  let at=0;
  const read=type=>{
    if (Array.isArray(type)) {
      const [kind,item,size]=type;
      if (kind==='option') { if (!values[at++]) return null; const v=read(item); return Array.isArray(item) && item[0]==='option' ? {some:v} : v; }
      const n=size ?? values[at++]; return Array.from({length:n},()=>read(item));
    }
    if(shapes[type]) return Object.fromEntries(shapes[type].map(([k,t])=>[k,read(t)]));
    if(type==='Vector3') return {x:values[at++],y:values[at++],z:values[at++]};
    if(type==='Quaternion') return {x:values[at++],y:values[at++],z:values[at++],w:values[at++]};
    if(type==='Matrix4') return {elements:Array.from({length:16},()=>values[at++])};
    const v=values[at++]; return type==='String' ? sim.bridge.strings.at(v) : type==='bool' ? Boolean(v) : v;
  };
  return {read, done(){if(at!==values.length)throw Error(`vehicle boundary words ${at}/${values.length}`);}};
}
