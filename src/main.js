import { isBattle } from "./launch.js";
import { createLoadingScreen } from "./shell/loading.js";

// The page is a battle when its URL names one (map, side, unit, spawn or
// spawnscreen), else the front end; ?shell=1 or ?shell=0 forces either
// (src/launch.js, docs/shell.md). Each loads only its own code.
if (isBattle(location.search)) startBattle();
else startShell();

function failed(what, error) {
  document.querySelector("#hud").textContent = `Failed to load ${what}: ${error.message}`;
  console.error(error);
}

function startBattle() {
  // The original's loading screen covers the page until the battle is ready
  // (window.__loading for checks).
  const loading = (window.__loading = createLoadingScreen({ query: location.search }));
  import("./game.js")
    .then(async ({ createGame }) => {
      const game = await createGame({ query: location.search, onProgress: loading.progress });
      loading.done(game);
    })
    .catch((error) => {
      loading.close();
      failed("game", error);
    });
}

function startShell() {
  import("./shell/index.js")
    .then(({ createShell }) => createShell({ query: location.search }))
    .catch((error) => failed("the front end", error));
}
