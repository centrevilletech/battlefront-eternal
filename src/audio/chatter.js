import { hashKey } from "./hash.js";

// The soldiers' chatter: the Classic AI's tactical voice-over and warnings,
// as the 2004 game plays them (docs/formats.md, "Sound"). The AI says
// who speaks, where, and which of his class's sound properties
// (src/ai/classic/squads.js: "vo" records in the simulation journal; a
// world worker sends them to the page, src/game.js); what is heard of it is
// worked out here.
//
// - The line is the speaker's class sound for the property: FleeSound (a
//   grenade warning), HeardEnemySound, ChargeSound, and the squad command
//   bank's entries (AISC* for a bot, SC* for the local player's own
//   commands), whose ODF keys the sound data keeps as their hashes. A
//   soldier in a seat speaks with his own class's lines. Every stock line
//   is a stream: cw_vo or gcw_vo for the bots (with the commander's
//   voice-over), cw_tac_vo or gcw_tac_vo for the player's own commands.
// - A bot's line (the soldier's chatter functions and the command
//   dispatcher) is queued on its stream's slot, 3D where he said it
//   (streams.js): everyone hears it, either team, by distance (the stock
//   chatter at full level to 11 or 16 m, nothing past 150 m); one line
//   plays at a time on the slot, the commander's lines included, best score
//   first.
// - The local player's own command plays at once, 2D: the dispatcher turns
//   the line's 3D off and doesn't queue it.
// - Cooldowns are the sounds' own: a line's PlayInterval holds every
//   speaker of it (20 +- 5 s for most of the stock chatter, 1 s for some of
//   the Naboo guard's), and each segment its own interval.
//
// Online the server's bots say nothing to the page yet: their records stay
// in the server's journal.

export class Chatter {
  constructor(audio) {
    this.audio = audio;
  }

  // The speaker's sound for a property: the class key by its name, else
  // by its hash (the command bank's keys have no names in the data).
  line(className, property) {
    const audio = this.audio;
    return audio.classSound(className, property) || audio.classSound(className, hashKey(property).slice(2));
  }

  // A "vo" record: { source: { unit }, position: [x, y, z], property,
  // positional }.
  say(record) {
    const speaker = record?.source?.unit;
    const className = speaker?.occupant?.className || speaker?.className;
    if (!className || !record.property) return;
    const ref = this.line(className, record.property);
    if (!ref) return;
    const streams = this.audio.streams;
    if (record.positional === false) {
      streams.say(ref.name, { category: "voice" });
      return;
    }
    const p = record.position;
    const position = Array.isArray(p) ? { x: p[0], y: p[1], z: p[2] } : p ? { x: p.x, y: p.y, z: p.z } : null;
    streams.queue(ref.name, { category: "voice", position, spatial: true });
  }
}
