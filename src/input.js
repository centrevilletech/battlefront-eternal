// Keyboard and mouse, bound like the captured 2004 PC control screens
// (docs/menus.md, Alignment and controls follow-up), or as the player
// rebinds them on the CONTROL page. Held keys are read as axes each step;
// presses are taken once by the simulation step.
import { ActionQueue } from './runtime/action-queue.js';
import { INPUT_CODES } from './runtime/network-entity-input.js';
import { FLIGHT_BINDINGS } from './flight/controls.js';

// The bindings: axes (each [plus keys, minus keys]), presses (taken once)
// and holds (read while down). The options' CONTROL page lists them
// (src/menus/hotkeys.js).
export const AXES = {
  forward: [["KeyW", "ArrowUp"], ["KeyS", "ArrowDown"]],
  right: [["KeyD", "ArrowRight"], ["KeyA", "ArrowLeft"]],
  turn: [[], []],
};

export const PRESSES = {
  jump: ["Space"],
  crouch: ["KeyC", "ControlLeft", "ControlRight"],
  prone: ["KeyX"],
  reload: ["KeyR", "Enter"],
  use: ["KeyE", "Numpad0"],
  nextPrimary: ["KeyF", "WheelUp"],
  previousPrimary: ["WheelDown"],
  nextSecondary: ["KeyG"],
  zoom: ["KeyZ", "Mouse1"],
  perspective: ["KeyQ", "Mouse3"],
  map: ["KeyM"],
  stats: ["Backquote"],
  // Squad commands, as listed by the original PC controls screen.
  squadMoveOut: ["F3"],
  squadHold: ["F4"],
  squadFollow: ["F2"],
  squadAtEase: ["F1"],
};

export const HOLDS = {
  firePrimary: ["Mouse0"],
  fireSecondary: ["Mouse2", "AltLeft", "AltRight"],
};

export const BINDINGS = { axes: AXES, presses: PRESSES, holds: HOLDS };

// The keys the world reads (src/runtime/commands.js): the tables', and for
// turning, which no key does by default, two keys nothing else is bound to.
export const WORLD_AXES = { ...AXES, turn: [["NumpadMultiply"], ["NumpadDivide"]] };

// Every binding by the name the CONTROL page's rows give it ("presses.jump",
// "holds.firePrimary", "axes.forward+" / "axes.forward-"), with its keys.
export function flatBindings(tables = BINDINGS) {
  const flat = {};
  for (const [name, [plus, minus]] of Object.entries(tables.axes)) Object.assign(flat, { [`axes.${name}+`]: plus, [`axes.${name}-`]: minus });
  for (const table of ["presses", "holds"]) for (const [name, codes] of Object.entries(tables[table])) flat[`${table}.${name}`] = codes;
  return flat;
}

// The tables with the player's own keys over them: the keyBindings
// setting, each binding's keys by its name (flatBindings).
export function playerBindings(own = {}) {
  const keys = (name, codes) => (Array.isArray(own?.[name]) ? own[name].filter((code) => typeof code === "string") : codes);
  const table = (prefix, bindings) => Object.fromEntries(Object.entries(bindings).map(([name, codes]) => [name, keys(`${prefix}.${name}`, codes)]));
  return {
    axes: Object.fromEntries(Object.entries(AXES).map(([name, [plus, minus]]) => [name, [keys(`axes.${name}+`, plus), keys(`axes.${name}-`, minus)]])),
    presses: table("presses", PRESSES),
    holds: table("holds", HOLDS),
  };
}

// One binding of a set given one key alone (the CONTROL page's PRESS NEW
// KEY), the key taken from any other binding of the set that had it, so a
// key does one thing. defaults: the set's own keys by name (flatBindings(),
// FLIGHT_BINDINGS); own: the player's over them, as a setting keeps them.
// Returns the new setting: the bindings that differ from the defaults.
export function rebind(defaults, own, name, code) {
  const next = {};
  for (const [key, codes] of Object.entries(defaults)) {
    const was = Array.isArray(own?.[key]) ? own[key] : codes;
    const now = key === name ? [code] : was.filter((other) => other !== code);
    if (now.length !== codes.length || now.some((other, i) => other !== codes[i])) next[key] = now;
  }
  return next;
}

// The keys the game reads itself, which the CONTROL page lists as they are
// (Esc's pause menu, Tab's player list, T and Y's chat), and the window's
// own: F11's full screen (the desktop app takes it before the page),
// F12's developer tools.
const RESERVED = new Set(["Escape", "Tab", "KeyT", "KeyY", "F11", "F12"]);
// Whether a binding can take a key: one the network can name (INPUT_CODES),
// and not one of those.
export const bindable = (code) => INPUT_CODES.includes(code) && !RESERVED.has(code);

