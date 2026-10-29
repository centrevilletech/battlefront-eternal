// What the end-of-match screens report for each Character (DETAILS and
// AWARDS; docs/scoreboard.md), kept as the 2004 game keeps its
// per-Character stats record.
//
// Most of it is kept per team slot (team 2's slot 1, every other team's
// slot 0), so a Character that changes teams keeps what it did for each.
// Nothing counts once the game is over.
//
// On each kill:
// - the killer's count of kills of that victim (NEMESIS and BAIT read it),
//   team kills and suicides included;
// - for an enemy's kill: the kills in this life (PUBLIC ENEMY, the best of
//   them), the kills while his own health was under 64/255 (SURVIVALIST),
//   and the most kills inside 5 s (KILLING SPREE), counted in a ring of the
//   last hundred kills shared by everyone. The 2004 game's count checks
//   the slot stored at the ring's next free entry instead of each entry's,
//   so until the ring has wrapped only slot 0's (team 1's) sprees count.
//   That is kept: it is what the original awards.
// - a team kill (a friend, or himself): TRAITOR.
// On each death: BANTHA FODDER; this life's kills and clock start again.
// A head hit (DEAD EYE, HEAD SHOTS) goes to whoever had hurt the soldier
// last before it. A vehicle's destruction (TANK BUSTER, VEHICLES DESTROYED)
// to whoever hurt it last, whatever his team; turrets don't count.
//
// Every world tick, for every bot and for the player from his spawn until
// his spawn screen opens:
// - time in his class (FAVORITE CLASS) and this life's length (LONGEST
//   LIVING STREAK), even while a bot waits to respawn;
// - with a body: time with the weapon in his hand or his seat's
//   (FAVORITE WEAPON), time in a vehicle's seat (FAVORITE VEHICLE), time
//   within 10 m of one spot (CAMPER), and his health: under 10% and back to
//   full is a NARROW ESCAPE.
// The time tables hold 5 classes, 10 weapons and 5 vehicles a slot; a new
// one takes the place of the one with the least time.

export const SPREE_WINDOW = 5; // s
export const RING_SIZE = 100;
export const CAMP_RADIUS = 10; // m
export const SURVIVAL_HEALTH = 64; // of 255
export const ESCAPE_LOW = 0.1;
const TABLES = { classes: 5, weapons: 10, vehicles: 5 };

export const slotOf = (team) => (team === 2 ? 1 : 0);
const u8 = (value) => value & 0xff;

function slotRecord() {
  return {
    teamKills: 0,
    vehiclesDestroyed: 0,
    spree: 0,
    survivalist: 0,
    lifeKills: 0,
    lifeKillsBest: 0,
    headshots: 0,
    deaths: 0,
    life: 0,
    lifeBest: 0,
    camp: 0,
    campBest: 0,
    escapes: 0,
    victims: new Map(), // Character -> kills of it
    classes: [], // [{ key, label, time }]
    weapons: [],
    vehicles: [],
  };
}

// A Character's record, made when first needed.
export function recordOf(character) {
  if (!character.record) character.record = { slots: [slotRecord(), slotRecord()], anchor: null, health: 255, low: false };
  return character.record;
}

// The slot a Character's numbers go to now.
function slot(character) {
  return recordOf(character).slots[slotOf(character.team)];
}

// Seconds into a time table (the least-used entry gives way when full).
function addTime(table, size, key, label, dt) {
  if (!key) return;
  const entry = table.find((e) => e.key === key);
  if (entry) {
    entry.time += dt;
    if (label) entry.label = label;
    return;
  }
  if (table.length < size) {
    table.push({ key, label: label || null, time: dt });
    return;
  }
  let least = table[0];
  for (const e of table) if (e.time < least.time) least = e;
  least.key = key;
  least.label = label || null;
  least.time = dt;
}

const text = (value) => String(value ?? "").trim().replace(/\n/g, " ") || null;

