import { OPTIONS, PAGES } from "./menus/options.js";
import { mainMenuUrl, restartUrl } from "./launch.js";
import { withSettings } from "./pause/overrides.js";

// A plain HTML form of the pause menu and its options, behind ?menu=html:
// a fallback for when the drawn menus (src/pause, src/menus) can't be used.
// Built from the same options schema, with the same object the spawn
// screen and scoreboard call (isOpen, open, close, onClose, enableRespawn).
const STYLE = `
#menu-html { position: fixed; inset: 0; display: grid; place-items: center; background: rgba(0, 0, 0, .45); font: 13px/1.4 sans-serif; color: #f2efe6; z-index: 10; }
#menu-html[hidden] { display: none; }
#menu-html form { width: min(440px, calc(100vw - 32px)); max-height: calc(100vh - 32px); overflow: auto; padding: 14px 18px; background: rgba(18, 20, 24, .95); border: 1px solid #5d584b; }
#menu-html h2 { margin: 0 0 8px; font-size: 15px; letter-spacing: .08em; color: #e6c25a; }
#menu-html fieldset { margin: 0 0 10px; border: 1px solid #3d3a33; }
#menu-html label { display: flex; justify-content: space-between; gap: 10px; margin: 0 0 6px; }
#menu-html .buttons { display: flex; flex-wrap: wrap; gap: 6px; }
`;

export class HtmlOptionsMenu {
  constructor(settings, { onOpen = () => {}, onClose = () => {} } = {}) {
    this.settings = settings;
    this.onOpen = onOpen;
    this.onClose = onClose;
    this.respawn = null;
    this.owners = [];
    const style = document.createElement("style");
    style.textContent = STYLE;
    document.head.appendChild(style);
    this.root = document.createElement("div");
    this.root.id = "menu-html";
    this.root.hidden = true;
    const field = (entry) => {
      const value = settings.get(entry.key);
      if (entry.kind === "toggle") return `<input type="checkbox" name="${entry.key}"${value ? " checked" : ""}>`;
      if (entry.kind === "slider") return `<input type="range" name="${entry.key}" min="${entry.min}" max="${entry.max}" step="${entry.step}" value="${value}">`;
      if (entry.kind === "fixed") return `<span>${entry.display}</span>`;
      return `<select name="${entry.key}">${entry.choices.map(([v, label]) => `<option value="${v}"${String(v) === String(value) ? " selected" : ""}>${label}</option>`).join("")}</select>`;
    };
    this.root.innerHTML = `<form autocomplete="off"><h2>GAME PAUSED</h2>
      <div class="buttons"><button type="submit">RESUME GAME</button><button type="button" data-action="respawn">RESPAWN</button>
      <button type="button" data-action="restart">RESTART MISSION</button><button type="button" data-action="quit">QUIT</button></div>
      ${PAGES.map((page) => `<fieldset><legend>${page.title}</legend>${OPTIONS.filter((e) => e.page === page.id)
        .map((entry) => `<label>${entry.label}${entry.apply !== "live" ? " *" : ""} ${field(entry)}</label>`).join("")}</fieldset>`).join("")}
      <p>* TAKES EFFECT ON THE NEXT MAP OR BATTLE</p></form>`;
    document.body.appendChild(this.root);
    const form = this.root.querySelector("form");
    form.addEventListener("input", (event) => {
      const input = event.target;
      const entry = OPTIONS.find((e) => e.key === input.name);
      if (!entry) return;
      const raw = input.type === "checkbox" ? input.checked : input.value;
      const value = entry.kind === "slider" ? Number(raw) : entry.kind === "choice" ? entry.choices.find(([v]) => String(v) === raw)?.[0] ?? raw : raw;
      settings.set(entry.key, value);
    });
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      this.close(true);
    });
    const ask = (question, yes) => () => window.confirm(question) && yes();
    this.root.querySelector('[data-action="respawn"]').addEventListener("click", ask("DO YOU REALLY WISH TO KILL YOUR CURRENT PLAYER?", () => {
      if (this.respawn?.available?.() ?? true) this.respawn?.respawn();
      this.close(true);
    }));
    this.root.querySelector('[data-action="restart"]').addEventListener("click", ask("DO YOU REALLY WANT TO RESTART THIS BATTLE?", () => location.replace(withSettings(restartUrl(), settings))));
    this.root.querySelector('[data-action="quit"]').addEventListener("click", ask("LEAVING THE GAME NOW WILL END THIS SESSION. ARE YOU SURE YOU WANT TO LEAVE?", () => location.replace(mainMenuUrl())));
    window.addEventListener("keydown", (event) => {
      if (event.code !== "Escape" || event.repeat) return;
      if (this.isOpen) this.close(true);
      else if (!this.owners.some((owner) => owner?.isOpen)) this.open();
    });
  }

  get isOpen() {
    return !this.root.hidden;
  }

  open() {
    if (this.isOpen) return;
    if (document.pointerLockElement) document.exitPointerLock();
    this.root.hidden = false;
    this.onOpen();
  }

  close(resume = false) {
    if (!this.isOpen) return;
    this.root.hidden = true;
    this.onClose(resume);
  }

  enableRespawn(options) {
    this.respawn = options;
  }

  setEscapeOwners(...screens) {
    this.owners = screens.filter(Boolean);
  }

  setPointerLocked() {}
}
