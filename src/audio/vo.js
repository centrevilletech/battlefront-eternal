// Voice-over during a battle, as the 2004 game triggers it (docs/
// formats.md, "Sound"). Every line is 2D and queued on its stream (cw_vo on
// Theed), so lines never overlap.
//
// - Command posts: each post's instance properties name a line for each
//   listener side, actor side, and event: VO_<listener>_<actor>Capture when
//   the actor's side captures it, VO_<listener>_<actor>Lost when the actor's
//   side loses it (it was neutralized). Theed sets six per post, for the
//   Republic and CIS listening: your side capturing, losing, and the enemy
//   capturing. The capture also plays the post's capture sound at the post,
//   and the loss its LostSound.
// - Bleeding (SetBleedingVoiceOver(listener, bleeding team, line, 1)): a
//   team starting to bleed reinforcements plays the listener's line for it;
//   while the listener's own team keeps bleeding the line repeats every
//   30 s. An enemy starting to bleed while your own team bleeds is silent.
//
// - Leaving the battlefield (SetOutOfBoundsVoiceOver(team, line)): the
//   listener's own side's line ("repleaving" on a Clone Wars Bespin), once
//   each time the player goes out (src/battlefield.js).
//
// - Objectives (VO_<listener>_<owner side>Info): while a post stands, the
//   post's update asks for its owner's Info line every frame,
//   "defend the shield generator" to the Rebels, "destroy the shield
//   generator" to the Empire; the line's own PlayInterval (90 s for these)
//   paces it. Only destructible posts set one. Not while the game is over
//   or before the player has a body.
//
// The AI's chatter shares the stream and its queue (chatter.js). Not in the
// port yet: planetary bonus lines (Galactic Conquest only).

const BLEED_REPEAT = 30; // s

export class VoiceOver {
  constructor(audio, mission, world, team, sides) {
    this.audio = audio;
    this.mission = mission || {};
    this.posts = world?.posts || {};
    this.team = Number(team);
    this.sides = sides; // team -> "Rep", "Cis", ...
    this.bleeding = {};
    this.timers = {};
  }

  line(name) {
    if (name) this.audio.streams.queue(name, { category: "voice" });
  }

  update(dt) {
    const battle = this.audio.game.battle;
    if (battle.over) return;
    this.objectives();
    for (const team of [1, 2]) {
      const now = (battle.bleed?.[team] ?? 0) > 0;
      const was = this.bleeding[team] || false;
      this.bleeding[team] = now;
      if (!now) continue;
      if (!was) {
        this.timers[team] = 0;
        // An enemy bleeding while we bleed too: the normal=0 line, which no
        // mission sets.
        if (team !== this.team && this.bleeding[this.team]) continue;
        this.line(this.bleedLine(team));
        continue;
      }
      this.timers[team] += dt;
      if (this.timers[team] > BLEED_REPEAT) {
        this.timers[team] = 0;
        if (team === this.team) this.line(this.bleedLine(team));
      }
    }
  }

  // Each standing post's Info line for its owner, asked for again each
  // frame; the line's PlayInterval decides when it is heard.
  objectives() {
    if (!this.audio.game.battle?.human?.unit) return;
    const listener = this.sides[this.team];
    if (!listener) return;
    for (const post of this.audio.game.posts?.posts || []) {
      if (post.active === false || (post.building && !post.building.alive)) continue;
      const vo = this.posts[post.desc?.name]?.vo;
      const owner = this.sides[post.team];
      if (vo && owner) this.line(vo[`VO_${listener}_${owner}Info`]);
    }
  }

  // The player has just gone outside the battlefield.
  leaving() {
    this.line(this.mission.outOfBounds?.[String(this.team)]);
  }

  bleedLine(team) {
    const found = (this.mission.bleeding || []).find(([listener, bleeding, , normal]) => Number(listener) === this.team && Number(bleeding) === team && Number(normal ?? 1) !== 0);
    return found?.[2] || null;
  }

  // A command post event from the game: captured by `by`, or lost by `from`.
  post(event) {
    const post = this.posts[event.post?.desc?.name] || this.posts[event.post?.name];
    const listener = this.sides[this.team];
    const actorTeam = event.kind === "captured" ? event.by : event.from;
    const actor = this.sides[actorTeam];
    if (!post || !listener || !actor) return;
    const what = event.kind === "captured" ? "Capture" : event.kind === "neutralized" || event.kind === "lost" ? "Lost" : null;
    if (what) this.line(post.vo[`VO_${listener}_${actor}${what}`]);
  }
}
