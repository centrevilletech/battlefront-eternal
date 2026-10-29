import { FLIGHT_BINDINGS, flightBindings } from '../flight/controls.js';
import { BINDINGS } from "../input.js";

// The CONTROL OPTIONS page's bindings list, in place of the old line of
// directions at the bottom of the screen: one tab each for INFANTRY,
// VEHICLE and STARFIGHTER, each a list of "ACTION:" and its keys, several
// joined with ", " in the original's key names.
//
// A row is { id, action, keys, binding }: binding names the entry of
// src/input.js's tables it shows ("presses.jump", "holds.firePrimary",
// "axes.forward+"; on the STARFIGHTER tab a flight control's name), so it
// follows them, and is what the CONTROL page rebinds (the original's PRESS
// NEW KEY, ESC TO CANCEL: option-pages.js); rows without one are keys the
// game reads directly (vehicles, the jetpack, the screens'), listed as they
// are.

export const TABS = [
  { id: "infantry", label: "INFANTRY", labelKey: "ifs.Controls.SoldierTab" },
  { id: "vehicle", label: "VEHICLE", labelKey: "ifs.Controls.VehicleTab" },
  { id: "starfighter", label: "STARFIGHTER", labelKey: "ifs.Controls.FlyerTab" },
];

// The original's names for keys and buttons.
const NAMES = {
  Numpad0: "KP_INSERT", Space: "SPACE", Enter: "RETURN", NumpadEnter: "KP_ENTER", Escape: "ESC", Tab: "TAB", Backspace: "BACKSPACE",
  ShiftLeft: "SHIFT", ShiftRight: "SHIFT", ControlLeft: "CONTROL", ControlRight: "CONTROL", AltLeft: "ALT", AltRight: "ALT",
  ArrowUp: "UP", ArrowDown: "DOWN", ArrowLeft: "LEFT", ArrowRight: "RIGHT",
  Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'",
  Comma: ",", Period: ".", Slash: "/", Insert: "INSERT", Delete: "DELETE", Home: "HOME", End: "END", PageUp: "PAGE UP", PageDown: "PAGE DOWN",
  Mouse0: "MOUSE BUTTON 1", Mouse2: "MOUSE BUTTON 2", Mouse1: "MOUSE BUTTON 3", Mouse3: "MOUSE BUTTON 4", Mouse4: "MOUSE BUTTON 5",
  WheelUp: "MOUSE WHEEL UP", WheelDown: "MOUSE WHEEL DOWN",
};

// A key code (KeyboardEvent.code, or Mouse0 / WheelUp...) as the
// original names it.
export function keyName(code) {
  if (NAMES[code]) return NAMES[code];
  const match = /^(?:Key|Digit)(.)$/.exec(code) || /^Numpad(\d)$/.exec(code);
  if (match) return code.startsWith("Numpad") ? `KP_${match[1]}` : match[1];
  return String(code).toUpperCase();
}

// Codes as one text: each name once (left and right Shift are one SHIFT),
// joined with ", ".
export function keysText(codes) {
  return [...new Set(codes.map(keyName))].join(", ");
}

