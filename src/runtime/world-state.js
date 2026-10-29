// Canonical value-only state for replay parity. Runtime objects and references
// never cross a host boundary; a lease is identified by id plus generation.
export function eventValue(event) {
  const result = {};
  for (const [key, value] of Object.entries(event)) {
    result[key] = value?.unit && Number.isInteger(value.generation)
      ? { id: value.unit.id, generation: value.generation }
      : value?.isVector3 ? value.toArray() : value;
  }
  return result;
}
export function worldState(g) {
  return {tick:g.worldClock.tick,random:g.randomState(),
    units:g.units.all().filter(u=>u.soldier).map(u=>[u.id,u.generation,u.team,u.alive,...u.position.toArray(),u.health,u.soldier.aimYaw,u.soldier.aimPitch]),
    posts:g.posts.posts.map(p=>[p.team,p.progress,p.neutralize,p.toward,p.contested]),tickets:g.battle.reinforcements,
    // The scoreboard's numbers, per Character in roster order.
    scores:g.battle.characters.map(c=>[c.team,c.name,c.kills,c.teamKills,c.deaths,c.captures]),gameOver:g.battle.gameOver,
    weapons:g.combat.arsenal.weapons.map(w=>[w.clip,w.ammo,w.heat]),
    vehicles:g.vehicles?.vehicles.map(v=>[v.name,v.alive,v.health,...v.position.toArray(),...v.velocity.toArray(),v.yaw,v.pitch,...v.quaternion.toArray(),v.flightState,v.airspeed,v.power,v.dying,
      v.seats.map(s=>[s.lease?.unit?.id ?? null,s.lease?.generation ?? null,s.aimYaw,s.pitch])]) || [],
    grenades:g.ordnance.grenades.map(g=>[...g.position.toArray(),...g.velocity.toArray(),g.life,g.gone]),
    humanState:g.spawnPort.humanState(),
    events:g.simEvents.ring.filter(Boolean).map(eventValue)};
}
