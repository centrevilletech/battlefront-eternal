// A replicated straight bolt has a launch sample and authoritative membership.
// Render its finite flight at the displayed world tick without advancing any
// collision, damage, lifetime, or mutable simulation position on the page.
// A class with a FadeOutTime (the sniper rifles') goes on being drawn where
// its flight ended (`ended`) until that time has run out (`fade`, 1 to 0).
export function linearBoltFrame(source, tick, out={}) {
  if(!Number.isFinite(source.networkFlightTick) || !Number.isFinite(tick))return null;
  const elapsed=(tick-source.networkFlightTick)/60;
  const fade=boltFade(source.def,elapsed-source.life);
  if(source.flown+source.speed*elapsed<0 || !(fade>0))return null;
  const flown=source.flown+source.speed*Math.min(elapsed,source.life);
  out.flown=flown;
  out.drawn=Math.min(flown,Math.max(source.def.LaserLength||1,source.def.GlowLength||0,source.def.BlurLength||0));
  out.ended=elapsed>=source.life;
  out.fade=fade;
  return out;
}

// How much of a bolt is drawn `after` seconds past the end of its flight
// (negative while it flies): all of it in flight; once it stops, a class
// with a FadeOutTime fades linearly to nothing over that time, as the
// 2004 game's laser does, and any other is gone at once.
export function boltFade(def, after) {
  if(after<0)return 1;
  const time=def?.FadeOutTime||0;
  return time>0?Math.max(0,1-after/time):0;
}
