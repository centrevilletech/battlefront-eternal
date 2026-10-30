// The player's look on the desktop app's own pages (prepare.html), set on
// <html data-bfe-theme> before anything is drawn: a classic script in the
// page's head, so the first frame is already in it.
//
// The look is the menuTheme setting, read as the front end reads its
// settings (src/settings.js): the file the desktop app keeps
// (electron/player-settings.js, through the preload's settingsStore) when
// complete, else localStorage's with the file's changes laid over them.
// Nothing is written here. The looks are the page theme's
// (src/menus/themes/page THEMES), Eternal when none is set.
(() => {
  const LOOKS = ["bf1", "bf2", "eternal"];
  const DEFAULT_LOOK = "eternal";
  const KEY = "battlefront-eternal.settings";
  const LEGACY_KEY = "battlefront-classic.settings";

  function local() {
    try {
      const stored = JSON.parse(localStorage.getItem(KEY) ?? localStorage.getItem(LEGACY_KEY) ?? "{}");
      return stored && typeof stored === "object" ? stored : {};
    } catch {
      return {};
    }
  }

  function settings() {
    let record = null;
    try {
      const read = window.bfeDesktop?.settingsStore?.read();
      record = read?.record && typeof read.record.values === "object" ? read.record : null;
    } catch {
      record = null;
    }
    return (record?.complete ? record.values : { ...local(), ...(record?.values || {}) }) || {};
  }

  const look = settings().menuTheme;
  document.documentElement.dataset.bfeTheme = LOOKS.includes(look) ? look : DEFAULT_LOOK;
})();
