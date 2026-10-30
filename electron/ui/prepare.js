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

// A button in the front end's shape for the look (looks.css .bfe-btn). As
// in the front end, its text is kept in lower case for the look that sets
// labels in title case (bf2; GOG stays in capitals), a click doesn't take
// the focus, and the label as written is its name for screen readers.
function button(label, action, primary = false) {
  const element = document.createElement("button");
  element.className = primary ? "bfe-btn small primary active" : "bfe-btn small";
  element.setAttribute("aria-label", label);
  const text = document.createElement("span");
  text.className = "label";
  text.textContent = label.toLowerCase().replace(/\bgog\b/g, "GOG");
  element.append(text);
  element.addEventListener("mousedown", (event) => event.preventDefault());
  element.addEventListener("click", () => desktop.prepare.action(action));
  element.addEventListener("pointerenter", () => light(element));
  element.addEventListener("focus", () => light(element));
  return element;
}

// One button lit at a time, as in the front end: the one under the pointer
// or holding the keyboard's focus, else the state's own choice.
function light(element = null) {
  const buttons = [...$("actions").children];
  const lit = element || buttons.find((item) => item.classList.contains("primary")) || null;
  for (const item of buttons) item.classList.toggle("active", item === lit);
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

// ---- The LOADING disk ----
//
// As the game's loading screen draws it without its texture
// (src/shell/loading.js: drawDisk, drawSegments), in its interface units:
// translucent segments turning both ways, two thin rings, 28 lights with a
// lit tail chasing round them, LOADING tilted across it, the whole turned to
// 0.866 of its width. prepare.css gives it the look's colours and motion.

const SVG = "http://www.w3.org/2000/svg";
// [start, length] in radians and [inner, outer] radius of each segment.
const SEGMENTS = [[0.1, 1.0, 74, 118], [1.5, 0.7, 80, 104], [2.6, 1.1, 66, 96], [3.9, 0.5, 78, 122], [4.8, 0.9, 70, 100], [5.7, 0.4, 84, 110]];
const LIGHTS = 28;

function svg(tag, attributes = {}) {
  const node = document.createElementNS(SVG, tag);
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, String(value));
  return node;
}

function segment(start, length, inner, outer) {
  const at = (radius, angle) => `${(radius * Math.cos(angle)).toFixed(2)} ${(radius * Math.sin(angle)).toFixed(2)}`;
  const end = start + length;
  return `M ${at(outer, start)} A ${outer} ${outer} 0 0 1 ${at(outer, end)} L ${at(inner, end)} A ${inner} ${inner} 0 0 0 ${at(inner, start)} Z`;
}

function drawDisk() {
  const face = svg("g", { transform: "scale(0.866 1)" });
  // The outer layer turns one way; the inner, smaller and a radian on, the other.
  for (const [name, scale, offset] of [["outer", 1, 0], ["inner", 0.75, 1]]) {
    const layer = svg("g", { class: `segments ${name}` });
    const turned = svg("g", { transform: `rotate(${((offset * 180) / Math.PI).toFixed(2)})` });
    for (const [start, length, inner, outer] of SEGMENTS) turned.append(svg("path", { d: segment(start, length, inner * scale, outer * scale) }));
    layer.append(turned);
    face.append(layer);
  }
  for (const radius of [60, 44]) face.append(svg("circle", { class: "ring", r: radius }));
  const lights = svg("g", { class: "lights" });
  for (let i = 0; i < LIGHTS; i++) {
    lights.append(svg("rect", { x: -3.5, y: -3.5, width: 7, height: 7, style: `--at: ${(i / LIGHTS).toFixed(4)}`, transform: `rotate(${((i * 360) / LIGHTS).toFixed(3)}) translate(52 0)` }));
  }
  face.append(lights);
  const label = svg("text", { transform: "rotate(-14)" });
  label.textContent = "LOADING";
  face.append(label);
  $("disk").replaceChildren(face);
}

