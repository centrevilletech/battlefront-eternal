import { EVENT_KIND } from "./control-layout.js";
import { LAYOUTS } from "./layouts.js";

// The audio panel: what the device offers, a speaker test, and a live view
// of the mix. Opened from the settings dialog ("Speaker test and audio
// details") or with ?audiodebug=1.
//
// The speaker test plays pink noise on one channel at a time, straight to
// the output (the game's sound is muted meanwhile), naming the speaker it is
// meant for, so the channel order can be checked on the user's own setup;
// the LFE gets a 50 Hz tone. "Around the room" pans a noise burst once round
// through the game's own panner.

const BURST = 1.2; // s

export class AudioPanel {
  constructor(audio) {
    this.audio = audio;
    this.root = document.createElement("div");
    this.root.id = "audio-panel";
    this.root.hidden = true;
    Object.assign(this.root.style, {
      position: "fixed",
      top: "12px",
      right: "12px",
      width: "min(440px, calc(100vw - 24px))",
      maxHeight: "calc(100vh - 24px)",
      overflow: "auto",
      padding: "12px 16px",
      background: "rgba(18, 20, 24, 0.94)",
      border: "1px solid #5d584b",
      borderRadius: "4px",
      color: "#f2efe6",
      font: "12px/1.45 monospace",
      zIndex: 20,
    });
    document.body.appendChild(this.root);
    window.addEventListener("audio-panel", () => this.toggle());
    window.addEventListener("keydown", (e) => {
      if (e.code === "Escape" && !this.root.hidden) this.toggle(false);
    });
    this.reading = null;
    this.reduction = 0;
    this.timer = null;
  }

  toggle(open = this.root.hidden) {
    this.root.hidden = !open;
    const r = this.audio.engine.renderer;
    if (open) {
      if (r) {
        r.onMeter = (m) => (this.reading = m);
        r.onLimiter = (m) => (this.reduction = m.reduction);
        r.meter(true);
      }
      this.render();
      this.timer = setInterval(() => this.refresh(), 250);
    } else {
      clearInterval(this.timer);
      this.timer = null;
      r?.meter(false);
    }
  }

  render() {
    const r = this.audio.engine.renderer;
    const ctx = this.audio.engine.ctx;
    const layout = r?.layout || LAYOUTS.stereo;
    this.root.innerHTML = `
      <div style="display:flex;justify-content:space-between;align-items:center">
        <b style="color:#e6c25a;letter-spacing:.08em">AUDIO</b>
        <button data-close style="font:inherit">close</button>
      </div>
      <div data-device></div>
      <div style="margin:8px 0 4px;color:#b9b19c">SPEAKER TEST (game muted while it plays)</div>
      <div data-speakers style="display:flex;flex-wrap:wrap;gap:4px">${layout.names
        .map((n, c) => `<button data-channel="${c}" style="font:inherit;min-width:46px">${n}</button>`)
        .join("")}
        <button data-cycle style="font:inherit">all in turn</button>
        <button data-around style="font:inherit">around the room</button>
      </div>
      <div data-testing style="min-height:1.4em;color:#9fe3a0"></div>
      <div style="margin:8px 0 4px;color:#b9b19c">MIX</div>
      <div data-mix></div>
      <div style="margin:8px 0 4px;color:#b9b19c">VOICES</div>
      <pre data-voices style="margin:0;white-space:pre-wrap"></pre>`;
    this.root.querySelector("[data-close]").onclick = () => this.toggle(false);
    for (const b of this.root.querySelectorAll("[data-channel]")) b.onclick = () => this.test([Number(b.dataset.channel)]);
    this.root.querySelector("[data-cycle]").onclick = () => this.test(layout.names.map((_, c) => c));
    this.root.querySelector("[data-around]").onclick = () => this.around();
    this.device = this.root.querySelector("[data-device]");
    this.mixView = this.root.querySelector("[data-mix]");
    this.voicesView = this.root.querySelector("[data-voices]");
    this.testing = this.root.querySelector("[data-testing]");
    if (ctx?.state !== "running") this.testing.textContent = "Click in the game once to start the sound.";
    this.refresh();
  }

  // How the renderer places sound, in a line.
  law(r) {
    if (r.binaural) return "headphones: 3rd-order ambisonics through a KU 100 dummy head (SADIE II)";
    if (r.hrtf && !r.enhanced) return "headphones: every source and stream channel through an HRTF (Rust IRCAM)";
    if (r.hrtf) return "headphones: Rust IRCAM HRTF per voice (tools/make_binaural.py builds the dummy-head decoder)";
    if (r.light) return `panning: OpenAL Soft 1.20.1's stereo law, the light mix (no reverb)`;
    if (r.alsoft) return `panning: OpenAL Soft 1.20.1's decoders${r.layout.center >= 0 ? " and front stabilizer" : ""}`;
    if (r.channels <= 2) return "panning: equal-power stereo";
    return `panning: pairwise (VBAP)${r.mode === "7.1.4" ? " with the height ring" : ""}`;
  }

