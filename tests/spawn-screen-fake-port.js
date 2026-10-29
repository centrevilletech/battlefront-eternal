// A stand-in for the world's spawn port (docs/human-spawn.md "The spawn port";
// the real one is game.js spawnPort), for the spawn screen's tests and for
// showing the screen over a living player (src/spawn-screen/debug.js).
//
// FakeSpawnPort holds plain data: teams, rosters, posts (owner and whether
// each may spawn), the player's state, camera shots. Requests are recorded;
// with autoSpawn the next step() "spawns" the request (phase alive, the
// "spawned" event). livePort(bf) builds one from the running game, taking
// the real port's bodies, models and camera when it has them.

export class FakeSpawnPort {
  constructor({ teams = {}, rosters = {}, posts = [], human = {}, shots = [], commonModel = null, previewBody = null, setViewShot = null, autoSpawn = true, canSwitchTeams = true } = {}) {
    this.data = { teams, rosters, posts };
    this.human = { phase: "unspawned", team: 1, className: null, classIndex: 0, post: null, canSwitchTeams, ...human };
    this.shots = shots;
    this.requests = [];
    this.suicides = 0;
    this.viewShots = [];
    this.autoSpawn = autoSpawn;
    this.listeners = new Map();
    this.models = commonModel;
    this.bodies = previewBody;
    this.viewShot = setViewShot;
  }

  on(event, fn) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event).add(fn);
    return () => this.listeners.get(event)?.delete(fn);
  }

  emit(event, detail) {
    for (const fn of this.listeners.get(event) || []) fn(detail);
  }

  humanState() {
    return { ...this.human };
  }

  teams() {
    return this.data.teams;
  }

  roster(team) {
    return this.data.rosters[team] || [];
  }

  // A post is eligible for the team that holds it, if it has spawns.
  posts(team) {
    return this.data.posts.map((post) => ({
      name: post.name,
      displayName: post.displayName ?? post.name,
      position: post.position ?? [0, 0, 0],
      owner: post.owner ?? 0,
      eligible: (post.owner ?? 0) === Number(team) && post.spawns !== false,
      vehicles: post.vehicles?.[team] ?? [],
    }));
  }

  // Kept while eligible, else the eligible post nearest the previous one,
  // else the eligible post nearest an enemy-held post (the front line).
  choosePost(team, previous = null) {
    const posts = this.posts(team);
    const eligible = posts.filter((post) => post.eligible);
    if (!eligible.length) return null;
    const near = (to) => eligible.reduce((best, post) => {
      const d = Math.hypot(post.position[0] - to[0], post.position[2] - to[2]);
      return !best || d < best.d ? { post, d } : best;
    }, null)?.post ?? null;
    const old = posts.find((post) => post.name === previous);
    if (old?.eligible) return old.name;
    if (old) return near(old.position)?.name ?? null;
    const enemy = posts.filter((post) => post.owner && post.owner !== Number(team));
    if (!enemy.length) return null;
    let best = null;
    for (const post of eligible) {
      const d = Math.min(...enemy.map((e) => Math.hypot(e.position[0] - post.position[0], e.position[2] - post.position[2])));
      if (!best || d < best.d) best = { post, d };
    }
    return best?.post.name ?? null;
  }

  request(choice) {
    this.requests.push(choice);
  }

  suicide() {
    this.suicides += 1;
  }

  // A choice already sent, taken back before it spawned him.
  cancel() {
    this.cancels = (this.cancels || 0) + 1;
    this.requests.length = 0;
  }

  // The world's next tick: a recorded request becomes a spawn.
  step() {
    if (!this.autoSpawn || !this.requests.length || this.human.phase === "alive") return;
    const { team, className, post } = this.requests.at(-1);
    const classIndex = Math.max(this.roster(team).findIndex((entry) => entry.className === className), 0);
    Object.assign(this.human, { phase: "alive", team, className, classIndex, post });
    this.emit("spawned", this.humanState());
  }

  // He died and his four seconds are up.
  await() {
    this.human.phase = "awaiting";
    this.emit("awaitingSpawn", this.humanState());
  }

  previewBody(className) {
    return this.bodies?.(className) ?? null;
  }

  commonModel(name) {
    return this.models?.(name) ?? null;
  }

  cameraShots() {
    return this.shots;
  }

  setViewShot(shot) {
    this.viewShots.push(shot);
    this.viewShot?.(shot);
  }
}