// ---- Asking before preparing (the confirm screen), and add-ons left out ----

// How the stores put back an installation's original files.
const REPAIR = "In Steam: the game's Properties > Installed Files > Verify integrity of game files. In GOG Galaxy: the game's settings > Manage installation > Verify / Repair.";

function element(tag, className = null, text = null) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== null) node.textContent = text;
  return node;
}

// Where a copy was found (electron/discover.js: source, how).
const foundIn = (copy) => copy.source === "chosen" ? "The folder you chose"
  : copy.source === "steam" || copy.source === "gog" ? `In ${copy.how}` : copy.how.charAt(0).toUpperCase() + copy.how.slice(1);

// The add-on maps a check found: each map, and those that will be left out
// with why.
function addOnList(check) {
  const lines = [];
  for (const addOn of check.addOns || []) {
    if (addOn.problems.length) lines.push([`${addOn.folder}: will be left out. ${addOn.problems[0].message}`, true]);
    else for (const map of addOn.maps) lines.push(map.problems.length ? [`${map.name} (${addOn.folder}): will be left out. ${map.problems[0].message}`, true] : [`${map.name} (${addOn.folder})`, false]);
  }
  if (!lines.length) return "None";
  const list = element("ul", "confirm-addon-list");
  for (const [text, out] of lines) list.append(element("li", out ? "left-out" : null, text));
  return list;
}

function confirmParts(view) {
  const { check, copy } = view;
  const facts = element("dl", "confirm-facts");
  const fact = (name, value, className) => {
    const dd = element("dd", className);
    dd.append(value);
    facts.append(element("dt", null, name), dd);
  };
  fact("FOLDER", element("code", null, copy.dir), "confirm-folder");
  fact("FOUND", foundIn(copy), "confirm-found");
  fact("CHECK", !check ? "Looking at what preparation needs." : check.ok ? "Everything preparation needs is there."
    : `${check.problems.length === 1 ? "A problem" : `${check.problems.length} problems`} found.`, `confirm-check ${!check ? "busy" : check.ok ? "ok" : "bad"}`);
  if (check?.ok) fact("ADD-ON MAPS", addOnList(check), "confirm-addons");
  const parts = [facts];
  const others = view.copies.map((entry, index) => ({ ...entry, index })).filter((entry) => !entry.selected);
  if (check && !check.ok) {
    const problems = element("ul", "confirm-problems");
    for (const entry of check.problems.slice(0, 6)) problems.append(element("li", null, entry.message));
    if (check.problems.length > 6) problems.append(element("li", null, `And ${check.problems.length - 6} more.`));
    const language = check.problems.every((entry) => entry.code === "language");
    parts.push(problems, element("p", "confirm-help", language ? "Use an English copy of the game: the Steam and GOG copies include one."
      : `${others.length ? "Use another copy below, or repair" : "Repair"} this one and choose CHECK AGAIN.`));
    if (!language) parts.push(element("p", "confirm-repair", REPAIR));
  } else if (check) {
    const option = element("label", "confirm-option");
    const box = element("input");
    box.type = "checkbox";
    box.checked = view.enhancedSound;
    box.addEventListener("change", () => desktop.prepare.action("enhanced-sound", box.checked));
    const adds = view.time?.sound > 0 ? ` Adds about ${clock(view.time.sound)}.` : "";
    option.append(box, element("span", "confirm-option-name", "ENHANCED SOUND"),
      element("span", "confirm-option-about", `The original sound, resampled and sharpened for modern speakers.${adds} Without it the game plays the original sound, and you can add it later.`));
    parts.push(option);
  }
  if (others.length) {
    const list = element("div", "confirm-copies");
    list.append(element("h2", null, others.length === 1 ? "ANOTHER COPY FOUND" : "OTHER COPIES FOUND"));
    for (const entry of others) {
      // The front end's button, lit while it has the pointer or the focus.
      const use = element("button", "bfe-btn small");
      use.setAttribute("aria-label", "USE THIS COPY");
      use.append(element("span", "label", "use this copy"));
      use.addEventListener("click", () => desktop.prepare.action("pick-copy", entry.index));
      for (const [event, lit] of [["pointerenter", true], ["pointerleave", false], ["focus", true], ["blur", false]]) use.addEventListener(event, () => use.classList.toggle("active", lit));
      const row = element("div", "confirm-copy");
      row.append(element("code", null, entry.dir), element("span", "confirm-copy-how", foundIn(entry)), use);
      list.append(row);
    }
    parts.push(list);
  }
  return parts;
}

