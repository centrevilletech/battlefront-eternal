import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_UNIT, playableClasses, sideOfClass, spawnPolicy, teamOfClass } from "../src/spawn-screen/policy.js";

// Theed's mission as map.json has it: the CIS are team 1, the Republic 2.
const theed = {
  sides: ["rep", "cis"],
  teams: {
    2: { name: "Republic", hero: "rep_inf_macewindu", classes: [["rep_inf_clone_trooper", 11], ["rep_inf_arc_trooper", 3], ["rep_inf_jet_trooper", 2]] },
    1: { name: "CIS", hero: "cis_inf_countdooku", classes: [["cis_inf_battledroid", 11], ["cis_inf_droideka", 2]] },
  },
};
// Hoth: no Republic class anywhere.
const hoth = {
  sides: ["all", "imp"],
  teams: {
    2: { name: "Alliance", classes: [["all_inf_soldiersnow", 10], ["all_inf_lukeskywalkersnow", 1]], hero: "all_inf_lukeskywalkersnow" },
    1: { name: "Empire", classes: [["imp_inf_storm_troopersnow", 10]] },
  },
};

test("with nothing in the URL the screen opens on the default team's first class", () => {
  const policy = spawnPolicy("", theed);
  assert.equal(policy.screen, true);
  assert.equal(policy.auto, null);
  assert.equal(policy.className, "rep_inf_clone_trooper");
  assert.equal(policy.team, 2);
  assert.equal(policy.side, "rep");
  // A mission without the old default unit: team 1's first class.
  const hothPolicy = spawnPolicy("?map=hot1", hoth);
  assert.equal(hothPolicy.screen, true);
  assert.equal(hothPolicy.className, "imp_inf_storm_troopersnow");
  assert.equal(hothPolicy.team, 1);
  assert.equal(hothPolicy.side, "imp");
});

test("unit= bypasses the screen and keeps today's choice", () => {
  const policy = spawnPolicy("?map=nab2&side=rep&unit=rep_inf_arc_trooper&ai=classic", theed);
  assert.equal(policy.screen, false);
  assert.deepEqual(policy.auto, { team: 2, className: "rep_inf_arc_trooper", post: null });
  assert.equal(policy.side, "rep");
  // Without side=, the side is the unit's own (it was always rep before; a
  // CIS unit= without side= used to fail to load).
  assert.equal(spawnPolicy("unit=rep_inf_clone_trooper", theed).side, "rep");
  assert.equal(spawnPolicy("unit=cis_inf_droideka", theed).side, "cis");
  assert.equal(spawnPolicy("unit=cis_inf_droideka", theed).team, 1);
  // A team's hero is played on that team (Mace Windu for the Republic).
  assert.equal(spawnPolicy("unit=rep_inf_macewindu", theed).team, 2);
  assert.equal(spawnPolicy("unit=cis_inf_countdooku", theed).team, 1);
  assert.equal(spawnPolicy("unit=all_inf_lukeskywalkersnow", hoth).team, 2);
  // A class no team lists is played on team 1, as before.
  assert.equal(spawnPolicy("unit=rep_inf_clone_trooper", hoth).team, 1);
  assert.equal(spawnPolicy("unit=rep_inf_clone_trooper", hoth).side, "rep");
});

test("side= alone picks that side's first non-hero class", () => {
  const policy = spawnPolicy("side=cis", theed);
  assert.equal(policy.screen, false);
  assert.equal(policy.className, "cis_inf_battledroid");
  assert.equal(policy.team, 1);
  assert.equal(policy.side, "cis");
  // Not the jet trooper, whatever the side.
  assert.equal(spawnPolicy("side=rep", theed).className, "rep_inf_clone_trooper");
  // The hero is never picked, even listed first.
  const heroFirst = { sides: ["all"], teams: { 1: { hero: "all_inf_lukeskywalker", classes: [["all_inf_lukeskywalker", 1], ["all_inf_soldier", 5]] } } };
  assert.equal(spawnPolicy("side=all", heroFirst).className, "all_inf_soldier");
});