// What a Character's tallies see of him this tick: the body he controls
// (his soldier, or the vehicle or turret he sits in), the weapon he holds
// or his seat's first, the vehicle he sits in, and his soldier's health
// (0..1, while it lives). Null with no body.
export function observe(character) {
  const unit = character.unit;
  if (!unit || unit.removed) return null;
  const seat = unit.seat && !unit.seat.released ? unit.seat : null;
  const vehicle = seat?.vehicle ?? null;
  const mount = vehicle ?? seat?.turret ?? null;
  const weapon = vehicle ? vehicle.seats?.[seat.index]?.weapons?.[0]?.state : seat?.turret ? seat.turret.weapon : unit.combat?.arsenal?.current(0);
  const combat = unit.combat;
  return {
    position: mount?.position ?? unit.position,
    health: unit.alive && combat && !combat.dead ? combat.health / Math.max(combat.maxHealth || 1, 1e-6) : null,
    weapon: weapon ? { key: weapon.name ?? weapon.def?.class ?? null, label: text(weapon.def?.displayName ?? weapon.def?.label) } : null,
    vehicle: vehicle ? { key: vehicle.name ?? null, label: text(vehicle.label) } : null,
    classLabel: text(unit.classDef?.displayName),
  };
}

export class BattleStats {
  constructor() {
    this.time = 0;
    this.ring = { killers: new Array(RING_SIZE).fill(null), times: new Float64Array(RING_SIZE), slots: new Int32Array(RING_SIZE), head: 0, tail: 0 };
  }

  // A kill: killer (a Character or null) of victim, relation the killer's
  // team's to the victim's. The killer's own last health (0..255) is the
  // tallies' (recordOf(killer).health).
  kill(killer, victim, relation) {
    if (!killer || relation === 0) return;
    const record = slot(killer);
    if (victim) record.victims.set(victim, u8((record.victims.get(victim) || 0) + 1));
    if (relation > 0) {
      record.teamKills = u8(record.teamKills + 1);
      return;
    }
    if (recordOf(killer).health < SURVIVAL_HEALTH) record.survivalist = u8(record.survivalist + 1);
    record.lifeKills = u8(record.lifeKills + 1);
    record.lifeKillsBest = Math.max(record.lifeKillsBest, record.lifeKills);
    this.spree(killer);
  }

  // The ring of recent kills: the killer's kills in the last 5 s, counted
  // only when the slot at the ring's next free entry is his (the
  // original game's).
  spree(killer) {
    const ring = this.ring;
    const next = (i) => (i + 1) % RING_SIZE;
    const now = this.time;
    while (ring.tail !== ring.head && !(now - SPREE_WINDOW < ring.times[ring.tail])) ring.tail = next(ring.tail);
    const head = next(ring.head);
    if (head === ring.tail) return; // full
    const own = slotOf(killer.team);
    ring.killers[ring.head] = killer;
    ring.times[ring.head] = now;
    ring.slots[ring.head] = own;
    ring.head = head;
    let count = 0;
    for (let i = ring.tail; i !== head; i = next(i)) if (ring.killers[i] === killer && ring.slots[head] === own) count++;
    const record = slot(killer);
    record.spree = Math.max(record.spree, u8(count));
  }

  // A death: BANTHA FODDER; the life's kills and clock and the low-health
  // mark start again.
  death(victim) {
    if (!victim) return;
    const record = slot(victim);
    record.deaths = u8(record.deaths + 1);
    record.lifeKills = 0;
    record.life = 0;
    recordOf(victim).low = false;
  }

  // A head hit, for whoever had hurt the soldier last before it.
  headshot(credited) {
    if (!credited) return;
    const record = slot(credited);
    record.headshots = (record.headshots + 1) & 0xffff;
  }

  // A vehicle destroyed, for whoever hurt it last.
  vehicleDestroyed(destroyer) {
    if (!destroyer) return;
    const record = slot(destroyer);
    record.vehiclesDestroyed = u8(record.vehiclesDestroyed + 1);
  }