function drawConfirm(view) {
  if (!view.copy) return;
  const check = view.check;
  const failed = !!check && !check.ok;
  document.body.classList.toggle("error", failed);
  $("title").textContent = !check ? "CHECKING YOUR COPY" : failed ? "THIS COPY CAN'T BE PREPARED" : "STAR WARS BATTLEFRONT FOUND";
  const takes = view.time ? ` and takes about ${clock(view.time.total)} on this computer` : "";
  setDetail(failed ? "Battlefront Eternal looked at this copy before converting anything, and found what would stop it."
    : `Battlefront Eternal converts the game's maps, armies, sound and movies into formats it can read. This happens once${takes}. Your installation is only read, never changed.`);
  $("progress").hidden = true;
  $("toggle-log").hidden = $("log").hidden = true;
  // Made again only when what it shows changes: the page redraws every few seconds.
  const section = $("confirm");
  section.hidden = false;
  const key = JSON.stringify([view.copies, check, view.time?.sound ?? null]);
  if (section.dataset.key !== key) {
    section.dataset.key = key;
    section.replaceChildren(...confirmParts(view));
  }
  const box = section.querySelector(".confirm-option input");
  if (box) box.checked = view.enhancedSound;
  setActions(failed ? [["CHECK AGAIN", "recheck", true], ["CHOOSE ANOTHER FOLDER", "choose"]]
    : check ? [["PREPARE", "prepare", true], ["CHOOSE ANOTHER FOLDER", "choose"]] : [["CHOOSE ANOTHER FOLDER", "choose"]]);
}

// What was left out of the add-on maps (.prepare.json's addOnFailures).
function leftOutLine(entry) {
  const named = entry.map ? `${entry.name} (${entry.addOn})` : entry.addOn;
  if (entry.part === "sound") return `${named}: plays the original sound, its Enhanced sound couldn't be prepared. ${entry.reason}`;
  if (entry.part === "effects") return `${named}: plays without its effects (fountains, steam, fires), which couldn't be prepared. ${entry.reason}`;
  return `${named}: left out. ${entry.reason}`;
}