// Data like stock Geonosis, for tests that don't load the game.
export function geonosisData() {
  const weapons = (...names) => names;
  return {
    teams: {
      1: { side: "rep", logo: "com_icon_republic", objectives: ["CAPTURE ENEMY COMMAND POSTS", "PROTECT THE AT-TE'S", "DESTROY THE TRANSPORTS"] },
      2: { side: "cis", logo: "com_icon_cis", objectives: ["CAPTURE ENEMY COMMAND POSTS", "DESTROY THE AT-TE'S", "PROTECT THE TRANSPORTS"] },
    },
    rosters: {
      1: [
        { className: "rep_inf_clone_trooper", displayName: "CLONE TROOPER", weapons: weapons("BLASTER RIFLE", "BLASTER PISTOL", "EMP GRENADE", "CONCUSSION GRENADE") },
        { className: "rep_inf_arc_trooper", displayName: "ARC TROOPER", weapons: weapons("MISSILE LAUNCHER", "BLASTER PISTOL", "THERMAL DETONATOR", "MINES") },
        { className: "rep_inf_clone_pilot", displayName: "CLONE PILOT", weapons: weapons("DN BOLT CASTER", "BLASTER PISTOL", "FUSION CUTTER", "DISPENSER HEALTH & AMMO") },
        { className: "rep_inf_clone_sharpshooter", displayName: "CLONE SHARPSHOOTER", weapons: weapons("SNIPER RIFLE", "BLASTER PISTOL", "THERMAL DETONATOR", "RECON DROID") },
        { className: "rep_inf_jet_trooper", displayName: "JET TROOPER", weapons: weapons("EMP LAUNCHER", "COMMANDO PISTOL", "THERMAL DETONATOR") },
      ],
      2: [
        { className: "cis_inf_battledroid", displayName: "SUPER BATTLE DROID", weapons: weapons("WRIST BLASTER", "TRI SHOT", "WRIST ROCKET") },
        { className: "cis_inf_assault", displayName: "ASSAULT DROID", weapons: weapons("MISSILE LAUNCHER", "BLASTER PISTOL", "THERMAL DETONATOR", "MINES") },
        { className: "cis_inf_pilotdroid", displayName: "DROID PILOT", weapons: weapons("RADIATION LAUNCHER", "BLASTER PISTOL", "FUSION CUTTER", "DISPENSER HEALTH & AMMO") },
        { className: "cis_inf_assassindroid", displayName: "ASSASSIN DROID", weapons: weapons("SNIPER RIFLE", "BLASTER PISTOL", "THERMAL DETONATOR", "RECON DROID") },
        { className: "cis_inf_droideka", displayName: "DROIDEKA", weapons: weapons("REPEATING BLASTERS", "SHIELD EMITTER") },
      ],
    },
    posts: [
      { name: "cp6", displayName: "SPIRE", owner: 2, position: [10, 0, -300] },
      { name: "cp14", displayName: "EAST BUNKER", owner: 2, position: [60, 0, -200] },
      { name: "cp13", displayName: "WEST BUNKER", owner: 2, position: [-40, 0, -210] },
      { name: "cp10", displayName: "DERELICT", owner: 1, position: [30, 0, 40] },
      { name: "cp7", displayName: "TECHNO UNION 1", owner: 2, position: [20, 0, -120] },
      { name: "cp3", displayName: "TECHNO UNION 2", owner: 2, position: [-10, 0, -110] },
      { name: "cp2", displayName: "FORWARD CENTER", owner: 1, position: [50, 0, 120], vehicles: { 1: ["LAAT/I GUNSHIP"] } },
    ],
    shots: [
      { position: [-6.94, -59.2, 26.14], quaternion: [0.0855, -0.022, 0.0019, 0.9961] },
      { position: [26.37, -59.94, 122.55], quaternion: [0.0819, -0.4119, 0.0372, 0.9068] },
      { position: [90.94, -49.29, -69.57], quaternion: [0.0744, 0.0772, -0.0058, 0.9942] },
    ],
  };
}

// A port over the running game (window.__bf): the real port's data, bodies,
// models and camera where it has them, with the player's state faked so the
// screen can be shown while he is alive. phase: what humanState says.
export async function livePort(bf, { phase = "awaiting", team = null } = {}) {
  const real = bf.spawnPort || null;
  // The battle this page plays of the map's (src/mission.js).
  const { selectMission } = await import("/src/mission.js");
  const world = selectMission(await fetch(`/maps/${new URLSearchParams(location.search).get("map") || "nab2"}/map.json`).then((r) => r.json()), location.search);
  const teams = real?.teams?.() || Object.fromEntries(Object.entries(world.spawnScreen?.teams || {}).map(([t, v]) => [t, { ...v, logo: LOGOS[v.side] }]));
  const rosters = {};
  for (const t of [1, 2]) rosters[t] = real?.roster?.(t) || [];
  const human = real?.humanState?.() || {};
  const port = new FakeSpawnPort({
    teams,
    rosters,
    posts: [],
    human: { ...human, phase, team: team ?? human.team ?? 1, classIndex: human.classIndex ?? 0 },
    shots: real?.cameraShots?.() || world.spawnScreen?.cameraShots || [],
    commonModel: (name) => real?.commonModel?.(name) ?? null,
    previewBody: (className) => real?.previewBody?.(className) ?? null,
    setViewShot: (shot) => real?.setViewShot?.(shot),
  });
  // Posts and their picks come from the world as it stands.
  if (real) {
    port.posts = (t) => real.posts(t);
    port.choosePost = (t, previous) => real.choosePost(t, previous);
  }
  return port;
}

const LOGOS = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance" };
