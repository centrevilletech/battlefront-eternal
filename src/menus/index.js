// The menu framework: the 2004 game's widgets and screens, drawn by a
// theme (Classic now) over the interface kit (src/interface). The pause
// menu (src/pause) and the front end (src/shell) are built from it.
// docs/menus.md is the guide.
export { ScreenStack, PULSE } from "./stack.js";
export { MenuScreen, FADE_IN } from "./screen.js";
export { Widget, Rows, Options, OptionRow, Button, Text, Panel, List, Tabs, HorizontalButtons, DOUBLE_CLICK } from "./widgets.js";
export { confirmPopup, okPopup, keyPopup } from "./popup.js";
export { bindMenuInput } from "./input.js";
export { SOUNDS, menuSound } from "./sounds.js";
export { OPTIONS, PAGES, OPTION_KEYS, NOTES, optionsFor, optionEntry, optionRow, applies, resetPage } from "./options.js";
export { optionsMenu, gamePage, videoPage, audioPage, controlsPage } from "./option-pages.js";
export { TABS as HOTKEY_TABS, hotkeyRows, keyName, keysText, bindingCodes } from "./hotkeys.js";
export { ClassicTheme, PAINTERS, OPTION_LOOKS, rowsShift, rowBars } from "./themes/classic/index.js";
