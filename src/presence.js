// What the page tells the desktop app for the player's activity on
// Discord (electron/presence; docs/presence.md): the front end's screen, a
// battle's start and end, the side the player fights for and, online, how
// many players are in. The desktop app already knows the battle and the
// server from the page's URL, and checks every report. Outside the desktop
// client this does nothing.

export function reportPresence(report) {
  try {
    globalThis.bfeDesktop?.presence?.update(report);
  } catch {
    // The activity is never worth an error in the game.
  }
}

// A battle's reports, from its spawn port (the player's team and each
// team's side, as it changes and every `every` ms: a battle in the world
// worker may not raise the page's events) and, in a network game, the
// scoreboard port's attached players. Returns a stop function.
export function followBattle({ spawnPort, scoreboardPort, every = 5000, report = reportPresence, setTimer = setInterval, clearTimer = clearInterval }) {
  let side, players;
  const sideNow = () => {
    const team = spawnPort?.humanState?.()?.team;
    const value = spawnPort?.teams?.()?.[team]?.side ?? null;
    if (value === side) return;
    side = value;
    report({ side });
  };
  const playersNow = () => {
    const list = scoreboardPort?.players?.();
    if (!Array.isArray(list) || list.length === players) return;
    players = list.length;
    report({ players });
  };
  report({ phase: "playing" });
  sideNow();
  playersNow();
  const offs = ["teamChanged", "spawned"].map((event) => spawnPort?.on?.(event, sideNow));
  const timer = setTimer(() => { sideNow(); playersNow(); }, every);
  return () => {
    clearTimer(timer);
    for (const off of offs) if (typeof off === "function") off();
  };
}
