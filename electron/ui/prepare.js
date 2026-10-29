// The first-run loader. The main process (electron/main.js) owns the state;
// this page draws it and sends the player's choices back.
const desktop = window.bfeDesktop;
const $ = (id) => document.getElementById(id);

const PHASES = [
  ["catalog", (key) => key === "catalog" || key === "catalog-update"],
  ["armies", (key) => key.startsWith("side:") || key === "common"],
  ["shell", (key) => key === "shell" || key === "hud" || key === "environment"],
  ["maps", (key) => key.startsWith("map:")],
  ["audio", (key) => key.startsWith("audio:")],
  ["textures", (key) => key === "textures"],
  ["movies", (key) => key === "movies"],
];
const phaseOf = (key) => PHASES.find(([, match]) => match(key))?.[0];

let view = null;
let catalog = null;
let catalogTried = false;
let artTimer = null;
let artIndex = 0;
let artMap = null;
let showLog = false;

function button(label, action, primary = false) {
  const element = document.createElement("button");
  element.textContent = label;
  if (primary) element.className = "primary";
  element.addEventListener("click", () => desktop.prepare.action(action));
  return element;
}

function setActions(list) {
  const actions = $("actions");
  const key = list.map(([label, action, primary]) => `${label}|${action}|${!!primary}`).join(",");
  if (actions.dataset.key === key) return;
  actions.dataset.key = key;
  actions.replaceChildren(...list.map(([label, action, primary]) => button(label, action, primary)));
}

function setDetail(text, code = null) {
  const detail = $("detail");
  detail.textContent = text;
  if (code) {
    const element = document.createElement("code");
    element.textContent = code;
    detail.append("\n", element);
  }
}

