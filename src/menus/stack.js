import { menuSound } from "./sounds.js";

// The screens the player has gone through, the top one live: pushing enters
// a screen forward, popping goes back to the one under it, which comes up
// again with shell_menu_exit. A popup (a screen with popup: true) is drawn
// over the screen under it, which keeps its title and corner buttons but
// hides its rows and lists, and takes all the input until it closes.
//
// The stack also keeps what every screen shares: the focused item's pulse
// (one clock for all, as the original's gButton_CurSizeAdd), the pointer,
// and the hotspots the theme drew, which the pointer is tested against.

// The focus pulse: alpha runs 0.3 -> 1 -> 0.3 at 2 a second (0.7 s a
// cycle), starting at 0.5 on its way up.
export const PULSE = { low: 0.3, high: 1, rate: 2, start: 0.5 };

export class ScreenStack {
  // theme: draws the screens and hit-tests them (themes/classic);
  // sound(kind): plays an interface sound (sounds.js); onEmpty(): the last
  // screen was popped.
  constructor({ theme = null, sound = menuSound, onEmpty = null } = {}) {
    this.theme = theme;
    this.playSound = sound;
    this.onEmpty = onEmpty;
    this.screens = [];
    this.pulse = PULSE.start;
    this.pulseDirection = PULSE.rate;
    this.time = 0;
    this.pointer = { x: 512, y: 384, visible: false, held: false };
    this.hotspots = [];
  }

  get top() {
    return this.screens.at(-1) || null;
  }

  get isEmpty() {
    return this.screens.length === 0;
  }

  // The screens drawn: the top one, and the one under a popup.
  get visible() {
    const top = this.top;
    if (!top) return [];
    if (!top.popup) return [top];
    const under = this.screens.at(-2);
    return under ? [under, top] : [top];
  }

  // Push a screen, entering it forward.
  push(screen) {
    screen.stack = this;
    this.screens.push(screen);
    screen.enter({ forward: true });
    return screen;
  }

  // Back to the screen under the top one, which comes up again (unless it
  // was under a popup, where it never went away).
  pop() {
    const screen = this.screens.pop();
    if (!screen) return null;
    screen.exit();
    this.pointer.held = false;
    const top = this.top;
    if (!top) {
      this.onEmpty?.();
      return screen;
    }
    if (!screen.popup) {
      top.enter({ forward: false });
      this.sound("exit");
    }
    return screen;
  }

  // The top screen swapped for another, entered forward.
  replace(screen) {
    const old = this.screens.pop();
    old?.exit();
    return this.push(screen);
  }

  // Every screen off, without sounds or hooks beyond their exits.
  clear() {
    while (this.screens.length) this.screens.pop().exit();
    this.pointer.held = false;
  }

  sound(kind) {
    this.playSound?.(kind);
  }

  update(dt) {
    this.time += dt;
    // The pulse, as the original steps it each frame.
    this.pulse += dt * this.pulseDirection;
    if (this.pulse > PULSE.high) {
      this.pulse = PULSE.high;
      this.pulseDirection = -Math.abs(this.pulseDirection);
    } else if (this.pulse < PULSE.low) {
      this.pulse = PULSE.low;
      this.pulseDirection = Math.abs(this.pulseDirection);
    }
    for (const screen of this.visible) screen.update(dt);
  }

  // --- input -------------------------------------------------------------

  // A key went down. Returns whether a screen used it.
  key(code) {
    const top = this.top;
    return top ? top.key(code) : false;
  }

  // The hotspot under an interface point, the top screen's only.
  hit(x, y) {
    const top = this.top;
    if (!top) return null;
    return this.theme?.hit?.(this, x, y) ?? null;
  }

  // The pointer moved to (x, y) in interface units: the focus goes to
  // what is under it, or nowhere; a held pointer drags (a slider).
  pointerMove(x, y) {
    this.pointer.x = x;
    this.pointer.y = y;
    this.pointer.visible = true;
    const top = this.top;
    if (!top) return;
    if (this.pointer.held && this.dragging) {
      this.dragging.widget.drag?.(this.dragging, x, y, top);
      return;
    }
    const target = this.hit(x, y);
    top.setFocus(target, "pointer");
    target?.widget.hover?.(target, top);
    if (!target) for (const widget of top.widgets) widget.hover?.(null, top);
  }

  // A button went down at the pointer (0: the left, the one that acts).
  pointerDown(button = 0) {
    const top = this.top;
    if (!top || button !== 0) return false;
    const target = this.hit(this.pointer.x, this.pointer.y);
    this.pointer.held = true;
    this.dragging = null;
    if (!target) return false;
    top.setFocus(target, "pointer");
    if (target.widget.press) {
      target.widget.press(target, top);
      this.dragging = target;
    } else {
      top.accept(target);
    }
    return true;
  }

  pointerUp() {
    this.pointer.held = false;
    this.dragging = null;
  }

  // The wheel, over whatever is under the pointer (else the focused item):
  // a list moves its selection a step a notch.
  wheel(deltaY) {
    const top = this.top;
    if (!top || !deltaY) return false;
    const target = this.hit(this.pointer.x, this.pointer.y) || top.focused;
    return Boolean(target?.widget.wheel?.(Math.sign(deltaY), target, top));
  }

  // --- drawing -----------------------------------------------------------

  // The visible screens into the surface's batch (the theme records the
  // hotspots as it draws).
  draw(surface, batch) {
    this.theme?.draw(this, surface, batch);
  }
}
