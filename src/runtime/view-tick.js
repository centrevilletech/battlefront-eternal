// The tick currently rendered is the server's lag-compensation reference.
export function renderedViewTick(clock, latest) {
  return Number.isFinite(clock) && Number.isFinite(latest) ? Math.max(0, Math.min(Math.floor(latest), clock)) : 0;
}
