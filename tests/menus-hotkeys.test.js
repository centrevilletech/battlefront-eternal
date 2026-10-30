import test from "node:test";
import assert from "node:assert/strict";
import { BINDINGS } from "../src/input.js";
import { TABS, hotkeyRows, keyName, keysText, bindingCodes } from "../src/menus/hotkeys.js";

test("keys and buttons in the original's names", () => {
  assert.equal(keyName("KeyW"), "W");
  assert.equal(keyName("Digit4"), "4");
  assert.equal(keyName("Space"), "SPACE");
  assert.equal(keyName("ArrowUp"), "UP");
  assert.equal(keyName("Mouse0"), "MOUSE BUTTON 1");
  assert.equal(keyName("Mouse2"), "MOUSE BUTTON 2");
  assert.equal(keyName("Mouse1"), "MOUSE BUTTON 3");
  assert.equal(keyName("WheelUp"), "MOUSE WHEEL UP");
  assert.equal(keyName("F3"), "F3");
  assert.equal(keysText(["ShiftLeft", "ShiftRight"]), "SHIFT", "left and right are one key");
  assert.equal(keysText(["KeyQ", "Mouse1"]), "Q, MOUSE BUTTON 3");
});

test("the list is built from src/input.js's bindings", () => {
  const infantry = hotkeyRows("infantry");
  const row = (action) => infantry.find((r) => r.action === `${action}:`);
  assert.equal(row("PRIMARY FIRE").keys, "MOUSE BUTTON 1");
  assert.equal(row("GRENADE / SECONDARY FIRE").keys, "MOUSE BUTTON 2, ALT");
  assert.equal(row("SECONDARY NEXT").keys, "G");
  assert.ok(infantry.indexOf(row("SECONDARY NEXT")) < 5, "secondary switching is visible without scrolling");
  assert.equal(row("JUMP").keys, "SPACE");
  assert.equal(row("CROUCH").keys, "C, CONTROL");
  assert.equal(row("PRONE").keys, "X");
  assert.equal(row("RELOAD").keys, "R, RETURN");
  assert.equal(row("ZOOM").keys, "Z, MOUSE BUTTON 3");
  assert.equal(row("MOVE FORWARD").keys, "W, UP");
  assert.equal(row("MOVE LEFT").keys, "A, LEFT");
  assert.equal(row("TURN RIGHT").keys, "");
  assert.equal(row("1ST/3RD VIEW").keys, "Q, MOUSE BUTTON 4");
  assert.equal(row("SHOW MAP").keys, "M");
  // The 2004 game has no key to move between command posts.
  assert.equal(row("NEXT COMMAND POST"), undefined);
  assert.equal(row("SQUAD CMD-MOVE OUT").keys, "F3");
  // Every press and hold the game binds is listed.
  const listed = new Set(infantry.filter((r) => r.binding).map((r) => r.binding));
  for (const name of Object.keys(BINDINGS.presses)) assert.ok(listed.has(`presses.${name}`), `presses.${name} listed`);
  for (const name of Object.keys(BINDINGS.holds)) assert.ok(listed.has(`holds.${name}`), `holds.${name} listed`);
  // The keys the game reads directly are listed as they are.
  assert.equal(row("PAUSE MENU").keys, "ESC");
  assert.equal(row("PLAYER LIST").keys, "TAB");
  assert.equal(row("PAUSE MENU").binding, null);
});

test("a change to the bindings shows (the rebinding seam)", () => {
  const bindings = { axes: BINDINGS.axes, holds: BINDINGS.holds, presses: { ...BINDINGS.presses, jump: ["KeyJ", "Space"] } };
  const jump = hotkeyRows("infantry", { bindings }).find((r) => r.action === "JUMP:");
  assert.equal(jump.keys, "J, SPACE");
  assert.equal(jump.binding, "presses.jump");
  assert.deepEqual(bindingCodes("axes.forward-", bindings), ["KeyS", "ArrowDown"]);
});

test("each tab: infantry, vehicle, starfighter", () => {
  assert.deepEqual(TABS.map((tab) => tab.id), ["infantry", "vehicle", "starfighter"]);
  const vehicle = hotkeyRows("vehicle");
  assert.equal(vehicle.find((r) => r.action === "BRAKE:").keys, "SPACE");
  assert.equal(vehicle.find((r) => r.action === "NEXT POSITION:").keys, "F, MOUSE WHEEL UP");
  const flyer = hotkeyRows("starfighter");
  assert.equal(flyer.find((r) => r.action === "TAKE OFF / LAND:").keys, "SPACE");
  assert.equal(flyer.find((r) => r.action === "BRAKE:").keys, "S");
  // The original's names, where it had the string.
  const named = hotkeyRows("infantry", { strings: { "ifs.controls.General.Jump": "SAUT" } });
  assert.equal(named.find((r) => r.binding === "presses.jump").action, "SAUT:");
});
