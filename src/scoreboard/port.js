import { shownStats } from "../battle.js";
import { personalStats, awards } from "../battle-stats.js";

// A player's connection as the session names it (the status line's words).
export const LINK_LABELS = { direct4: "DIRECT IPV4", direct6: "DIRECT IPV6", relay4: "RELAY IPV4", relay6: "RELAY IPV6" };

// The scoreboard's way into the world: plain values to read, made from the
// battle's Characters (src/battle.js) each time they are asked for, so the
// board never holds the world's objects. The world keeps the scores (and a
// worker or replay keeps the same ones); the board only shows them.
//
// battle: the Battle; teams(): the spawn port's { 1: { side, logo }, 2 };
// humanTeam(): the player's team; relation(a, b): team a's relation to
// team b (1 friend, 0 neutral, -1 enemy); commonModel(name): a fresh copy of
// a common hologram model, or null; label(kind, key): a class's display
// name ("class", class name), or null; restart(): the battle again.
//
// network: whether the battle is a network game. Its rows carry each
// player's PING, as the original's network player list does, and a
// player's DETAILS his CONNECTION.
export function scoreboardPort({ battle, teams = () => ({}), humanTeam = () => battle.own, relation = () => 0, commonModel = () => null,
  label = () => null, restart = () => {}, canRestart = () => true, network = false }) {
  const name = (character) => String(character?.name ?? "");
  // The server's last round trip to a player in ms, null until measured.
  const ping = (character) => (Number.isFinite(character?.link?.ping) ? character.link.ping : null);
  return {
    network: Boolean(network),
    // [{ slot, team, name, ping, link, human, local }]: the players attached
    // to a network game, in roster order, or null offline (the player's
    // activity counts them: src/presence.js). ping as roster() has it; link
    // is the path the server reaches them by ("direct4", "direct6",
    // "relay4", "relay6", or "unknown").
    players() {
      if (!network) return null;
      const rows = [];
      battle.characters.forEach((character, slot) => {
        if (!character.human || (character.team !== 1 && character.team !== 2)) return;
        rows.push({
          slot,
          team: character.team,
          name: String(character.name ?? ""),
          ping: ping(character),
          link: character.link?.kind ?? "unknown",
          human: true,
          local: character === battle.human,
        });
      });
      return rows;
    },
    // [{ slot, team, name, kills, deaths, points, human, local, ping }]:
    // every Character of teams 1 and 2 in roster order, bots included
    // (locals aren't listed). KILLS is kills - 2 x team kills; local is the
    // player himself; ping a player's in a network game, else null.
    roster() {
      const rows = [];
      battle.characters.forEach((character, slot) => {
        if (character.team !== 1 && character.team !== 2) return;
        const human = Boolean(character.human);
        rows.push({
          slot,
          team: character.team,
          name: String(character.name ?? ""),
          ...shownStats(character),
          human,
          local: character === battle.human,
          ping: network && human ? ping(character) : null,
        });
      });
      return rows;
    },
    // { 1: { name, reinforcements, side, logo }, 2: ... }: each team's
    // display name, what it has left, its side and its logo model.
    teams() {
      const sides = teams() || {};
      const out = {};
      for (const team of [1, 2]) {
        if (!battle.teams?.[team]) continue;
        out[team] = {
          name: String(battle.names?.[team] ?? ""),
          reinforcements: Math.max(Math.ceil(battle.reinforcements?.[team] ?? 0), 0),
          side: sides[team]?.side ?? null,
          logo: sides[team]?.logo ?? null,
        };
      }
      return out;
    },
    // DETAILS for the Character at a roster slot: [{ key, label, value }]
    // after the three blank rows. In a network game a player's ends with
    // the path the server reaches him by (the port's own row: the original
    // shows its CONNECTION column only on Xbox Live).
    personal(slot) {
      const character = battle.characters[slot];
      if (!character) return [];
      const rows = personalStats(character, battle.characters, { label, name });
      if (network && character.human) rows.push({ key: "ifs.MPLobby.qos_header", label: "CONNECTION", value: LINK_LABELS[character.link?.kind] ?? "" });
      return rows;
    },
    // AWARDS: [{ key, label, text, team }], team the winner's (null: none).
    awards() {
      return awards(battle.characters, { name }).map((award) => ({ key: award.key, label: award.label, text: award.text, team: award.character?.team ?? null }));
    },
    humanTeam,
    relation,
    // { over, gameOver, finished }: a team has lost; the scores have
    // stopped; the end-of-match screens take over.
    state: () => ({ over: Boolean(battle.over), gameOver: Boolean(battle.gameOver), finished: Boolean(battle.finished) }),
    commonModel,
    canRestart,
    restart,
  };
}
