const VIEW = Object.freeze({invertMouse:1,firstPerson:2,flight:4,flightInvert:8,gamepad:16,
  mouseScale:32,aspect:64,viewTick:128,sensitivity:256,curve:512,bindings:1024});
// Public view objects are editable by input producers. Store the protocol
// projection when they change, instead of rebuilding every actor at publish.
export function publicationView(context, view) {
  const fs = context.flightSettings, pad = context.gamepad;
  view.flags = (context.invertMouse ? VIEW.invertMouse : 0) | (context.firstPerson ? VIEW.firstPerson : 0) | (fs ? VIEW.flight : 0) | (fs?.invert ? VIEW.flightInvert : 0) | (pad ? VIEW.gamepad : 0)
    | (context.mouseScale !== undefined ? VIEW.mouseScale : 0) | (context.aspect !== undefined ? VIEW.aspect : 0) | (context.viewTick !== undefined ? VIEW.viewTick : 0)
    | (fs?.sensitivity !== undefined ? VIEW.sensitivity : 0) | (fs?.curve !== undefined ? VIEW.curve : 0) | (fs?.bindings !== undefined ? VIEW.bindings : 0);
  view.mouseScale = typeof context.mouseScale === 'number' ? context.mouseScale : NaN;
  view.aspect = typeof context.aspect === 'number' ? context.aspect : NaN;
  view.viewTick = typeof context.viewTick === 'number' ? context.viewTick : NaN;
  view.sensitivity = typeof fs?.sensitivity === 'number' ? fs.sensitivity : NaN;
  view.curve = typeof fs?.curve === 'number' ? fs.curve : NaN;
  view.profile = 0; view.profileFound = true;
  for (const key in fs?.bindings) if (Object.prototype.propertyIsEnumerable.call(fs.bindings, key)) { view.profileFound = false; break; }
  view.buttons = 0;
  for (let j = 0; j < 4; j++) view.axes[j] = typeof (pad?.axes?.[j] ?? 0) === 'number' ? (pad?.axes?.[j] ?? 0) : NaN;
  for (let j = 0; j < 16; j++) if (pad?.buttons?.[j]?.pressed) view.buttons |= 1 << j;
}
