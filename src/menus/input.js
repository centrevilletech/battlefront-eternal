// The page's events into a ScreenStack: keys on the window, the pointer and
// buttons over the canvas, the wheel. The menus' keys: Up / Down move the
// focus, Left / Right change a value, Enter accepts, Esc goes back,
// Page Up / Page Down page a list.
//
// stack: the ScreenStack; element: the canvas; toInterface(x, y): canvas
// pixels to interface units; scale(): interface units per pixel ({ x, y },
// for a captured mouse, which only moves); isActive(): whether the menus
// take input now. exclusive: the menus own the keyboard while active (the
// pause menu over a battle: nothing under it sees a key go down).
// passKey(event): a key to leave alone even so (Esc while the audio panel
// is open). pointer: false leaves the pointer and the wheel to the page
// (the page theme's own elements take them). A top screen that grabsKeys
// (the CONTROL page's PRESS NEW KEY) keeps every key from the browser too.
// Returns a function that unbinds it all.

const SWALLOW = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "NumpadEnter", "PageUp", "PageDown", "Space", "Tab", "Escape", "Backspace"]);
const REPEATS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown"]);

export function bindMenuInput({ stack, element, toInterface, scale = null, isActive = () => true, exclusive = false, passKey = () => false, target = window, pointer: withPointer = true }) {
  const listeners = [];
  const listen = (on, type, handler, options) => {
    on.addEventListener(type, handler, options);
    listeners.push(() => on.removeEventListener(type, handler, options));
  };
  const local = (event) => {
    const rect = element.getBoundingClientRect();
    return toInterface(event.clientX - rect.left, event.clientY - rect.top);
  };
  const locked = () => typeof document !== "undefined" && document.pointerLockElement === element;

  listen(target, "keydown", (event) => {
    if (!isActive() || passKey(event)) return;
    if (exclusive) event.stopImmediatePropagation();
    if (SWALLOW.has(event.code) || stack.top?.grabsKeys) event.preventDefault();
    if (event.repeat && !REPEATS.has(event.code)) return;
    stack.key(event.code);
  }, { capture: exclusive });
  // Key releases go on to everything: what tracks held keys underneath
  // (the spawn screen's Space) must hear them.
  if (!withPointer) return () => listeners.forEach((remove) => remove());
  const pointer = typeof document !== "undefined" ? document : element;
  listen(pointer, "mousemove", (event) => {
    if (!isActive()) return;
    // The browser sends a move that didn't move when it lets go of a
    // captured mouse (at the window's corner): not the player's.
    if (event.movementX === 0 && event.movementY === 0) return;
    // A button let go outside the page leaves no mouseup.
    if (stack.pointer.held && event.buttons === 0) stack.pointerUp();
    if (locked()) {
      const k = scale?.() || { x: 1, y: 1 };
      stack.pointerMove(stack.pointer.x + event.movementX * k.x, stack.pointer.y + event.movementY * k.y);
    } else if (event.target === element || element.contains?.(event.target)) {
      const point = local(event);
      stack.pointerMove(point.x, point.y);
    }
  });
  listen(pointer, "mousedown", (event) => {
    if (!isActive()) return;
    if (!locked()) {
      if (event.target !== element && !element.contains?.(event.target)) return;
      const point = local(event);
      stack.pointerMove(point.x, point.y);
    }
    if (exclusive) event.stopImmediatePropagation();
    if (stack.pointerDown(event.button)) event.preventDefault();
  }, { capture: exclusive });
  listen(pointer, "mouseup", () => stack.pointerUp());
  listen(target, "wheel", (event) => {
    if (isActive()) stack.wheel(event.deltaY);
  }, { passive: true });
  return () => listeners.forEach((remove) => remove());
}