  refresh() {
    const e = this.audio.engine;
    const r = e.renderer;
    const ctx = e.ctx;
    if (!ctx || !r?.layout || !this.audio.control) return;
    const dest = ctx.destination;
    this.device.innerHTML =
      `device: ${dest.maxChannelCount} channels, ${ctx.sampleRate} Hz, latency ${((ctx.baseLatency || 0) * 1000).toFixed(1)}` +
      ` + ${((ctx.outputLatency || 0) * 1000).toFixed(1)} ms<br>` +
      `rendering: ${r.enhanced ? "Enhanced" : "Original (2004)"}, ${r.layout.label} (${r.channels} ch: ${r.layout.names.join(" ")})` +
      `<br>${this.law(r)}`;
    const m = this.reading;
    const f = (x, unit = "") => (Number.isFinite(x) ? `${x.toFixed(1)}${unit}` : "-inf");
    this.mixView.innerHTML = m
      ? `loudness M ${f(m.momentary)} S ${f(m.shortTerm)} I ${f(m.integrated)} LUFS · true peak ${f(m.truePeak, " dBTP")} · limiter ${f(this.reduction, " dB")}<br>` +
        `peaks ${m.channels.map((p, c) => `${r.layout.names[c] || c} ${f(p)}`).join("  ")}<br>` +
        `voices ${e.stats.real} heard / ${e.stats.virtual} virtual`
      : "waiting for the meter...";
    const lines = this.audio.control.diagnostic()
      .sort((a, b) => b.score - a.score)
      .slice(0, 24)
      .map((v) => {
        const name = this.audio.data.label(v.name || "?");
        const db = v.audible > 0 ? (20 * Math.log10(v.audible)).toFixed(1) : "-inf";
        return `${v.real ? "*" : " "} ${name.padEnd(34).slice(0, 34)} ${v.category.padEnd(10)} ${v.mode3D ? `${v.distance.toFixed(1).padStart(6)} m` : "    2D  "} ${db.padStart(6)} dB${v.occluded > 0.5 ? " occl" : ""}`;
      });
    this.voicesView.textContent = lines.join("\n") || "(none)";
  }

  async test(channels) {
    const e = this.audio.engine;
    const ctx = e.ctx;
    const r = e.renderer;
    if (!ctx || ctx.state !== "running") return;
    const layout = r.layout;
    const N = layout.channels;
    const pink = await r.testBuffer(false, BURST);
    const tone = await r.testBuffer(true, BURST);
    const restore = r.masterVolume ?? 1;
    r.setVolume(0);
    let t = ctx.currentTime + 0.1;
    for (const c of channels) {
      // A Rust stream test bus targets exactly this device channel after
      // the muted game mix, including the LFE's generated 50 Hz tone.
      this.audio.control.enqueue(EVENT_KIND.TEST_CHANNEL,-1,-1,-1,r.engineSample(c === layout.lfe ? tone : pink),null,t,c === layout.lfe ? 1 : 0.35,c,BURST);
      const at = t;
      setTimeout(() => {
        if (this.testing) this.testing.textContent = `playing: ${layout.names[c]} (channel ${c + 1} of ${N})`;
        for (const b of this.root.querySelectorAll("[data-channel]")) b.style.outline = Number(b.dataset.channel) === c ? "2px solid #9fe3a0" : "";
      }, Math.max((at - ctx.currentTime) * 1000, 0));
      t += BURST + 0.25;
    }
    setTimeout(() => {
      r.setVolume(restore);
      if (this.testing) this.testing.textContent = "done: did each name match the speaker it came from?";
      for (const b of this.root.querySelectorAll("[data-channel]")) b.style.outline = "";
    }, (t - ctx.currentTime) * 1000 + 100);
  }

  // A burst of noise carried once round the listener, through the game's
  // own panning (so it shows the renderer, not just the wiring).
  async around() {
    const e = this.audio.engine;
    const ctx = e.ctx;
    const r = e.renderer;
    if (!ctx || ctx.state !== "running") return;
    const buffer = await r.testBuffer(false, BURST);
    this.audio.control.enqueue(EVENT_KIND.TEST_AROUND,-1,-1,-1,r.engineSample(buffer),null,ctx.currentTime,0.3,-1,6);
    if (this.testing) this.testing.textContent = "playing: one turn around the listener";
    setTimeout(()=> { if (this.testing) this.testing.textContent = "done"; },6100);
  }
}