// A set of bindings as Controls reads it, resolved once rather than
// scanning every binding on each keyboard or mouse press (including
// high-rate input devices): which presses and holds a key queues, every key
// bound, and the world's view of them (worldKeys).
function keyMap(tables) {
  const actions = new Map();
  for (const bindings of [tables.presses, tables.holds]) {
    for (const [name, codes] of Object.entries(bindings)) {
      for (const code of codes) {
        if (!actions.has(code)) actions.set(code, []);
        actions.get(code).push(name);
      }
    }
  }
  const held = Object.entries(tables.holds).map(([name, codes]) => [codes, HOLDS[name][0]]);
  for (const [name, sides] of Object.entries(tables.axes)) sides.forEach((codes, side) => held.push([codes, WORLD_AXES[name][side][0]]));
  return { tables, actions, bound: new Set(Object.values(flatBindings(tables)).flat()), ...worldMap(held, [...Object.values(HOLDS), ...Object.values(WORLD_AXES).flat()]) };
}

// A craft's keys (src/boarding.js seatScheme) the same way, against the
// flight defaults the world reads with them.
function flightKeys(scheme) {
  const held = Object.entries(scheme).filter(([name]) => FLIGHT_BINDINGS[name]).map(([name, codes]) => [codes, FLIGHT_BINDINGS[name][0]]);
  return worldMap(held, Object.values(FLIGHT_BINDINGS));
}

// held: [the player's keys, the world's key for them]; world: the world's
// own keys. taken: every key that stands for something the world reads.
function worldMap(held, world) {
  return { held, taken: new Set([...held.flatMap(([codes]) => codes), ...world.flat()]) };
}

// The held keys as a command carries them to the world, which reads the
// tables' keys on the page, in a worker and on the server: each held hold,
// axis side or flight control as the world's key for it, and the keys the
// world reads itself (Space's brake) as they are. A key the player moved
// does only what it was moved to, and none of the player's own keys reach
// the server, whose network names only its own set (INPUT_CODES).
function worldKeys(down, { held, taken }) {
  const keys = [];
  for (const code of down) if (!taken.has(code)) keys.push(code);
  for (const [codes, key] of held) if (!keys.includes(key) && anyDown(down, codes)) keys.push(key);
  return keys;
}
// Flight-scheme names read from the held-key set every tick (flightInput),
// never taken as press edges. Queueing them sends them down the discrete
// action lane, whose wire registry cannot name them: every A/D press in a
// fighter threw "unknown input action" and the stuck edge rethrew forever.
const FLIGHT_HELD = new Set(["throttle", "brake", "rollLeft", "rollRight", "boost", "rollLock"]);

const anyDown = (down, codes) => {
  for (let i = 0; i < codes.length; i++) if (down.has(codes[i])) return 1;
  return 0;
};

