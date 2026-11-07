// map.json's mission block as calls, for maps without ai-mission.json. It
// lacks relations, bonuses and the original order.
export function callsFromMapMission(mission) {
  const calls = [];
  if (!mission) return calls;
  for (const [team, t] of Object.entries(mission.teams || {})) {
    const n = Number(team);
    if (t.name) calls.push({ name: "SetTeamName", args: [n, t.name] });
    for (const [className, count] of t.classes || []) calls.push({ name: "AddUnitClass", args: [n, className, count] });
    if (t.hero) calls.push({ name: "SetHeroClass", args: [n, t.hero] });
    if (t.units != null) calls.push({ name: "SetUnitCount", args: [n, t.units] });
    if (t.reinforcements != null) calls.push({ name: "SetReinforcementCount", args: [n, t.reinforcements] });
  }
  if (mission.spawnDelay) calls.push({ name: "SetSpawnDelay", args: mission.spawnDelay });
  if (mission.attacker != null) calls.push({ name: "SetAttackingTeam", args: [mission.attacker] });
  return calls;
}

