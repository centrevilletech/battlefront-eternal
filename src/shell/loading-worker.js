import { LoadingView, runView } from "./loading.js";

// The loading screen's own thread (loading.js): it draws on the canvas the
// page handed over, so its zoom and its disk keep moving while the battle's
// load holds the page's thread. Once the pictures are in it sends the page
// the sequence's plan (its length and its sounds); the page puts the sounds
// on its audio clock and says when the sequence starts (go). The page also
// sends the window's size; this sends back where the pictures are, for the
// hand-over and checks.

let view = null;
let size = [1, 1];

self.onmessage = (event) => {
  const message = event.data || {};
  if (message.type === "start") {
    view = new LoadingView(message.map);
    size = [message.canvas.width, message.canvas.height];
    const ready = () => self.postMessage({ type: "ready", plan: view.plan() });
    view.load().then(ready, (error) => {
      console.warn(`loading screen: ${error.message}`);
      ready();
    });
    const frame = typeof requestAnimationFrame === "function" ? (fn) => requestAnimationFrame(fn) : (fn) => setTimeout(fn, 1000 / 60);
    runView(view, message.canvas, () => size, frame);
    setInterval(() => self.postMessage({ type: "state", state: view.state() }), 100);
  } else if (message.type === "go" && view) {
    view.go(message.start);
  } else if (message.type === "size") {
    size = message.size;
  }
};