// Keys the browser would otherwise scroll or act on, and any function key
// the player has bound (F5 would reload the page).
const SWALLOW = new Set(["Space", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Tab", "F1", "F2", "F3", "F4", "AltLeft", "AltRight"]);
const FUNCTION_KEY = /^F\d+$/;
// The Discord overlay's keys (Shift or Ctrl with `) reach the page too:
// they open Discord, not the stats line (docs/presence.md).
export const overlayChord = (event) => event.code === "Backquote" && (event.shiftKey || event.ctrlKey);

export class Controls {
  // bindings: the player's own keys (the keyBindings setting; setBindings).
  constructor(element, { onLook = () => {}, onInput = null, bindings = null } = {}) {
    this.element = element;
    this.onLook = onLook;
    this.down = new Set();
    this.queued = new ActionQueue();
    this.looks = [];
    // The spawn screen's choices (src/human-spawn.js), sent with the next
    // command in the order made; a cleared input doesn't drop them.
    this.human = [];
    this.sequence = 0;
    this.onClear = null;
    this.locked = false;
    this.enabled = true;
    this.lockListeners = new Set();
    if (bindings) this.setBindings(bindings);

    window.addEventListener("keydown", (event) => {
      if (!this.enabled) return;
      onInput?.(event);
      if (SWALLOW.has(event.code) || (FUNCTION_KEY.test(event.code) && this.keys.bound.has(event.code))) event.preventDefault();
      if (!event.repeat && !overlayChord(event)) this.press(event.code);
      this.down.add(event.code);
    });
    window.addEventListener("keyup", (event) => {
      if (this.enabled) onInput?.(event);
      this.down.delete(event.code);
    });
    window.addEventListener("blur", () => this.clear());
    element.addEventListener("click", () => {
      if (this.enabled && !this.locked) this.lock();
    });
    element.addEventListener("contextmenu", (event) => event.preventDefault());
    window.addEventListener("mousedown", (event) => {
      if (!this.locked || !this.enabled) return;
      onInput?.(event);
      const code = `Mouse${event.button}`;
      this.press(code);
      this.down.add(code);
    });
    window.addEventListener("mouseup", (event) => {
      if (this.locked && this.enabled) onInput?.(event);
      this.down.delete(`Mouse${event.button}`);
    });
    window.addEventListener(
      "wheel",
      (event) => {
        if (!this.locked || !this.enabled || event.deltaY === 0) return;
        onInput?.(event);
        this.press(event.deltaY > 0 ? "WheelDown" : "WheelUp");
      },
      { passive: true },
    );
    // Listeners hear whether the mouse is captured, and whether a release
    // was the game's own (release()) rather than the browser's (Esc, or the
    // page losing focus), which pauses the game.
    document.addEventListener("pointerlockchange", () => {
      this.locked = document.pointerLockElement === element;
      const requested = !this.locked && this.releasing;
      if (!this.locked) {
        this.releasing = false;
        this.clear();
      }
      for (const listener of this.lockListeners) listener(this.locked, requested);
    });
    document.addEventListener("mousemove", (event) => {
      if (this.locked && this.enabled) {
        onInput?.(event);
        if (this.looks.length >= 2048) { this.clear(); throw new Error("mouse command queue overflow"); }
        this.looks.push([event.movementX, event.movementY]);
        this.onLook(event.movementX, event.movementY);
      }
    });
  }

  // The 2004 game read DirectInput's raw mouse counts, with no pointer speed
  // or acceleration. Unadjusted movement is the browser's equivalent where it
  // has one. Resolves whether the browser took the request (Chrome refuses
  // one without a user gesture, or too soon after Esc released the mouse).
  async lock() {
    try {
      await this.element.requestPointerLock({ unadjustedMovement: true });
      return true;
    } catch {
      try {
        await this.element.requestPointerLock();
        return true;
      } catch {
        // The next click locks.
        return false;
      }
    }
  }

  // Free the mouse, as the game's own choice (a screen that wants it).
  release() {
    if (typeof document === "undefined" || document.pointerLockElement !== this.element) return;
    this.releasing = true;
    document.exitPointerLock();
  }

  onLockChange(listener) {
    this.lockListeners.add(listener);
  }

  // The player's own keys (playerBindings), from the next key on.
  setBindings(own) {
    this.keys = keyMap(playerBindings(own));
  }

  // Holds queue a press too, so a click shorter than a frame still fires.
  press(code) {
    const scheme = this.flightScheme?.();
    const actions = scheme ? Object.keys(scheme).filter(name => !FLIGHT_HELD.has(name) && scheme[name].includes(code)) : this.keys.actions.get(code);
    if (actions) for (const name of actions) this.queued.add(name);
  }

  // A queued press, taken once.
  take(name) {
    const had = this.queued.has(name);
    this.queued.delete(name);
    return had;
  }

  held(name) {
    const codes = this.flightScheme?.()?.[name] || this.keys.tables.holds[name];
    return codes ? Boolean(anyDown(this.down, codes)) : false;
  }

  axis(name) {
    const [plus, minus] = this.keys.tables.axes[name];
    return anyDown(this.down, plus) - anyDown(this.down, minus);
  }

  clear() {
    this.down.clear();
    this.queued.clear();
    this.looks.length = 0;
    this.onClear?.();
  }

  // The held keys go as the world reads them (worldKeys), against the
  // craft's keys while flying one.
  command(tick, epoch, generation) {
    const scheme = this.flightScheme?.();
    const command = { tick, epoch, generation, sequence: ++this.sequence,
      down: worldKeys(this.down, scheme ? flightKeys(scheme) : this.keys), actions: [...this.queued], look: this.looks };
    if (this.human.length) command.human = this.human.splice(0);
    this.queued.clear(); this.looks = [];
    return this.decorateCommand?.(command) || command;
  }

  // Keyboard sugar for scripted tests: hold these key codes until cleared.
  hold(...codes) {
    for (const code of codes) this.down.add(code);
  }
}
// Until setBindings, the tables' own keys.
Controls.prototype.keys = keyMap(BINDINGS);
