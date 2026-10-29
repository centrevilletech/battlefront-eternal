import { MenuScreen } from "./screen.js";
import { HorizontalButtons, Panel, Text } from "./widgets.js";

// The YES / NO popup (the original's Popup_YesNo):
// a window centred on the screen with the question over YES and NO, NO
// focused. YES plays shell_menu_accept and NO (or Esc) shell_menu_cancel;
// either closes it, then calls onYes or onNo. Opening it is silent. The
// screen under it keeps its title and corner buttons and hides its rows.
//
// strings: the localized strings (common.yes and common.no).
export function confirmPopup({ id = "confirm", message, onYes = null, onNo = null, strings = {}, focus = "no" } = {}) {
  let screen = null;
  const close = () => {
    if (screen.stack?.top === screen) screen.stack.pop();
  };
  const buttons = new HorizontalButtons({
    id: "answer",
    buttons: [
      { id: "yes", label: strings["common.yes"] || "YES", sound: "accept", action: () => { close(); onYes?.(); } },
      { id: "no", label: strings["common.no"] || "NO", sound: "cancel", action: () => { close(); onNo?.(); } },
    ],
  });
  screen = new MenuScreen({
    id,
    popup: true,
    widgets: [new Panel({ id: "window", look: "popup" }), new Text({ id: "message", look: "popup", text: message }), buttons],
    back: (self) => {
      self.sound("cancel");
      close();
      onNo?.();
    },
    onEnter: () => {
      screen.focused = { widget: buttons, item: focus === "yes" ? 0 : 1 };
    },
  });
  return screen;
}

// The CONTROL page's PRESS NEW KEY, ESC TO CANCEL (the original's
// ifs.GameOpt.pc_keybindpopup): a window with the action over the prompt.
// The next key, mouse button or wheel notch a binding can take (accepts)
// closes it and goes to onKey(code), as Mouse0 / WheelUp...; Esc closes it
// with nothing. Keys come through the stack as every menu key does, and
// none reaches the page meanwhile (grabsKeys: bindMenuInput). The mouse is
// heard on target (the window) ahead of the menus' own listeners while it
// is open, and the rest of a click it took goes nowhere.
export function keyPopup({ id = "rebind", action = "", onKey = null, accepts = () => true, strings = {}, target = globalThis.window ?? null } = {}) {
  let screen = null, unlisten = null;
  const take = (code) => {
    if (!accepts(code)) return false;
    screen.sound("accept");
    if (screen.stack?.top === screen) screen.stack.pop();
    onKey?.(code);
    return true;
  };
  screen = new MenuScreen({
    id,
    popup: true,
    grabsKeys: true,
    // Wide enough for the prompt on one line (313 units of gamefont_medium).
    widgets: [
      new Panel({ id: "window", look: "popup", size: [360, 180] }),
      new Text({ id: "message", look: "popup", width: 330, text: [action, strings["ifs.GameOpt.pc_keybindpopup"] || "PRESS NEW KEY, ESC TO CANCEL"] }),
    ],
    back: (self) => {
      self.sound("cancel");
      if (self.stack?.top === self) self.stack.pop();
    },
    // Esc goes on to back(); any other key is the popup's, taken or not.
    onKey: (code) => code !== "Escape" && (take(code), true),
    onEnter: () => {
      unlisten ??= listenMouse(target, take);
    },
    onExit: () => {
      unlisten?.();
      unlisten = null;
    },
  });
  return screen;
}

// The mouse's buttons and wheel on target, in the capture phase, for
// take(code); returns what stops it. A button taken has its release, click
// and context menu swallowed too, or the page under the popup would act on
// them (a click on BACK, the browser's menu on the right button).
function listenMouse(target, take) {
  if (!target?.addEventListener) return () => {};
  const stop = (event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const down = (event) => {
    stop(event);
    if (take(`Mouse${event.button}`)) swallowClick(target, event.button);
  };
  const wheel = (event) => {
    stop(event);
    if (event.deltaY) take(event.deltaY > 0 ? "WheelDown" : "WheelUp");
  };
  target.addEventListener("mousedown", down, CAPTURE);
  target.addEventListener("wheel", wheel, { ...CAPTURE, passive: false });
  return () => {
    target.removeEventListener("mousedown", down, CAPTURE);
    target.removeEventListener("wheel", wheel, CAPTURE);
  };
}

// As an object: Node's EventTarget (the tests') removes no listener given
// a bare true.
const CAPTURE = { capture: true };
const CLICK_EVENTS = ["mouseup", "click", "auxclick", "dblclick", "contextmenu"];
function swallowClick(target, button) {
  let timer = null;
  const done = () => {
    clearTimeout(timer);
    for (const type of CLICK_EVENTS) target.removeEventListener(type, swallow, CAPTURE);
  };
  const swallow = (event) => {
    if (event.button !== button) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    // The click and the menu follow the release at once.
    if (event.type === "mouseup") {
      clearTimeout(timer);
      timer = setTimeout(done, 100);
    }
  };
  for (const type of CLICK_EVENTS) target.addEventListener(type, swallow, CAPTURE);
  // Released where the page never hears it.
  timer = setTimeout(done, 5000);
}

// The OK popup (the original's Popup_Ok): a 260 x 180 window with its
// message (in a box 220 wide) over OK. OK or Esc closes it, then calls
// onOk. ("SELECTED MAPLIST IS FULL.", "NO NETWORK ADAPTOR DETECTED...")
export function okPopup({ id = "ok", message, onOk = null, strings = {} } = {}) {
  let screen = null;
  const close = () => {
    if (screen.stack?.top === screen) screen.stack.pop();
  };
  const button = new HorizontalButtons({
    id: "answer",
    y: 40,
    buttons: [{ id: "ok", label: strings["common.ok"] || "OK", sound: "accept", action: () => { close(); onOk?.(); } }],
  });
  screen = new MenuScreen({
    id,
    popup: true,
    widgets: [new Panel({ id: "window", look: "popup", size: [260, 180] }), new Text({ id: "message", look: "popup", width: 220, text: message }), button],
    back: (self) => {
      self.sound("accept");
      close();
      onOk?.();
    },
  });
  return screen;
}
