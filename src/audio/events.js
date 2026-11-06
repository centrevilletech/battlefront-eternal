// The game's sound events. Gameplay code reports what happens with one-line
// calls (emitSound("explosion", def, position)); the audio engine
// (src/audio/game.js) listens. With no listener the calls do nothing, so the
// game runs the same without audio. docs/audio.md describes the engine.
let listener = null;
const observers = [];
// Registration is cold. Tombstones preserve live Set iteration when a
// callback removes itself or another observer during a nested emission.
export function observeSounds(handler) {
  if (!observers.includes(handler)) observers.push(handler);
  return () => { const at = observers.indexOf(handler); if (at >= 0) observers[at] = null; };
}
function deliver(handler,count,type,a,b,c) {
  if (count === 1) handler(type);
  else if (count === 2) handler(type,a);
  else if (count === 3) handler(type,a,b);
  else if (count === 0) handler();
  else handler(type,a,b,c);
}

export function emitSound(type,a,b,c) {
  // Reading only arguments.length avoids materializing a rest array while
  // retaining the exact payload arity used by interface event observers.
  const count = arguments.length;
  if (listener) deliver(listener,count,type,a,b,c);
  for (let i=0;i<observers.length;i++) if (observers[i]) deliver(observers[i],count,type,a,b,c);
}

export function listenSounds(handler) {
  listener = handler;
  return () => {
    if (listener === handler) listener = null;
  };
}
