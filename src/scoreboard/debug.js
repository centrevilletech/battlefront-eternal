import { PEEK, END } from "./model.js";

// The scoreboard on demand over the running game, for checks and
// screenshots, whatever the battle is doing:
//   const { scoreboardDebug } = await import("/src/scoreboard/debug.js");
//   scoreboardDebug(__bf, "end");  // the end table; "peek" for Tab's
// The board is the game's own (__bf.scoreboard); clock: "manual" to step it
// by hand with update(dt).
export function scoreboardDebug(bf, state = "peek", { clock = null } = {}) {
  const board = bf.scoreboard;
  if (!board) throw new Error("no scoreboard (the page builds it; headless worlds don't)");
  if (clock) board.clock = clock;
  if (board.isOpen && board.state !== state) board.model.close();
  if (!board.isOpen) board.model.open(state === "end" ? END : PEEK);
  if (board.state === PEEK) board.input.tab.start(board.input.time);
  return board;
}