  // One world tick of dt seconds for each Character whose tallies run.
  // see(character): observe(character) by default.
  tick(dt, characters, active = () => true, see = observe) {
    this.time += dt;
    for (const character of characters) {
      if (!active(character)) continue;
      const record = recordOf(character);
      const own = slot(character);
      const seen = character.className || character.unit ? see(character) : null;
      if (character.className) {
        addTime(own.classes, TABLES.classes, character.className, seen?.classLabel, dt);
        own.life += dt;
        own.lifeBest = Math.max(own.lifeBest, own.life);
      }
      if (!seen) continue;
      if (seen.health !== null) {
        record.health = Math.min(Math.max(Math.trunc(seen.health * 255), 0), 255);
        if (seen.health < ESCAPE_LOW) record.low = true;
        else if (record.low && seen.health >= 1) {
          record.low = false;
          own.escapes = u8(own.escapes + 1);
        }
      }
      const p = seen.position;
      if (p) {
        const a = record.anchor;
        if (!a || (p.x - a.x) ** 2 + (p.y - a.y) ** 2 + (p.z - a.z) ** 2 > CAMP_RADIUS * CAMP_RADIUS) {
          record.anchor = { x: p.x, y: p.y, z: p.z };
          own.camp = 0;
        } else {
          own.camp += dt;
          own.campBest = Math.max(own.campBest, own.camp);
        }
      }
      if (seen.weapon) addTime(own.weapons, TABLES.weapons, seen.weapon.key, seen.weapon.label, dt);
      if (seen.vehicle) addTime(own.vehicles, TABLES.vehicles, seen.vehicle.key, seen.vehicle.label, dt);
    }
  }
}

// ---------------------------------------------------------------- sharing

// A Character's record as the end-of-match screens read it, for a page
// that shows a battle run elsewhere (a world worker: src/runtime/replica.js;
// a server: the world summaries, src/runtime/network-entity-summary.js):
// each team slot's tallies, its victims by roster index, and its favourite
// class, weapon and vehicle (the first entry of each table with the most
// time, which is all personalStats shows). The running clocks, the camping
// spot and the low-health mark stay with the battle. Null without a record.
export const SHARED_TALLIES = ["teamKills", "vehiclesDestroyed", "spree", "survivalist", "lifeKillsBest", "headshots", "deaths", "escapes", "lifeBest", "campBest"];
export const FAVOURITES = ["classes", "weapons", "vehicles"];
export function packRecord(character, characters) {
  const record = character?.record;
  if (!record) return null;
  return record.slots.map((s) => {
    const out = {};
    for (const key of SHARED_TALLIES) out[key] = s[key];
    out.victims = [];
    for (const [victim, n] of s.victims) {
      const index = characters.indexOf(victim);
      if (index >= 0) out.victims.push([index, n]);
    }
    for (const table of FAVOURITES) {
      let best = null;
      for (const e of s[table]) if (!best || e.time > best.time) best = e;
      out[table] = best ? [best.key, best.label, best.time] : null;
    }
    return out;
  });
}

// A packed record onto the Character (victims by index into `characters`,
// the page's own roster).
export function applyRecord(character, packed, characters) {
  if (!character || !Array.isArray(packed)) return;
  const record = recordOf(character);
  packed.forEach((p, i) => {
    const s = record.slots[i];
    if (!s || !p) return;
    for (const key of SHARED_TALLIES) s[key] = Number(p[key]) || 0;
    s.victims = new Map();
    for (const [index, n] of p.victims || []) if (characters[index]) s.victims.set(characters[index], n);
    for (const table of FAVOURITES) {
      const best = p[table];
      s[table] = best ? [{ key: best[0], label: best[1] ?? null, time: best[2] }] : [];
    }
  });
}

// label(kind, key) for personalStats from the loaded content (game.data:
// its sides and vehicle pack), named as the tallies name them (observe): a
// class, a weapon or a vehicle that arrived by key alone (a server's record).
export function contentLabels(data) {
  const names = { class: new Map(), weapon: new Map(), vehicle: new Map() };
  const weapon = (def) => { if (def?.class && !names.weapon.has(def.class)) names.weapon.set(def.class, text(def.displayName ?? def.label)); };
  for (const side of Object.values(data?.sides || {})) {
    for (const [className, def] of Object.entries(side.units || {})) {
      names.class.set(className, text(def.displayName));
      for (const w of def.weapons || []) weapon(w);
    }
  }
  for (const [name, def] of Object.entries(data?.vehicles?.classes || {})) {
    names.vehicle.set(name, text(def.label));
    for (const seat of def.seats || []) for (const w of seat.weapons || []) weapon(w.def);
  }
  return (kind, key) => names[kind]?.get(key) ?? null;
}

// ---------------------------------------------------------------- reports

const sum = (character, key) => recordOf(character).slots.reduce((total, s) => total + s[key], 0);

