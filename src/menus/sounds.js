import { emitSound } from "../audio/events.js";

// The interface sounds the menus play (core.lvl's, on the shellfx bus),
// through the game's sound events: a battle's audio plays them from its
// bank, the front end's from its own.
export const SOUNDS = {
  select: "shell_select_change", // focus moves: keys, or the pointer onto a new item
  accept: "shell_menu_accept", // an item chosen, a value changed, a popup's YES
  enter: "shell_menu_enter",
  cancel: "shell_menu_cancel", // a popup's NO
  error: "shell_menu_error", // an action that can't be done
  exit: "shell_menu_exit", // coming back to a screen
};

// A sound by its kind (or its own name).
export function menuSound(kind) {
  emitSound("interface", SOUNDS[kind] || kind);
}
