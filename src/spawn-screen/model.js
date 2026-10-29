// The spawn screen's state, apart from the world and the screen: the team,
// class and command post the player has picked, and where the screen is in
// its life. The 2004 game's SpawnDisplay keeps the same
// (docs/spawn-screen.md).
//
// - closed: no screen.
// - choosing: the original's state 1, the player picking.
// - committed: state 2, SPAWN pressed and the choice sent to the world,
//   until the world spawns the unit (in single player about a frame).
//
// Pure: everything about the world comes from the port (humanState, roster,
// posts, choosePost), and nothing here touches the page.
//
// Choices, as the original makes them:
// - On opening, the team and class are the player's last (class 0 at the
//   start), and the post is the one remembered from the last life, checked
//   again: kept while eligible, else the eligible post nearest it, else the
//   front line (port.choosePost), else a random eligible post.
// - A team switch resets the class to 0 and picks a random eligible post of
//   the new team, then checks it the same way.
// - The post is checked again every frame; a lost post moves the selection
//   to the nearest eligible one.
// - Tab steps through the team's eligible posts with a counter per team.

export const CLOSED = "closed";
export const CHOOSING = "choosing";
export const COMMITTED = "committed";

export class SpawnModel {
  // port: the world's spawn port (docs/spawn-screen.md); random: 0..1, for
  // the post picks (UI only: the pick reaches the world inside the request).
  constructor({ port, random = Math.random } = {}) {
    this.port = port;
    this.random = random;
    this.state = CLOSED;
    this.team = 1;
    this.classIndex = 0;
    this.post = null;
    this.opens = 0;
    this.cycles = new Map(); // team -> the Tab counter
    this.listeners = new Map();
  }

  get isOpen() {
    return this.state !== CLOSED;
  }

  // Listeners by event: open ({ first, reason }), close, class ({ index }),
  // team ({ team, source }), post ({ post, reason }), commit (the request).
  on(event, listener) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event).add(listener);
    return () => this.listeners.get(event)?.delete(listener);
  }

  emit(event, detail) {
    for (const listener of this.listeners.get(event) || []) listener(detail);
  }

  // The selected team's classes, without heroes, in mission order.
  roster(team = this.team) {
    return this.port.roster(team) || [];
  }

  get className() {
    return this.roster()[this.classIndex]?.className ?? null;
  }

  // The selected team's posts, and the ones it may spawn at.
  posts(team = this.team) {
    return this.port.posts(team) || [];
  }

  eligible(team = this.team) {
    return this.posts(team).filter((post) => post.eligible);
  }

  // The screen opens: team and class from the player's Character, the post
  // re-checked. reason: "start" (map load), "death", or anything else.
  open(reason = "death") {
    const human = this.port.humanState();
    if (human.spectator) return;
    this.state = CHOOSING;
    this.team = human.team || 1;
    const count = this.roster().length;
    const index = Number.isInteger(human.classIndex) ? human.classIndex : this.indexOf(human.className);
    this.classIndex = index >= 0 && index < count ? index : 0;
    if (!this.post && human.post) this.post = human.post;
    const first = this.opens === 0;
    this.opens += 1;
    this.emit("open", { first, reason });
    this.check("open", true);
  }

  close() {
    if (this.state === CLOSED) return;
    this.state = CLOSED;
    this.emit("close");
  }

  indexOf(className) {
    return className ? this.roster().findIndex((entry) => entry.className === className) : -1;
  }

  isEligible(name, team = this.team) {
    return this.posts(team).some((post) => post.name === name && post.eligible);
  }

  // Every frame: keep the post while it is eligible, else move it as the
  // original does. On opening (random = true) a failed pick falls back to a
  // random eligible post.
  check(reason = "check", random = false) {
    if (this.state !== CHOOSING) return false;
    let next = this.port.choosePost(this.team, this.post) || null;
    if (!next && random) next = this.randomPost();
    return this.setPost(next, reason);
  }

  setPost(name, reason) {
    if (name === this.post) return false;
    this.post = name;
    this.emit("post", { post: name, reason });
    return true;
  }

  randomPost(team = this.team) {
    const eligible = this.eligible(team);
    if (!eligible.length) return null;
    return eligible[Math.min(Math.floor(this.random() * eligible.length), eligible.length - 1)].name;
  }

  selectClass(index) {
    if (this.state !== CHOOSING) return false;
    const count = this.roster().length;
    if (!Number.isInteger(index) || index < 0 || index >= count || index === this.classIndex) return false;
    this.classIndex = index;
    this.emit("class", { index });
    return true;
  }

  // Up wraps to the last class; down is (index + 1) mod count.
  previousClass() {
    const count = this.roster().length;
    if (!count) return false;
    return this.selectClass(this.classIndex - 1 >= 0 ? this.classIndex - 1 : count - 1);
  }

  nextClass() {
    const count = this.roster().length;
    if (!count) return false;
    return this.selectClass((this.classIndex + 1) % count);
  }

  // Whether he may pick his team, read when asked: online the server says so
  // in its welcome, which can arrive after the screen opened at map load (a
  // server that assigns teams must never show the switch).
  get canSwitchTeams() {
    return this.port.humanState().canSwitchTeams !== false;
  }

  // Team 1 <-> 2, when the mission allows it. source: "key" or "mouse" (a
  // mouse switch also flips the reinforcement counter).
  switchTeam(source = "key") {
    if (this.state !== CHOOSING || !this.canSwitchTeams) return false;
    this.team = this.team === 1 ? 2 : 1;
    this.classIndex = 0;
    this.emit("team", { team: this.team, source });
    this.emit("class", { index: 0 });
    this.post = this.randomPost();
    this.emit("post", { post: this.post, reason: "team" });
    this.check("team");
    return true;
  }

  // The world moved him to another team while the screen is up (online,
  // the server's assignment arriving after the screen opened on the URL's
  // side): show that team's classes and posts, as switchTeam does, without
  // asking for the switch.
  followTeam(team) {
    if (this.state !== CHOOSING || (team !== 1 && team !== 2) || team === this.team) return false;
    this.team = team;
    this.classIndex = 0;
    this.emit("team", { team, source: "world" });
    this.emit("class", { index: 0 });
    this.post = this.randomPost();
    this.emit("post", { post: this.post, reason: "team" });
    this.check("team");
    return true;
  }

  // A post clicked on the map: only one the selected team itself holds and
  // may spawn at (allied posts take Tab or the default pick).
  selectPost(name) {
    if (this.state !== CHOOSING) return false;
    const post = this.posts().find((p) => p.name === name);
    if (!post || !post.eligible || post.owner !== this.team) return false;
    return this.setPost(name, "click");
  }

  // Tab: the next of the team's eligible posts, by the team's own counter.
  cyclePost() {
    if (this.state !== CHOOSING) return false;
    const eligible = this.eligible();
    if (!eligible.length) return false;
    let at = this.cycles.get(this.team) || 0;
    if (at >= eligible.length) at = 0;
    this.cycles.set(this.team, at + 1);
    return this.setPost(eligible[at].name, "cycle");
  }

  // SPAWN: the choice goes to the world. Null without a class or a post.
  commit() {
    if (this.state !== CHOOSING || this.port.humanState()?.spectator) return null;
    const className = this.className;
    if (!className || !this.post) return null;
    const request = { team: this.team, className, post: this.post };
    this.state = COMMITTED;
    this.emit("commit", request);
    return request;
  }

  // Back to choosing, when the world didn't take the request.
  uncommit() {
    if (this.state !== COMMITTED) return;
    this.state = CHOOSING;
    this.check("uncommit");
  }
}
