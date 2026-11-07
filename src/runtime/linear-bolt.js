// How much of a bolt is drawn `after` seconds past the end of its flight
// (negative while it flies): all of it in flight; once it stops, a class
// with a FadeOutTime fades linearly to nothing over that time, as the
// 2004 game's laser does, and any other is gone at once.
export function boltFade(def, after) {
  if(after<0)return 1;
  const time=def?.FadeOutTime||0;
  return time>0?Math.max(0,1-after/time):0;
}
