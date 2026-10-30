// The page theme's three looks (index.js) and the player's pick of one (the
// menuTheme setting), on their own: the loading screen and its worker read
// the pick without the page and its styles.

export const THEMES = [
  { id: "bf1", label: "Battlefront (2004)" },
  { id: "bf2", label: "Battlefront II (2005)" },
  { id: "eternal", label: "Eternal" },
];
export const DEFAULT_THEME = "eternal";
export const themeOf = (value) => (THEMES.some((theme) => theme.id === value) ? value : DEFAULT_THEME);