// Each tab's rows, in order: [action, the original's string key, keys]. A
// binding is "presses.jump", "holds.firePrimary" or "axes.forward+" /
// "axes.forward-"; anything else is a fixed key, as written.
const ROWS = {
  infantry: [
    ["PRIMARY FIRE", "ifs.controls.General.Primary", "holds.firePrimary"],
    ["GRENADE / SECONDARY FIRE", "bfe.controls.GrenadeSecondaryFire", "holds.fireSecondary"],
    ["SECONDARY NEXT", "ifs.controls.General.SecondaryNext", "presses.nextSecondary"],
    ["PRIMARY NEXT", "ifs.controls.General.PrimaryNext", "presses.nextPrimary"],
    ["PRIMARY PREVIOUS", null, "presses.previousPrimary"],
    ["JUMP", "ifs.controls.General.Jump", "presses.jump"],
    ["DIVE", null, "SPACE WITH A OR D"],
    ["JETPACK", null, "SPACE IN THE AIR"],
    ["CROUCH", "ifs.controls.General.Crouch", "presses.crouch"],
    ["PRONE", null, "presses.prone"],
    ["ZOOM", "ifs.controls.General.Zoom", "presses.zoom"],
    ["1ST/3RD VIEW", "ifs.controls.General.FirstThirdPerson", "presses.perspective"],
    ["RELOAD", "ifs.controls.General.Reload", "presses.reload"],
    ["ENTER/EXIT", null, "presses.use"],
    ["SQUAD CMD-MOVE OUT", null, "presses.squadMoveOut"],
    ["SQUAD CMD-HOLD", null, "presses.squadHold"],
    ["SQUAD CMD-FOLLOW", null, "presses.squadFollow"],
    ["SQUAD CMD-AT EASE", null, "presses.squadAtEase"],
    ["MOVE FORWARD", null, "axes.forward+"],
    ["MOVE BACK", null, "axes.forward-"],
    ["MOVE LEFT", null, "axes.right-"],
    ["MOVE RIGHT", null, "axes.right+"],
    ["TURN LEFT", null, "axes.turn-"],
    ["TURN RIGHT", null, "axes.turn+"],
    ["LOOK LEFT/RIGHT AXIS", null, "MOUSE X AXIS"],
    ["LOOK UP/DOWN AXIS", null, "MOUSE Y AXIS"],
    ["SHOW MAP", null, "presses.map"],
    ["NEXT COMMAND POST", null, "presses.nextSpawn"],
    ["PLAYER LIST", "ifs.controls.playerlist", "TAB"],
    ["CHAT", null, "T"],
    ["TEAM CHAT", null, "Y"],
    ["PAUSE MENU", "ifs.controls.General.Pause", "ESC"],
    ["STATS LINE", null, "presses.stats"],
  ],
  vehicle: [
    ["PRIMARY FIRE", "ifs.controls.General.Primary", "holds.firePrimary"],
    ["SECONDARY FIRE", "ifs.controls.General.Secondary", "holds.fireSecondary"],
    ["ACCELERATE", null, "axes.forward+"],
    ["REVERSE", null, "axes.forward-"],
    ["STRAFE LEFT", null, "axes.right-"],
    ["STRAFE RIGHT", null, "axes.right+"],
    ["TURN LEFT", null, "axes.turn-"],
    ["TURN RIGHT", null, "axes.turn+"],
    ["AIM", null, "MOUSE"],
    ["BRAKE", null, "SPACE"],
    ["NEXT POSITION", "ifs.controls.General.NextPosition", "presses.nextPrimary"],
    ["ENTER/EXIT", null, "presses.use"],
    ["RELOAD", "ifs.controls.General.Reload", "presses.reload"],
    ["1ST/3RD VIEW", "ifs.controls.General.FirstThirdPerson", "presses.perspective"],
    ["SHOW MAP", null, "presses.map"],
    ["PLAYER LIST", "ifs.controls.playerlist", "TAB"],
    ["CHAT", null, "T"],
    ["TEAM CHAT", null, "Y"],
    ["PAUSE MENU", "ifs.controls.General.Pause", "ESC"],
  ],
  starfighter: [],
};

// A binding's codes, from the tables (src/input.js's BINDINGS shape).
export function bindingCodes(binding, bindings = BINDINGS) {
  const [table, name] = binding.split(".");
  if (table === "axes") {
    const axis = bindings.axes?.[name.slice(0, -1)];
    return axis ? axis[name.endsWith("+") ? 0 : 1] : [];
  }
  return bindings[table]?.[name] || [];
}

// A tab's rows: { id, action ("PRIMARY FIRE:"), keys ("MOUSE BUTTON 1"),
// binding (null for a fixed key) }. strings: localized labels where the
// original had them.
export function hotkeyRows(tab, { bindings = BINDINGS, strings = {}, flight = {} } = {}) {
  if (tab === 'starfighter') return Object.entries(flightBindings(flight)).map(([name,codes]) => ({id:name,action:(name === 'takeoff' ? 'TAKE OFF / LAND' : name.replace(/([A-Z])/g,' $1').toUpperCase()) + ':',keys:keysText(codes),binding:name}));
  return (ROWS[tab] || []).map(([label, key, source]) => {
    const bound = /^(presses|holds|axes)\./.test(source);
    const action = String((key && strings[key]) || label).trim();
    return {
      id: `${tab}:${label}`,
      action: `${action}:`,
      keys: bound ? keysText(bindingCodes(source, bindings)) : source,
      binding: bound ? source : null,
    };
  });
}