test("spawn= alone bypasses with the old default unit", () => {
  const policy = spawnPolicy("spawn=CP3", theed);
  assert.equal(policy.screen, false);
  assert.equal(policy.className, DEFAULT_UNIT);
  assert.equal(policy.post, "CP3");
  assert.deepEqual(policy.auto, { team: 2, className: DEFAULT_UNIT, post: "CP3" });
});

test("spawnscreen=0|1 overrides either way", () => {
  assert.equal(spawnPolicy("spawnscreen=0", theed).screen, false);
  assert.equal(spawnPolicy("spawnscreen=0", theed).className, DEFAULT_UNIT);
  const forced = spawnPolicy("spawnscreen=1&unit=rep_inf_arc_trooper", theed);
  assert.equal(forced.screen, true);
  assert.equal(forced.auto, null);
  assert.equal(forced.className, "rep_inf_arc_trooper");
  // Empty values name nothing.
  assert.equal(spawnPolicy("unit=&side=", theed).screen, true);
});

test("the policy reads URLSearchParams, strings and plain objects alike", () => {
  const forms = [new URLSearchParams("unit=cis_inf_droideka"), "?unit=cis_inf_droideka", { unit: "cis_inf_droideka" }];
  const results = forms.map((form) => spawnPolicy(form, theed));
  for (const result of results) assert.deepEqual(result, results[0]);
  assert.equal(spawnPolicy(null, null).className, DEFAULT_UNIT);
});

test("roster helpers", () => {
  assert.deepEqual(playableClasses(theed.teams[2]), ["rep_inf_clone_trooper", "rep_inf_arc_trooper", "rep_inf_jet_trooper"]);
  assert.deepEqual(playableClasses(undefined), []);
  assert.equal(teamOfClass(theed, "cis_inf_droideka"), 1);
  assert.equal(teamOfClass(theed, "rep_inf_macewindu"), 2);
  assert.equal(teamOfClass(theed, "nobody"), 1);
  assert.equal(sideOfClass(theed, "imp_inf_storm_trooper"), "rep");
  assert.equal(sideOfClass(hoth, "imp_inf_storm_trooper"), "imp");
});

test("a network battle runs the screen and takes no unit or post pick from the URL", () => {
  const mission = { teams: { 1: { classes: [["rep_inf_clone_trooper"], ["rep_inf_jet_trooper"]] }, 2: { classes: [["cis_inf_battledroid_inf"]] } } };
  const policy = spawnPolicy("?map=nab2&side=rep&unit=cis_inf_battledroid_inf&spawn=cp3&network=1", mission);
  assert.equal(policy.screen, true);
  assert.equal(policy.auto, null);
  assert.equal(policy.side, "rep");
  assert.equal(policy.className, "rep_inf_clone_trooper", "the body loaded first comes from the side, never a stale pick");
  assert.equal(policy.post, null);
  assert.equal(spawnPolicy("?map=nab2&side=rep&network=1&spawnscreen=0", mission).screen, false, "the explicit override still wins");
});

test("the side loaded first follows the chosen class when the URL names a faction the mission lacks", () => {
  // A mission without the URL's side: Theed's historical battle (nab2c_h),
  // CIS against the Naboo Guard, under a URL that says side=rep.
  const theed = { sides: ["gar", "cis"], teams: { 1: { classes: [["cis_inf_battledroid_inf", 11], ["cis_inf_droideka", 2]] }, 2: { classes: [["gar_inf_soldier", 12], ["gar_inf_pilot", 5]] } } };
  const online = spawnPolicy("?map=nab2&side=rep&ai=classic&seed=1&network=1", theed);
  assert.equal(online.screen, true);
  assert.equal(online.className, "cis_inf_battledroid_inf");
  assert.equal(online.side, "cis", "the body loaded first comes from its own side, not the URL's");
  assert.equal(online.team, 1);
  const offline = spawnPolicy("?map=nab2&side=rep", theed);
  assert.equal(offline.className, DEFAULT_UNIT);
  assert.equal(offline.side, "rep", "without the screen the old default Republic body and side stand");
  const guard = spawnPolicy("?map=nab2&side=rep&unit=gar_inf_soldier", theed);
  assert.equal(guard.side, "gar"); assert.equal(guard.team, 2);
});
