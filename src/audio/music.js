// The battle's music, as the 2004 game runs it (docs/formats.md,
// "Sound"):
//
// - SetAmbientMusic(team, amount, track, stage, isFraction) gives each team
//   a start, middle, and end track; with isFraction the threshold is that
//   fraction of the team's reinforcements when the script ran, in whole
//   tickets (Theed: 200, 198, 20).
// - The stage: for team 1 and then team 2, the last of its stages whose
//   threshold its tickets are at or below. Team 2 is looked at last and its
//   start stage always matches, so in practice team 2's tickets set the
//   stage for both sides.
// - Every frame the listener's team's track for the stage is asked for, and
//   the stage before it stopped (for both teams). One track plays at a time:
//   the highest priority among those asked for, once it is ready. A track
//   is ready MinInactiveTime after it last stopped (and at the start); once
//   playing it can't be stopped for MinPlaybackTime. It fades in over
//   FadeInTime and out over FadeOutTime, plays one segment (picked by
//   weight, or in turn with CyclePlayback), and, still asked for, plays
//   again when it is next ready: Theed's start track plays its 39 s, rests
//   90 s, and plays again.
// - When a team is out of reinforcements, 2 s later the game is over: every
//   stream and voice stops, sound effects fade out over a second, and the
//   listener's team's victory or defeat track plays.

const STOPPED = 0;
const STARTING = 1;
const PLAYING = 2;
const STOPPING = 3;
const GAME_OVER_DELAY = 2; // s

class Track {
  constructor(director, name) {
    this.director = director;
    this.name = name;
    this.music = director.audio.data.object("Music", name);
    this.state = STOPPED;
    this.minPlay = this.music?.MinPlaybackTime ?? this.music?.["6813be83"] ?? 0;
    this.minIdle = this.music?.MinInactiveTime ?? this.music?.["35724887"] ?? 0;
    this.priority = this.music?.Priority ?? 0;
    this.played = 0;
    this.idle = this.minIdle; // ready from the start
    this.voice = null;
    this.wanted = false;
  }

  get ready() {
    return this.idle >= this.minIdle;
  }

  // The order tracks start in: priority, and only once ready.
  get rank() {
    return this.ready ? this.priority + 0.0001 : 0;
  }

  start() {
    const m = this.music;
    const stream = m?.SoundStream || m?.soundstream || this.name;
    this.state = STARTING;
    this.played = 0;
    this.voice = this.director.audio.streams.play(stream, {
      category: "music",
      fadeIn: m?.FadeInTime ?? 1,
      onEnd: (voice) => {
        if (voice === this.voice) this.ended();
      },
    });
    if (!this.voice) this.ended();
    else this.state = PLAYING;
  }

  // Asked to stop: only after MinPlaybackTime.
  stop() {
    if (this.state !== PLAYING && this.state !== STARTING) return false;
    if (this.played < this.minPlay) return false;
    this.state = STOPPING;
    const voice = this.voice;
    this.voice = null;
    voice?.stop(this.music?.FadeOutTime ?? 1);
    this.ended();
    return true;
  }

  ended() {
    this.state = STOPPED;
    this.voice = null;
    this.idle = 0;
  }

  update(dt) {
    if (this.state === PLAYING || this.state === STARTING) this.played += dt;
    else this.idle += dt;
  }
}

export class BattleMusic {
  constructor(audio, mission, team) {
    this.audio = audio;
    this.mission = mission || {};
    this.team = Number(team);
    this.tracks = new Map();
    this.slots = {}; // team -> [stage 0, 1, 2] -> { track, threshold }
    const battle = audio.game.battle;
    for (const [t, list] of Object.entries(this.mission.music || {})) {
      const start = battle.start?.[t] ?? 0;
      for (const [amount, name, stage, isFraction] of list) {
        const threshold = isFraction !== undefined && isFraction !== 0 ? Math.trunc(start * amount) : amount;
        (this.slots[t] ||= [])[Math.round(stage)] = { track: this.track(name), threshold };
      }
    }
    this.over = null; // s since the battle was decided
    this.stage = 0;
  }

  track(name) {
    if (!this.tracks.has(name)) this.tracks.set(name, new Track(this, name));
    return this.tracks.get(name);
  }

  // The stage, as the original game works it out (see the top of the file).
  stageNow() {
    const battle = this.audio.game.battle;
    let stage = this.stage;
    for (const team of ["1", "2"]) {
      const slots = this.slots[team];
      if (!slots) continue;
      const left = battle.reinforcements?.[team] ?? 0;
      for (let s = 2; s >= 0; s--) {
        if (slots[s] && slots[s].threshold >= left) {
          stage = s;
          break;
        }
      }
    }
    return stage;
  }

  update(dt) {
    const battle = this.audio.game.battle;
    for (const track of this.tracks.values()) track.update(dt);
    if (battle.over) {
      this.gameOver(dt);
      return;
    }
    this.stage = this.stageNow();
    for (const track of this.tracks.values()) track.wanted = false;
    const own = this.slots[String(this.team)];
    const want = own?.[this.stage]?.track || null;
    if (want) want.wanted = true;
    // Earlier stages stop, for both teams.
    if (this.stage > 0) {
      for (const slots of Object.values(this.slots)) {
        const before = slots[this.stage - 1]?.track;
        if (before && before !== want) before.stop();
      }
    }
    // One plays: the best of those asked for; the others are stopped.
    const asked = [...this.tracks.values()].filter((t) => t.wanted || t.state !== STOPPED);
    asked.sort((a, b) => b.rank - a.rank);
    const head = asked[0];
    for (const t of asked) if (t !== head && t.state !== STOPPED) t.stop();
    const playing = [...this.tracks.values()].some((t) => t.state !== STOPPED);
    if (head && head.wanted && head.state === STOPPED && !playing && head.ready) head.start();
  }

  gameOver(dt) {
    if (this.over === null) this.over = 0;
    const before = this.over;
    this.over += dt;
    if (before < GAME_OVER_DELAY && this.over >= GAME_OVER_DELAY) {
      const battle = this.audio.game.battle;
      const lost = (battle.reinforcements?.[this.team] ?? 1) <= 0;
      for (const t of this.tracks.values()) {
        t.voice?.stop(0.05);
        t.ended();
      }
      this.audio.endOfBattle();
      const name = (lost ? this.mission.defeat : this.mission.victory)?.[String(this.team)];
      if (name) this.track(name).start();
    }
  }
}
