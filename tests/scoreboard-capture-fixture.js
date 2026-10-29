// The retail end tables as the original game showed them, as scoreboard port
// rows, for checks and side-by-sides:
//
// - geo1: Geonosis in Instant Action, the player on CIS (team 2), CIS won:
//   REPUBLIC : 0, CIS : 89. Thirty Republic rows and twenty-five CIS, the
//   table scrolled to its end: fourteen rows a side above the view.
// - nab1: Naboo Plains in a LAN game, the player on the Republic (team 2)
//   at -1 kills after a suicide: CIS : 0, REPUBLIC : 45. Seventeen rows a
//   side, scrolled down one.
// - hot1: Hoth in Instant Action, the player on the Empire (team 1, the
//   left) after one RESPAWN: EMPIRE : 249, REBELS : 0. Twenty-eight rows a
//   side, scrolled to the end. With the player's DETAILS and the AWARDS the
//   original showed.
//
// The rows above the view are stand-ins with more kills than any shown, so
// they sort first.
const TABLES = {
  geo1: {
    titles: { 1: ["REPUBLIC", 0, "rep"], 2: ["CIS", 89, "cis"] },
    hidden: 14,
    lists: {
      1: [["PI 610", 6, 6, 0], ["DELTA 607", 6, 6, 0], ["UPSILON 941", 5, 4, 0], ["OMEGA 370", 5, 5, 0], ["ALPHA 989", 5, 7, 0],
        ["RHO 571", 5, 9, 0], ["CHI 178", 5, 9, 0], ["RHO 773", 4, 7, 0], ["TAU 753", 4, 8, 0], ["BETA 030", 4, 8, 0], ["OMIKRON 621", 3, 5, 1],
        ["EPSILON 905", 3, 6, 0], ["EPSILON 497", 2, 5, 1], ["TAU 378", 2, 5, 0], ["THETA 269", 1, 2, 1], ["PSI 487", 1, 8, 0]],
      2: [["UNIT 466", 3, 6, 0], ["UNIT 538", 3, 9, 1], ["UNIT 843", 3, 10, 1], ["UNIT 182", 2, 3, 0], ["UNIT 754", 2, 9, 0],
        ["UNIT 397", 2, 10, 1], ["UNIT 065", 1, 2, 0], ["UNIT 538", 1, 6, 0], ["UNIT 729", 0, 4, 3], ["UNIT 032", 0, 4, 1],
        ["Player 1", 0, 5, 0, true]],
    },
  },
  nab1: {
    titles: { 1: ["CIS", 0, "cis"], 2: ["REPUBLIC", 45, "rep"] },
    hidden: 1,
    lists: {
      1: [["UNIT 167", 4, 1, 0], ["UNIT 466", 4, 2, 1], ["UNIT 196", 4, 3, 0], ["UNIT 165", 4, 4, 0], ["UNIT 065", 4, 6, 0],
        ["UNIT 397", 4, 7, 0], ["UNIT 503", 3, 2, 1], ["UNIT 197", 3, 5, 0], ["UNIT 538", 3, 6, 0], ["UNIT 879", 2, 3, 0],
        ["UNIT 182", 1, 3, 0], ["UNIT 892", 1, 4, 0], ["UNIT 576", 1, 5, 0], ["UNIT 032", 1, 5, 0], ["UNIT 253", 0, 2, 0], ["UNIT 790", 0, 5, 1]],
      2: [["NU 783", 3, 4, 0], ["EPSILON 497", 3, 5, 0], ["ALPHA 332", 3, 5, 0], ["PHI 022", 3, 6, 0], ["UPSILON 941", 2, 3, 0],
        ["THETA 269", 2, 3, 0], ["RHO 571", 2, 4, 0], ["CHI 987", 2, 4, 0], ["BETA 030", 2, 4, 0], ["GAMMA 022", 2, 5, 0],
        ["TAU 378", 1, 4, 0], ["RHO 557", 1, 5, 0], ["PHI 854", 1, 6, 0], ["CHI 178", -1, 2, 1], ["Player 1", -1, 3, 0, true], ["PSI 487", -4, 2, 2]],
    },
  },
};
TABLES.hot1 = {
  titles: { 1: ["EMPIRE", 249, "imp"], 2: ["REBELS", 0, "all"] },
  hidden: 12,
  player: 1,
  lists: {
    1: [...["RH 270", "QO 449", "QO 388", "PG 134", "OL 425", "KL 987", "KG 077", "JU 535", "JM 221", "JJ 579", "JE 929", "HD 781", "DX 178",
      "DARTH VADER", "CX 512"].map((name) => [name, 0, 0, 0]), ["Player 1", 0, 1, 0, true]],
    2: [...["JENS HANTONAR", "JACEN QE-KORA", "HAN PORINOM", "GIZ GUNNAR", "FIN ULRAND", "EVAN NORDAN", "DORAN KROTAN", "DEVON GOLDAM",
      "DAX OLESA", "BEN ULRAND", "BEN PULASTRA", "ASH ULRAND", "ARAN NOMANTE", "ARAN GUNNAR", "ALLYN FERA"].map((name) => [name, 0, 0, 0]),
    ["LUKE SKYWALKER", 0, 1, 0]],
  },
  personal: ["0", "STORMTROOPER 0:16", "BLASTER RIFLE 0:11", "", " 0:12", "0", "0", "", ""],
  awards: [["", null], ["", null], ["GIZ GUNNAR (0:38)", 2], ["", null], ["", null], ["", null], ["PLAYER 1 (1)", 1], ["", null]],
};
const LOGOS = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance" };
const PERSONAL = ["ifs.stats.headshots", "ifs.stats.favclass", "ifs.stats.favweapon", "ifs.stats.favvehicle", "ifs.stats.livingstreak",
  "ifs.stats.narrowescapes", "ifs.stats.vehiclesdestroyed", "ifs.stats.nemesis", "ifs.stats.bait"];
