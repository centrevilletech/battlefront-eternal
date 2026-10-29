// The click gate: browsers play no sound until the page has been clicked or
// a key pressed, so the front end opens on the original's first screen
// movie (start_screen, the profile screen's flythrough) with "PRESS ANY KEY
// OR CLICK TO BEGIN" pulsing at the bottom (the port's own words; the
// original's PC build has no such line). The press unlocks the sound.
//
// The line is gamefont_medium, white, its alpha pulsing between 0.3 and 1
// as a focused button's label does (a triangle, 0.7 s round).

const IW = 1024;
export const GATE_TEXT = "PRESS ANY KEY OR CLICK TO BEGIN";
const PULSE = 0.7; // s
const LOW = 0.3;
// Where the line sits: centred, 0.82 of the way down the safe area.
const AT = 0.82;

// Keys that don't count as the press: the browser's own and modifiers held.
export function countsAsPress(event) {
  if (event.type !== "keydown") return true;
  if (event.ctrlKey || event.metaKey || event.altKey) return false;
  return !/^(F\d+|Tab|Unidentified|Control|Meta|Alt|OS)$/.test(event.key || "");
}

// The pulse's alpha t seconds in.
export function pulse(t) {
  const phase = (t / PULSE) % 1;
  const tri = phase < 0.5 ? phase * 2 : 2 - phase * 2;
  return LOW + (1 - LOW) * (1 - tri);
}

// Show the gate over the page until the first press. fonts: CanvasFonts
// (src/shell/text.js); text: the line; drawn: false draws nothing (the page
// theme shows its own line) and only waits for the press. Returns
// { pressed, close() }: pressed resolves inside the press's own event, so
// the sound can start there.
export function showGate({ parent = document.body, fonts = null, text = GATE_TEXT, onPress = () => {}, drawn = true } = {}) {
  const canvas = document.createElement("canvas");
  canvas.id = "gate";
  canvas.style.cssText = "position:fixed;inset:0;width:100%;height:100%;z-index:5;pointer-events:none";
  parent.append(canvas);
  const ctx = canvas.getContext("2d");
  const startedAt = performance.now();
  let closed = false;
  let resolve;
  const pressed = new Promise((r) => { resolve = r; });

  function draw(now) {
    if (closed) return;
    requestAnimationFrame(draw);
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(window.innerWidth * ratio));
    const h = Math.max(1, Math.round(window.innerHeight * ratio));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const H = 768 + (h / w - 0.75) * 768;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.setTransform(w / IW, 0, 0, h / H, 0, 0);
    ctx.globalAlpha = pulse((now - startedAt) / 1000);
    const y = H * (0.05 + 0.9 * AT);
    if (!fonts?.has("gamefont_medium") || !fonts.draw(ctx, IW / 2, y, text, "gamefont_medium", { align: 2 })) {
      // No game font extracted: the browser's.
      ctx.font = "bold 18px sans-serif";
      ctx.fillStyle = "#fff";
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(text, IW / 2, y);
    }
    ctx.globalAlpha = 1;
  }

  const press = (event) => {
    if (closed || !countsAsPress(event)) return;
    close();
    onPress(event);
    resolve(event);
  };
  const types = ["pointerdown", "keydown", "touchstart"];
  for (const type of types) window.addEventListener(type, press, { capture: true });

  function close() {
    if (closed) return;
    closed = true;
    for (const type of types) window.removeEventListener(type, press, { capture: true });
    canvas.remove();
  }

  if (drawn) requestAnimationFrame(draw);
  else canvas.remove();
  return { pressed, close, canvas };
}
