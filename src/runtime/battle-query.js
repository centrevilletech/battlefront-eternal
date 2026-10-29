// What of a server's battle query a player's page takes: the battle's own
// keys (map, side, mission, era, bots, seed, match token), each checked. A
// server names its battle in its listing, welcome and rotation, and nothing
// else a server sends may reach the page's URL, where it could change how
// the page runs. The master checks a listing's the same way.
const BATTLE_KEYS = {
  map: /^[a-z0-9_-]{1,64}$/i,
  side: /^[a-z0-9_-]{1,64}$/i,
  mission: /^[a-z0-9_-]{1,64}$/i,
  era: /^(cw|gcw)$/,
  ai: /^(classic|off)$/,
  seed: /^\d{1,10}$/,
  match: /^[a-f0-9]{32}$/,
};

export function battleParams(query) {
  const out = new URLSearchParams();
  for (const [key, value] of new URLSearchParams(typeof query === 'string' ? query : '')) {
    if (BATTLE_KEYS[key]?.test(value) && !out.has(key)) out.set(key, value);
  }
  return out;
}
