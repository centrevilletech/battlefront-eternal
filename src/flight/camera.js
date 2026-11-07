import { setting } from '../seats.js';

// How fast a section's chase camera eases (1/s), as the 2004 game's camera
// section parser reads it: its turn (AimTension) and each of its
// axes in its own frame, right, left, up, down, back, forward (MoveTension
// sets all six; MoveTensionX, Y and Z one axis, one value for both ways or a
// pair). `rate` where the section sets none. Both are kept hashed by the
// import; most flyer pilot sections set them, the snowspeeder's rising at
// 2.5/s and sinking at 8.5.
export function cameraTension(s, rate) {
  const number = (name, fallback) => { const n = parseFloat(setting(s, name)); return Number.isFinite(n) ? n : fallback; };
  const all = number('MoveTension', rate), moveTension = [all, all, all, all, all, all];
  ['MoveTensionX', 'MoveTensionY', 'MoveTensionZ'].forEach((name, axis) => {
    const value = setting(s, name), pair = value === undefined ? null : String(value).trim().split(/\s+/).map(Number);
    if (pair && Number.isFinite(pair[0])) { moveTension[axis * 2] = pair[0]; moveTension[axis * 2 + 1] = Number.isFinite(pair[1]) ? pair[1] : pair[0]; }
  });
  return { aimTension: number('AimTension', rate), moveTension };
}
