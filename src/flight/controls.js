export const FLIGHT_BINDINGS = {
  throttle: ['KeyW'], brake: ['KeyS'], rollLeft: ['KeyA'], rollRight: ['KeyD'], boost: ['ShiftLeft','ShiftRight'],
  trick: ['AltLeft','ControlRight'], immelmann: ['CapsLock'], rollLock: ['KeyZ','Mouse1'],
  takeoff: ['Space'], targetLock: ['KeyQ','Mouse3'], firePrimary: ['Mouse0'], fireSecondary: ['Mouse2'],
  perspective: ['KeyV'], use: ['KeyE'], nextPrimary: ['Digit1','WheelUp'], previousPrimary: ['Digit2','WheelDown'],
};
export const flightBindings = settings => ({ ...FLIGHT_BINDINGS, ...(settings || {}) });
export function flightInput(controls, bindings, gamepad = null) {
  bindings ||= FLIGHT_BINDINGS;
  const held = name => (bindings[name]||[]).some(k=>controls.down.has(k));
  const edge = name => controls.take(name);
  const pad = gamepad || {}, axes = pad.axes || [], button = i => Boolean(pad.buttons?.[i]?.pressed);
  const dead = x => Math.abs(x||0)>.12?x:0;
  return {
    forward: held('throttle')-held('brake') || -dead(axes[3]), right: held('rollRight')-held('rollLeft') || dead(axes[2]),
    ...(dead(axes[0]) || dead(axes[1]) ? {stickX: dead(axes[0]),stickY: -dead(axes[1])}:{}),
    boost: held('boost') || button(11), rollLock: held('rollLock'),
    takeoff: edge('takeoff'), trick: edge('trick') || button(0), immelmann: edge('immelmann'), targetLock: edge('targetLock'),
  };
}
