import { Renderer } from "./spatial.js";

// Browser device ownership only. Native sound control owns resource gates,
// voices, buses and event selection; the renderer carries its PCM to I/O.
export class AudioEngine {
  constructor({ settings = null, maxVoices = 32, virtualVoices = 64, context = null } = {}) {
    this.settings = settings; this.maxVoices = maxVoices; this.virtualVoices = virtualVoices;
    this.ctx = context; this.stats = {real:0,virtual:0,started:0,stolen:0,dropped:0,culled:0};
    if (context) this.renderer = new Renderer(context);
  }
  // The context starts suspended until the page has been clicked; the first
  // click (the one that captures the mouse) resumes it.
  ensureContext() {
    if (this.ctx) return this.ctx;
    const Context = window.AudioContext || window.webkitAudioContext;
    this.ctx = new Context({ latencyHint: "interactive" });
    this.renderer = new Renderer(this.ctx);
    const resume = () => {
      if (this.ctx.state === "suspended" && !document.hidden) this.ctx.resume();
    };
    for (const type of ["pointerdown", "keydown", "touchstart"]) window.addEventListener(type, resume, { capture: true });
    // Hidden tabs go quiet and stop spending time on audio.
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) this.ctx.suspend();
      else if (navigator.userActivation?.hasBeenActive ?? true) this.ctx.resume();
    });
    return this.ctx;
  }

  applyVolumes() {
    const master = this.settings?.get("masterVolume");
    this.renderer?.setVolume(master === undefined || master === null ? 1 : Number(master));
  }

}
