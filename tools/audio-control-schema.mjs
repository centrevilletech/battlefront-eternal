// Sound controls exchange numbers once per frame. Field order and strides
// are generated together so a new game fact cannot silently shift one side.
import { readFileSync, writeFileSync } from "node:fs";
export const rows = {
  CONFIG: "master_bus pause_bus own_scope_in own_scope_out victory1 victory2 defeat1 defeat2 bounds1 bounds2 bleed11 bleed12 bleed21 bleed22 space_send tow_attach tow_release sample_base device_memory",
  FRAME: "dt wall_now now rate enhanced light hrtf binaural lfe running max_voices virtual_voices px py pz fx fy fz ux uy uz own team over tickets1 tickets2 bleed1 bleed2 human effects_volume ambience_volume music_volume voice_volume interface_volume water_level world collision channels configuration",
  SOUND: "kind bus pitch pitch_dev gain gain_dev reverb loop pan mode3d doppler bias priority min_distance mute_distance max_distance roll_off roll_in_distance probability interval interval_dev random_position cycle stream first count",
  ENHANCED_SAMPLE: "id render_id duration rate scale available",
  SAMPLE: "render_id duration rate scale available",
  ENTRY: "id weight interval interval_dev",
  BUS: "gain parent target amount lead_in lead_out",
  EMITTER: "id kind class x y z vx vy vz health max_health alive removed mode air posture dive grounded foot0 foot1 foot2 shield selected0 selected1 zoom zoom_step first_person mounted firing_primary firing_secondary generation flight piloted speed_limit collision_drop life countdown salvo flying walk_phase walk_blend",
  WEAPON: "id owner class selected reloading overheated charging firing heat threshold held max_pressed wait shot_delay empty launcher strike zoom_first_person ordinal",
  POST: "id class x y z active team contested progress neutralize building_alive ambient1 ambient2 info11 info12 info21 info22 capture11 capture12 capture21 capture22 lost11 lost12 lost21 lost22 present",
  ITEM: "id class x y z mine landed gone life span present mine_base landing_source",
  TURRET: "id class owner x y z held alive yaw pitch yaw_top pitch_top present",
  EVENT: "kind owner weapon class sound x y z amount health value seconds positional placed",
  READY: "voice generation part sample duration channels time failed wall_now",
  CLASS: "fire reload overheat charge fire_loop empty change jump land roll squat prone stand rustle hurt death engine turn_on turning_off turn_off takeoff activate deactivate yaw pitch impact bounce splash explosion powerup ordnance ambient ambient2 tick hydraulic step0 step1 step2 step3 step4 step5 dk_step0 dk_step1 dk_step2 weapon_left weapon_right ambient_rep ambient_cis ambient_all ambient_imp overheat_pitch charge_pitch fire_stop tick_pitch turn_off_time footstep_stride yaw_pitch pitch_pitch collision discharge dispute capture lost tow_attach tow_release",
  LAYER: "parent sound delay deviation binding_first binding_count",
  BINDING: "input output first count",
  POINT: "x y",
  STREAM: "slots channels",
  SEGMENT: "stream sample duration channels stereo surround6 surround8 first",
  TRACK: "stream min_play min_idle priority fade_in fade_out",
  MUSIC_SLOT: "team stage track amount fraction start",
  REGION: "sound x y z min_distance max_distance size_x size_y size_z scale",
  FOLEY: "class material left right jump bodyfall land roll squat prone stand",
  FOOT: "id owner index m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11 m12 m13 m14 m15 present",
  PACKET: "kind count",
  BODY: "id enabled",
  COUNT: "post team value",
  MATERIAL: "min_x min_y min_z max_x max_y max_z group",
  WATER: "cell grid offset_x offset_z",
  PROPERTY: "class key sound",
  NAME: "key sound",
  ROOM: "room room_hf decay hf_ratio reflections refl_delay reverb reverb_delay diffusion density hf_reference mask",
  FOOT_DEF: "class index x y z length detection threshold hydraulic centred rate first count",
  PLANE: "nx ny nz distance",
  INFO: "commands placements requests real virtual started stolen dropped culled seed time room_changed room room_hf decay hf_ratio reflections refl_delay reverb reverb_delay diffusion density hf_reference accepted error debug_count",
  REQUEST: "voice generation segment part sample operation variant",
  DEBUG_VOICE: "sound category mode3d distance audible score real occluded elapsed",
};
export const enums = {
  REQUEST_KIND: { LOAD: 0, RELEASE: 1, EVICT: 2 },
  INPUT: { FRAME: 1, EMITTER: 2, WEAPON: 3, POST: 4, ITEM: 5, TURRET: 6, FOOT: 7, EVENT: 8, BODY: 9, COUNT: 10 },
  IMPORT: { SAMPLE: 1, BUS: 2, ENTRY: 3, SOUND: 4, CLASS: 5, LAYER: 6, BINDING: 7, POINT: 8, STREAM: 9, SEGMENT: 10, TRACK: 11, MUSIC_SLOT: 12, REGION: 13, STATIC_EMITTER: 14, FOLEY: 15, MATERIAL: 16, WATER: 17, PROPERTY: 18, NAME: 19, EFFECT: 20, CONFIG: 21, ROOM: 22, FOOT_DEF: 23, HEIGHT: 24, PLANE: 25, BUS_NAME: 26, ENHANCED_SAMPLE: 27 },
  EVENT_KIND: { FIRE: 1, ACTOR_FIRE: 2, HURT: 3, DEATH: 4, IMPACT: 5, BOUNCE: 6, SPLASH: 7, EXPLOSION: 8, SOUND: 9, POWERUP: 10, TURRET_FIRE: 11, INTERFACE: 12, BUS_FADE: 13, CHATTER: 14, POST: 15, LEAVING: 16, PAUSE: 17, TOW_ATTACH: 18, TOW_RELEASE: 19, END: 20, RETARGET: 21, START: 22, ACTOR_HURT: 23, ACTOR_DEATH: 24, TEST_CHANNEL: 25, TEST_AROUND: 26 },
  EMITTER_KIND: { ABSENT: 0, SOLDIER: 1, DROIDEKA: 2, HULL: 3, ROUND: 4, STATION: 5, DROID: 6, EVENT_ACTOR: 7 },
};
export function main(args = []) {
  const banner = "// Generated by tools/audio-control-schema.mjs; do not edit.\n";
  let js = banner, rust = banner;
  for (const [name, fields] of Object.entries(rows)) {
    const names = fields.split(" ");
    if (names.includes("stride")) throw Error(`reserved sound layout field: ${name}.stride`);
    const layout = Object.fromEntries(names.map((field, index) => [field.toUpperCase(), index]));
    layout.STRIDE = names.length;
    js += `export const ${name} = ${JSON.stringify(layout)};\n`;
    rust += `pub mod ${name.toLowerCase()} {\n`;
    for (const [field, index] of Object.entries(layout)) rust += `    pub const ${field}: usize = ${index};\n`;
    rust += "}\n";
  }
  for (const [name, values] of Object.entries(enums)) {
    js += `export const ${name} = ${JSON.stringify(values)};\n`;
    rust += `pub mod ${name.toLowerCase()} {\n` + Object.entries(values).map(([key, value]) => `    pub const ${key}: usize = ${value};`).join("\n") + "\n}\n";
  }
  for (const [path, contents] of [["src/audio/control-layout.js", js], ["rust/audio-control/src/layout.rs", rust]]) {
    const url = new URL("../" + path, import.meta.url);
    if (args.includes("--check")) { if (readFileSync(url, "utf8") !== contents) throw Error(`stale sound control layout: ${path}`); }
    else writeFileSync(url, contents);
  }
}
