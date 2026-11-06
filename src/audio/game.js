import { SoundData } from "./data.js";
import { AudioEngine } from "./engine.js";
import { StreamAssets } from "./stream-assets.js";
import { SoundControl } from "./control.js";
import { EVENT_KIND } from "./control-layout.js";
import { listenSounds } from "./events.js";
import { EnhancedAssets } from "./enhanced.js";
import { AudioPanel } from "./panel.js";

const AUDIO_ROOT = "/audio";
// The page owns the browser device and asynchronous asset transport. All
// battle sound decisions run in one native phase from the captured facts.
export class GameAudio {
  constructor({settings = null} = {}) {
    this.settings = settings; this.engine = new AudioEngine({settings}); this.streams = new StreamAssets(this.engine);
    this.data = null; this.game = null; this.ready = false;
    this.unlisten = listenSounds((type,a,b,c)=>this.on(type,a,b,c));
    settings?.onChange((key)=> { if (/Volume$/.test(key)) this.engine.applyVolumes(); else if (key.startsWith("audio")) this.configure(); });
  }
  reportError(error) {
    this.error = error;
    this.ready = false;
    const stopping = this.engine.ctx?.suspend?.();
    stopping?.catch(() => {});
    console.error(`Audio unavailable: ${error.message}`);
    if (typeof document === "undefined") return;
    let alert = document.getElementById("audio-error");
    if (!alert) {
      alert = document.createElement("div"); alert.id = "audio-error"; alert.setAttribute("role", "alert");
      alert.style.cssText = "position:fixed;left:1rem;right:1rem;bottom:1rem;z-index:10000;padding:1rem;background:#3b1010;color:white";
      document.body.append(alert);
    }
    alert.textContent = `Audio unavailable: ${error.message}`;
  }

  // Load the battle's sound: public/audio/index.json maps each mission to
  // the sound sub-level it reads.
  async load(world) {
    const index = await fetch(`${AUDIO_ROOT}/index.json`)
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null);
    const sublevel = index?.missions?.[world.mission?.name] || index?.maps?.[world.name];
    if (!sublevel) throw new Error(`no sound for ${world.mission?.name || world.name}: run tools/extract.py`);
    const data = await SoundData.load(AUDIO_ROOT, sublevel);
    this.engine.ensureContext();
    this.engine.renderer.onError = (error) => this.reportError(error);
    await this.engine.renderer.load(AUDIO_ROOT, this.config());
    this.engine.enhanced = await EnhancedAssets.open(this.engine.ctx, AUDIO_ROOT, sublevel);
    this.data = data;
    this.streams.setData(data);
    if (!(await this.configure())) throw this.error;
    this.panel = new AudioPanel(this);
    if (new URLSearchParams(location.search).get("audiodebug") === "1") this.panel.toggle(true);
    this.ready = true;
    if (this.game) await this.prepareControl();
    return data;
  }

  attach(game) {
    this.game = game;
    if (this.ready) this.prepareControl().catch((error)=>this.reportError(error));
  }
  async prepareControl(seed) {
    this.ready = false;
    this.control?.dispose(); this.control = await SoundControl.prepare(this,seed);
    this.control.prepareVehicles(); this.ready = true; this.start();
  }
  retarget(fields) {
    if (!this.game) return; const old = this.game.soldier;
    Object.assign(this.game,fields);
    if (fields.vehicles && this.control) this.control.prepareVehicles();
    if (this.started && this.game.soldier !== old) this.control.enqueue(EVENT_KIND.RETARGET,this.control.actor(this.game.combat,this.game.unit,this.game.combat?.owner));
  }
  start() {
    if (!this.control) return;
    this.control.enqueue(EVENT_KIND.START); this.started = true;
  }
  // The audio settings: Original or Enhanced, the output layout, the
  // dynamics profile, mono, and the enhanced mode's clarity stage.
  config() {
    const s = this.settings;
    const get = (key, fallback) => {
      const v = s?.get(key);
      return v === undefined || v === null ? fallback : v;
    };
    return {
      quality: get("audioQuality", "enhanced"),
      output: get("audioOutput", "auto"),
      dynamics: get("audioDynamics", "normal"),
      mono: Boolean(get("audioMono", false)),
      clarity: Boolean(get("audioClarity", true)),
    };
  }

  async configure() {
    const r = this.engine.renderer;
    if (!r?.loaded) return false;
    try {
      await r.configure(this.config());
      this.configured();
      return true;
    } catch (error) {
      this.reportError(error);
      return false;
    }
  }

  configured() {
    this.engine.applyVolumes();
    if (this.engine.renderer.enhanced) this.engine.enhanced?.preload().catch((error)=>this.engine.renderer.fail(error));
  }
  on(type,a,b,c) { if (this.ready && this.started) this.control.event(type,a,b,c); }
  setPaused(paused) { if (this.control) this.control.enqueue(EVENT_KIND.PAUSE,-1,-1,-1,0,null,0,0,paused ? 1:0); }
  endOfBattle() { if (this.control) this.control.enqueue(EVENT_KIND.END); }
  update(dt) { if (this.ready && this.started) { try { this.control.update(dt); } catch (error) { this.reportError(error); } } }
}
