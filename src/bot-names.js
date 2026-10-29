import { ClassicRandom } from "./ai/random.js";

// Bot names, as the 2004 game makes them when a team's Characters are
// created at the battle's start (its Team's MakeAIName; docs/scoreboard.md).
//
// Each team draws from a generator of its own, seeded with its side type
// (Rebels 1, Empire 2, Republic 3, CIS 4) just before its Characters are
// made. Nothing else draws from it and the human doesn't, so a side's bots
// are named the same every battle: the Republic's start DELTA 855,
// THETA 269, CHI 987, OMEGA 370.
//
// A name is a first name picked from the side's list (or two random letters
// A-Z when the list is empty), a space, then a last name picked from its
// list (or three random digits). A pick is a raw draw modulo the list's
// length, unsigned; the lists hold up to 64 names. Locals (side type 5) are
// all called the level's local name ("GEONOSIAN") and draw nothing.

export const LOCALS = 5;
const LIST_LIMIT = 64;

// lists: ai-mission.json `names` ({ 0..4: { first, last }, local }); faction:
// the team's side type. Returns a function giving the team's next name.
export function botNames(lists, faction) {
  if (faction === LOCALS) return () => lists?.local || "LOCAL";
  const side = faction >= 1 && faction <= 4 ? faction : 0;
  const random = new ClassicRandom(side);
  const first = (lists?.[side]?.first || []).slice(0, LIST_LIMIT);
  const last = (lists?.[side]?.last || []).slice(0, LIST_LIMIT);
  const pick = (count) => random.next() % count;
  const letter = () => String.fromCharCode(65 + pick(26));
  const digit = () => String.fromCharCode(48 + pick(10));
  return () => {
    const given = first.length ? first[pick(first.length)] : letter() + letter();
    const family = last.length ? last[pick(last.length)] : digit() + digit() + digit();
    return `${given} ${family}`;
  };
}

// The human's name offline: the original's profile name, or "Player n" (his
// index + 1) without one, which is what the port has. The one place it is
// made: his Character takes it, and the end-of-match table and the message
// feed read it from there.
export function humanName(index = 0) {
  return `Player ${index + 1}`;
}
