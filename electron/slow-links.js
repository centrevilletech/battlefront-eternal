import net from "node:net";

// Node gives each of a host's addresses (its IPv6, its IPv4) 250 ms to
// connect before dropping it for the next ("happy eyeballs"). Over a slow
// or congested link, with round trips of 500 ms and more, the IPv4 attempt
// is dropped before it can answer; on a machine with no IPv6 route that
// leaves nothing, and every request to a dual-stack host (the online
// service, updates, signalling) fails with "fetch failed" (ETIMEDOUT) while
// browsers and curl get through. Each attempt gets long enough to answer
// over such a link; one that fails outright (no route) still moves on at
// once.
export const CONNECT_ATTEMPT = 2500; // ms

// A process's default: set on import, first thing, by the main process
// (boot.js) and the network process (net-process.js).
export function allowSlowLinks(target = net) {
  target.setDefaultAutoSelectFamilyAttemptTimeout?.(CONNECT_ATTEMPT);
}

// The same for a Node process this one starts (a hosted server), as its
// first argument.
export const SLOW_LINKS_FLAG = `--network-family-autoselection-attempt-timeout=${CONNECT_ATTEMPT}`;

allowSlowLinks();