// m:ss of whole seconds (the minutes not padded).
export function clock(seconds) {
  const s = Math.max(Math.trunc(seconds), 0);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// The entry with the most time across both slots' tables, or null.
function favourite(character, table) {
  let best = null;
  for (const s of recordOf(character).slots) for (const e of s[table]) if (!best || e.time > best.time) best = e;
  return best;
}

// The Character that killed this one most often, and how often.
function nemesis(character, characters) {
  let best = null;
  let most = 0;
  for (const other of characters) {
    if (other === character) continue;
    const n = recordOf(other).slots.reduce((total, s) => total + (s.victims.get(character) || 0), 0);
    if (n > most) {
      most = n;
      best = other;
    }
  }
  return best ? { character: best, count: most } : null;
}

// The Character this one killed most often.
function bait(character) {
  const counts = new Map();
  for (const s of recordOf(character).slots) for (const [victim, n] of s.victims) counts.set(victim, (counts.get(victim) || 0) + n);
  let best = null;
  let most = 0;
  for (const [victim, n] of counts) {
    if (n > most) {
      most = n;
      best = victim;
    }
  }
  return best ? { character: best, count: most } : null;
}

// DETAILS: [{ key, label, value }] in the screen's order, after its three
// blank rows. label(kind, key) names a class, weapon or vehicle when its
// entry has no name of its own; name(character) is how a Character is
// listed. An empty value shows the label alone.
export const PERSONAL = [
  ["ifs.stats.headshots", "HEAD SHOTS"],
  ["ifs.stats.favclass", "FAVORITE CLASS"],
  ["ifs.stats.favweapon", "FAVORITE WEAPON"],
  ["ifs.stats.favvehicle", "FAVORITE VEHICLE"],
  ["ifs.stats.livingstreak", "LONGEST LIVING STREAK"],
  ["ifs.stats.narrowescapes", "NARROW ESCAPES"],
  ["ifs.stats.vehiclesdestroyed", "VEHICLES DESTROYED"],
  ["ifs.stats.nemesis", "NEMESIS"],
  ["ifs.stats.bait", "BAIT"],
];

export function personalStats(character, characters, { label = () => null, name = (c) => c.name } = {}) {
  const fav = (table, kind) => {
    const e = favourite(character, table);
    const named = e ? e.label || label(kind, e.key) : null;
    return named ? `${named} ${clock(e.time)}` : "";
  };
  const who = (found) => (found ? `${name(found.character)} (${found.count})` : "");
  const best = recordOf(character).slots.reduce((total, s) => total + Math.trunc(s.lifeBest), 0);
  const values = [
    String(sum(character, "headshots")),
    fav("classes", "class"),
    fav("weapons", "weapon"),
    fav("vehicles", "vehicle"),
    ` ${clock(best)}`,
    String(sum(character, "escapes")),
    String(sum(character, "vehiclesDestroyed")),
    who(nemesis(character, characters)),
    who(bait(character)),
  ];
  return PERSONAL.map(([key, fallback], i) => ({ key, label: fallback, value: values[i] }));
}

// AWARDS, in the screen's order: each the highest value over every
// Character, slot 0 first then slot 1, each in roster order; only more
// than the best so far wins, so ties go to the first and 0 wins nothing.
export const AWARDS = [
  ["ifs.stats.tankbuster", "TANK BUSTER", (s) => s.vehiclesDestroyed],
  ["ifs.stats.killingspree", "KILLING SPREE", (s) => s.spree],
  ["ifs.stats.camper", "CAMPER", (s) => Math.trunc(s.campBest), (v) => clock(v)],
  ["ifs.stats.survivalist", "SURVIVALIST", (s) => s.survivalist],
  ["ifs.stats.traitor", "TRAITOR", (s) => s.teamKills],
  ["ifs.stats.publicenemy", "PUBLIC ENEMY", (s) => s.lifeKillsBest],
  ["ifs.stats.meat", "BANTHA FODDER", (s) => s.deaths],
  ["ifs.stats.deadeye", "DEAD EYE", (s) => s.headshots],
];

// [{ key, label, character, value, text }]: character null when nobody
// won it.
export function awards(characters, { name = (c) => c.name } = {}) {
  return AWARDS.map(([key, label, value, format = String]) => {
    let best = 0;
    let winner = null;
    for (const index of [0, 1]) {
      for (const character of characters) {
        const v = value(recordOf(character).slots[index]);
        if (v > best) {
          best = v;
          winner = character;
        }
      }
    }
    return { key, label, character: winner, value: best, text: winner ? `${name(winner)} (${format(best)})` : "" };
  });
}
