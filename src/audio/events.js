// The game's sound events. Gameplay code reports what happens with one-line
// calls (emitSound("explosion", def, position)); the audio engine
// (src/audio/game.js) listens. With no listener the calls do nothing, so the
// game runs the same without audio. docs/audio.md describes the engine.
let listener = null;
const observers = new Set();
export function observeSounds(handler) { observers.add(handler); return () => observers.delete(handler); }

export function emitSound(type, ...args) {
  if (listener) listener(type, ...args);
  for (const observer of observers) observer(type, ...args);
}

export function listenSounds(handler) {
  listener = handler;
  return () => {
    if (listener === handler) listener = null;
  };
}