// 0.1.0-preview.262 reads as "preview 262".
const versionName = (version) => {
  const match = /^\d+\.\d+\.\d+-preview\.(\d+)$/.exec(version || "");
  return match ? `preview ${match[1]}` : version || "";
};
const megabytes = (bytes) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(2)} GB` : `${(bytes / 1e6).toFixed(bytes >= 1e8 ? 0 : 1)} MB`);

const clock = (seconds) => {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  const minutes = Math.round(seconds / 60);
  if (minutes < 1) return "under a minute";
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
};

// ---- The loading pictures ----

async function loadCatalog() {
  if (catalog || catalogTried) return catalog;
  catalogTried = true;
  try {
    const response = await fetch("/common/shell/shell.json", { cache: "no-store" });
    if (response.ok) catalog = await response.json();
  } catch {
    catalog = null;
  }
  if (!catalog) setTimeout(() => (catalogTried = false), 5000);
  return catalog;
}

function picturesFor(mapId) {
  const maps = catalog?.maps || [];
  const entry = maps.find((map) => map.id === mapId);
  const stages = (entry?.load?.stages || []).map((stage) => stage.image).filter(Boolean);
  if (stages.length) return { planet: entry.load.planet || "", name: entry.name || "", images: stages.slice(1).concat(stages[0]) };
  // Between maps: every level picture, a tour of the galaxy.
  const images = maps.flatMap((map) => (map.load?.stages || []).slice(-1).map((stage) => stage.image)).filter(Boolean);
  return { planet: "", name: "", images };
}

function showPicture(url) {
  const a = $("art-a");
  const b = $("art-b");
  const [front, back] = a.classList.contains("show") ? [b, a] : [a, b];
  const image = new Image();
  image.onload = () => {
    front.style.backgroundImage = `url("${url}")`;
    front.classList.add("show");
    back.classList.remove("show");
    document.body.classList.add("art-on");
  };
  image.src = url;
}

async function updateArt(mapId) {
  if (!(await loadCatalog())) return;
  if (artMap === mapId && artTimer) return;
  artMap = mapId;
  artIndex = 0;
  clearInterval(artTimer);
  const { planet, images } = picturesFor(mapId);
  $("planet").textContent = planet;
  if (!images.length) return;
  const next = () => showPicture(`/common/shell/${images[artIndex++ % images.length]}`);
  next();
  artTimer = setInterval(next, 8000);
}

// ---- Drawing the state ----

function drawProgress(snapshot, estimate) {
  $("progress").hidden = false;
  // Progress by expected time (electron/prepare.js: estimate), not by step
  // count: a map counts for its two minutes, the movies for theirs.
  const fraction = estimate?.known ? estimate.fraction : 0;
  $("fill").style.width = `${(fraction * 100).toFixed(1)}%`;
  $("progress").classList.toggle("estimating", !estimate?.known);
  const current = snapshot.current;
  const others = (snapshot.running || 0) - 1;
  $("step").textContent = current
    ? current.label.toUpperCase() + (others > 0 ? `  +${others} MORE` : "")
    : snapshot.status === "running" ? "STARTING" : "";

  if (!estimate?.known) {
    $("numbers").textContent = "";
  } else if (snapshot.status === "running") {
    const parts = [`${Math.floor(fraction * 100)}%`];
    if (estimate.elapsed >= 60) parts.push(`${clock(estimate.elapsed)} elapsed`);
    if (estimate.remaining > 0) parts.push(`about ${clock(estimate.remaining)} left`);
    $("numbers").textContent = parts.join("  ·  ");
  } else {
    $("numbers").textContent = `${Math.floor(fraction * 100)}%`;
  }

  const byPhase = new Map(PHASES.map(([name]) => [name, []]));
  for (const step of snapshot.steps) byPhase.get(phaseOf(step.key))?.push(step);
  const active = current ? phaseOf(current.key) : null;
  for (const item of $("phases").children) {
    const steps = byPhase.get(item.dataset.phase) || [];
    const finished = steps.length && steps.every((step) => ["done", "failed", "unavailable"].includes(step.status));
    const skipped = steps.length && steps.every((step) => step.status === "unavailable");
    item.className = item.dataset.phase === active ? "active" : skipped ? "skipped" : finished ? "done" : "";
  }

  const mapId = current?.key.startsWith("map:") ? current.key.split(":")[1] : null;
  if (snapshot.steps.some((step) => step.key === "catalog" && step.status === "done")) updateArt(mapId);
}

function drawLog(snapshot) {
  $("toggle-log").hidden = !snapshot.log.length;
  $("toggle-log").textContent = showLog ? "HIDE DETAILS" : "SHOW DETAILS";
  const log = $("log");
  log.hidden = !showLog;
  if (!showLog) return;
  const atBottom = log.scrollTop + log.clientHeight >= log.scrollHeight - 8;
  log.textContent = snapshot.log.slice(-200).join("\n");
  if (atBottom) log.scrollTop = log.scrollHeight;
}

function draw() {
  if (!view) return;
  const { mode, install, snapshot } = view;
  document.body.classList.toggle("error", mode === "error" || mode === "update-error" || snapshot?.status === "error");
  document.body.classList.toggle("warn", mode === "missing" || mode === "outdated");
  document.body.classList.toggle("idle", mode !== "preparing" && mode !== "searching" && mode !== "update");

  if (mode === "update") {
    $("phases").hidden = true;
    $("toggle-log").hidden = $("log").hidden = true;
    const progress = view.progress || {};
    const name = versionName(view.version);
    if (view.phase === "checking") {
      $("title").textContent = "CHECKING FOR UPDATES";
      setDetail("");
      $("progress").hidden = true;
      setActions([]);
    } else if (view.phase === "restarting") {
      $("title").textContent = "STARTING THE NEW VERSION";
      setDetail(`Battlefront Eternal ${name} is ready.`);
      $("progress").hidden = true;
      setActions([]);
    } else {
      $("title").textContent = "UPDATING BATTLEFRONT ETERNAL";
      setDetail(`Installing ${name}.` + (view.refresh ? " Your game data is refreshed for it after the update." : ""));
      $("progress").hidden = false;
      // Unchanged files are copied from this version first, then the
      // changed ones download.
      const downloading = progress.phase === "downloading" && progress.total > 0;
      const copying = progress.phase === "copying" && progress.files > 0;
      $("progress").classList.toggle("estimating", !downloading && !copying);
      const fraction = downloading ? progress.received / progress.total : copying ? progress.done / progress.files : 0;
      $("fill").style.width = `${Math.min(100, fraction * 100).toFixed(1)}%`;
      $("step").textContent = downloading ? "DOWNLOADING" : copying ? "KEEPING UNCHANGED FILES" : progress.phase === "finishing" ? "FINISHING" : "STARTING";
      $("numbers").textContent = downloading ? `${megabytes(progress.received)} of ${megabytes(progress.total)}` : copying ? `${progress.done.toLocaleString("en-US")} of ${progress.files.toLocaleString("en-US")} files` : "";
      setActions([["SKIP FOR NOW", "update-skip"]]);
    }
  } else if (mode === "update-error") {
    $("title").textContent = "THE UPDATE DID NOT FINISH";
    setDetail(view.message || "", null);
    $("progress").hidden = true;
    $("toggle-log").hidden = $("log").hidden = true;
    setActions([["TRY AGAIN", "update-retry", true], ["PLAY WITHOUT UPDATING", "update-skip"]]);
  } else if (mode === "searching") {
    $("title").textContent = "SEARCHING FOR STAR WARS BATTLEFRONT";
    setDetail("Looking through your Steam and GOG libraries.");
    $("progress").hidden = true;
    setActions([]);
  } else if (mode === "outdated") {
    $("title").textContent = "GAME DATA NEEDS AN UPDATE";
    setDetail("This content was prepared for a different version. Install Star Wars: Battlefront (2004) from Steam or GOG, or choose its folder, to update automatically, or import a current content export from your other computer.");
    $("progress").hidden = true;
    $("phases").hidden = true;
    setActions([["CHOOSE GAME FOLDER", "choose", true], ["IMPORT CONTENT EXPORT", "import"], ["SEARCH AGAIN", "search"]]);
  } else if (mode === "missing") {
    $("title").textContent = "STAR WARS BATTLEFRONT NOT FOUND";
    setDetail(
      "Battlefront Eternal plays the battles from your own copy of Star Wars: Battlefront (2004). " +
        "Import a content export from your other computer, or install the original from Steam or GOG. " +
        "If it is installed somewhere else, choose its folder.",
    );
    $("progress").hidden = true;
    setActions([["IMPORT CONTENT EXPORT", "import", true], ["CHOOSE GAME FOLDER", "choose"], ["GET IT ON STEAM", "steam"], ["GET IT ON GOG", "gog"], ["SEARCH AGAIN", "search"]]);
  } else if (mode === "ready") {
    $("title").textContent = "YOUR GAME CONTENT";
    setDetail("Export prepared content for another computer, import an existing export, or attach the original game for future content updates.");
    $("progress").hidden = true;
    setActions([["PLAY", "play", true], ["EXPORT CONTENT", "export"], ["IMPORT CONTENT EXPORT", "import"], ["CHOOSE GAME FOLDER", "choose"]]);
  } else if (mode === "transfer") {
    $("title").textContent = view.operation === "export" ? "EXPORTING YOUR CONTENT" : view.operation === "source" ? "CHECKING ORIGINAL GAME DATA" : "IMPORTING YOUR CONTENT";
    const progress = view.progress || {};
    setDetail(progress.phase || "Starting…", progress.file || null);
    $("progress").hidden = false;
    $("phases").hidden = true;
    $("progress").classList.toggle("estimating", !progress.total);
    $("fill").style.width = `${progress.total ? Math.min(100, progress.done / progress.total * 100) : 0}%`;
    $("step").textContent = progress.phase || "";
    $("numbers").textContent = progress.total ? `${Math.floor(progress.done / progress.total * 100)}%` : "";
    $("toggle-log").hidden = $("log").hidden = true;
    setActions([["CANCEL", "cancel-transfer"]]);
  } else if (mode === "transfer-done" || mode === "transfer-error") {
    $("title").textContent = mode === "transfer-error" ? "CONTENT TRANSFER STOPPED" : view.operation === "export" ? "CONTENT EXPORTED" : "READY FOR BATTLE";
    setDetail(view.message, view.file || null);
    $("progress").hidden = true;
    const actions = [];
    if (view.hasContent) actions.push(["MAIN MENU", "play", true]);
    actions.push(["IMPORT CONTENT EXPORT", "import"]);
    if (!view.hasContent) actions.push(["CHOOSE GAME FOLDER", "choose"]);
    setActions(actions);
  } else if (mode === "preparing" && snapshot) {
    $("phases").hidden = false;
    const busy = snapshot.status === "busy";
    const failedRun = snapshot.status === "error";
    const done = snapshot.status === "finished" || snapshot.status === "finished-with-errors";
    $("title").textContent = busy
      ? "PREPARING IN ANOTHER WINDOW"
      : failedRun
      ? "PREPARATION STOPPED"
      : done
        ? "READY FOR BATTLE"
        : snapshot.playable
          ? "BATTLEFIELDS READY · FINISHING UP"
          : "PREPARING YOUR BATTLEFRONT";
    if (busy) setDetail("Another Battlefront Eternal is preparing this game data. This screen picks up as soon as it finishes or closes.");
    else if (failedRun) setDetail(snapshot.error || "Something went wrong.");
    else if (done && snapshot.failed.length) setDetail(`Ready, though ${snapshot.failed.length} step(s) failed and will be retried next launch.`);
    else if (snapshot.playable) setDetail("Every battlefield is in. Now the HUD, the enhanced sound, textures and the menu movies.");
    else setDetail("Preparing maps, armies, sound and movies from your copy. This happens once.", install || null);
    drawProgress(snapshot, view.estimate);
    const actions = [];
    if (failedRun) actions.push(["TRY AGAIN", "retry", true]);
    if (done) actions.push(["PLAY", "play", true]);
    setActions(actions);
  } else if (mode === "error") {
    $("title").textContent = "SOMETHING WENT WRONG";
    setDetail(view.message || "");
    setActions([["TRY AGAIN", "retry", true], ["CHOOSE FOLDER", "choose"]]);
  }
  if (snapshot) drawLog(snapshot);
}

$("toggle-log").addEventListener("click", () => {
  showLog = !showLog;
  draw();
});

desktop.prepare.onState((next) => {
  view = next;
  draw();
});
desktop.prepare.state().then((next) => {
  view = next;
  draw();
});
setInterval(draw, 5000);