function drawLeftOut(failures) {
  const section = $("left-out");
  section.hidden = !failures?.length;
  if (!failures?.length) return;
  const key = JSON.stringify(failures);
  if (section.dataset.key === key) return;
  section.dataset.key = key;
  const maps = failures.some((entry) => entry.part === "map" || entry.part === "add-on");
  const list = element("ul", "left-out-list");
  for (const entry of failures) list.append(element("li", null, leftOutLine(entry)));
  section.replaceChildren(element("h2", null, maps ? "ADD-ON MAPS LEFT OUT" : "ADD-ON MAPS PREPARED IN PART"), list,
    element("p", "left-out-help", "Everything else is ready. What was left out is tried again the next time your game data is prepared, for example when you choose the game folder again."));
}

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
    const finished = steps.length && steps.every((step) => ["done", "failed", "unavailable", "off", "left-out"].includes(step.status));
    const skipped = steps.length && steps.every((step) => step.status === "unavailable");
    // Left out by the player's choice (the Enhanced sound).
    const off = steps.length && steps.every((step) => step.status === "off" || step.status === "left-out") && steps.some((step) => step.status === "off");
    item.className = item.dataset.phase === active ? "active" : off ? "off" : skipped ? "skipped" : finished ? "done" : "";
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
  // The disk turns while something runs: not on a finished run, which may wait for PLAY.
  const finished = snapshot?.status === "finished" || snapshot?.status === "finished-with-errors";
  document.body.classList.toggle("idle", (mode !== "preparing" || finished) && mode !== "searching" && mode !== "update" && !(mode === "confirm" && !view.check));
  $("confirm").hidden = mode !== "confirm";
  $("left-out").hidden = true;

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
    const checking = view.phase === "checking";
    $("title").textContent = checking ? "CHECKING YOUR COPY" : "SEARCHING FOR STAR WARS BATTLEFRONT";
    setDetail(checking ? "Making sure your copy of Star Wars: Battlefront (2004) still has everything preparation needs." : "Looking through your Steam and GOG libraries.", checking ? install : null);
    $("progress").hidden = true;
    setActions([]);
  } else if (mode === "confirm") {
    drawConfirm(view);
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
    const prepared = view.prepared || {};
    $("title").textContent = "YOUR GAME CONTENT";
    setDetail("Export prepared content for another computer, import an existing export, or attach the original game for future content updates." +
      (prepared.canAddSound ? `\nThe Enhanced sound was left out when this game data was prepared. ADD ENHANCED SOUND prepares it from your copy${prepared.soundTime ? `, in about ${clock(prepared.soundTime)}` : ""}.` : ""));
    $("progress").hidden = true;
    drawLeftOut(prepared.addOnFailures);
    setActions([["PLAY", "play", true], ...(prepared.canAddSound ? [["ADD ENHANCED SOUND", "add-sound"]] : []), ["EXPORT CONTENT", "export"], ["IMPORT CONTENT EXPORT", "import"], ["CHOOSE GAME FOLDER", "choose"]]);
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
    const sound = snapshot.enhancedSound !== false;
    if (busy) setDetail("Another Battlefront Eternal is preparing this game data. This screen picks up as soon as it finishes or closes.");
    else if (failedRun) setDetail(snapshot.error || "Something went wrong.");
    else if (done && snapshot.addOnFailures?.length) setDetail("Your game data is ready, with what could be prepared of your add-on maps.");
    else if (snapshot.playable) setDetail(sound ? "Every battlefield is in. Now the HUD, the enhanced sound, textures and the menu movies." : "Every battlefield is in. Now the HUD, textures and the menu movies.");
    else setDetail(sound ? "Preparing maps, armies, sound and movies from your copy. This happens once." : "Preparing maps, armies and movies from your copy. This happens once. The Enhanced sound is off: you can add it later.", install || null);
    drawLeftOut(done ? snapshot.addOnFailures : null);
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
  // A list on show (the copy to prepare from, the add-on maps left out)
  // scrolls on its own, so the buttons stay in view (prepare.css).
  document.body.classList.toggle("listing", !$("confirm").hidden || !$("left-out").hidden);
  markMore();
}

// A list with more below than it has room for fades out at its foot.
function markMore() {
  for (const list of [$("confirm"), $("left-out")]) list.classList.toggle("more", !list.hidden && list.scrollTop + list.clientHeight < list.scrollHeight - 2);
}
for (const id of ["confirm", "left-out"]) $(id).addEventListener("scroll", markMore, { passive: true });
window.addEventListener("resize", markMore);

$("toggle-log").addEventListener("click", () => {
  showLog = !showLog;
  draw();
});

// Leaving the buttons lights the state's own choice again, unless the
// keyboard's focus is on one.
$("actions").addEventListener("pointerleave", () => {
  if (!$("actions").querySelector(":focus-visible")) light();
});
$("actions").addEventListener("focusout", (event) => {
  if (!$("actions").contains(event.relatedTarget)) light();
});

drawDisk();

desktop.prepare.onState((next) => {
  view = next;
  draw();
});
desktop.prepare.state().then((next) => {
  view = next;
  draw();
});
setInterval(draw, 5000);