const PERSONAL_LABELS = ["HEAD SHOTS", "FAVORITE CLASS", "FAVORITE WEAPON", "FAVORITE VEHICLE", "LONGEST LIVING STREAK", "NARROW ESCAPES",
  "VEHICLES DESTROYED", "NEMESIS", "BAIT"];
const AWARDS = ["ifs.stats.tankbuster", "ifs.stats.killingspree", "ifs.stats.camper", "ifs.stats.survivalist", "ifs.stats.traitor",
  "ifs.stats.publicenemy", "ifs.stats.meat", "ifs.stats.deadeye"];
const AWARD_LABELS = ["TANK BUSTER", "KILLING SPREE", "CAMPER", "SURVIVALIST", "TRAITOR", "PUBLIC ENEMY", "BANTHA FODDER", "DEAD EYE"];

export function captureRows(which = "geo1") {
  const table = TABLES[which];
  const rows = [];
  let slot = 0;
  for (const team of [1, 2]) {
    for (let i = 0; i < table.hidden; i++) rows.push({ slot: slot++, team, name: `ABOVE ${i}`, kills: 40 - i, deaths: 0, points: 0, human: false, local: false });
    for (const [name, kills, deaths, points, local = false] of table.lists[team]) rows.push({ slot: slot++, team, name, kills, deaths, points, human: local, local });
  }
  return rows;
}

// The board's port, answering with a capture's rows and titles (the player
// on team 2 unless the table says), and its DETAILS and AWARDS when the
// capture has them.
export function capturePort(port, which = "geo1") {
  const table = TABLES[which];
  return {
    ...port,
    roster: () => captureRows(which),
    humanTeam: () => table.player ?? 2,
    personal: (slot) => (table.personal ? table.personal.map((value, i) => ({ key: PERSONAL[i], label: PERSONAL_LABELS[i], value })) : port.personal(slot)),
    awards: () => (table.awards ? table.awards.map(([text, team], i) => ({ key: AWARDS[i], label: AWARD_LABELS[i], text, team })) : port.awards()),
    relation: (a, b) => (a === b ? 1 : -1),
    teams: () => Object.fromEntries([1, 2].map((team) => {
      const [name, reinforcements, side] = table.titles[team];
      return [team, { name, reinforcements, side, logo: LOGOS[side] }];
    })),
  };
}
