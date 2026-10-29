// For every turret class on each map: at yaw 0 / pitch 0, where the barrel
// (gun +z), the drum and the seat point compared with Turret.facing, and the
// node set the class names. Run: BFE_CONTENT_DIR=... node tools/turret-geometry-audit.mjs [maps]
import { headlessGame } from './headless-world.mjs';
import * as THREE from 'three';
const maps = (process.argv[2] || 'bes1,bes2,end1,geo1,hot1,kam1,kas2,nab1,nab2,rhn1,rhn2,tat1,tat2,yav1,yav2').split(',');
const deg = (r) => +(r * 180 / Math.PI).toFixed(1);
const yawOf = (v) => Math.atan2(v.x, v.z);
const wrap = (a) => Math.atan2(Math.sin(a), Math.cos(a));
for (const map of maps) {
  const game = await headlessGame(`map=${map}&side=rep&spawnscreen=0`).catch(e => { console.log(map, 'load failed', e.message); return null; });
  if (!game) continue;
  await game.ready;
  const seen = new Set();
  for (const t of game.live.turrets) {
    const cls = t.desc.class;
    if (seen.has(cls)) continue; seen.add(cls);
    if (t.state !== 'built') { t.show('built'); t.health = t.maxHealth; }
    t.yaw = 0; t.pitch = 0; t.gunYaw = 0; t.gunPitch = 0; t.present();
    t.rig.root.updateMatrixWorld(true);
    const V = () => new THREE.Vector3();
    const barrel = t.gun ? V().set(0, 0, 1).transformDirection(t.gun.matrixWorld) : null;
    const muzzle = t.muzzle(V(), V());
    const pivot = (t.gun || t.drum || t.rig.root).getWorldPosition(V());
    const toMuzzle = muzzle.clone().sub(pivot);
    const seat = t.seat ? t.seat.getWorldPosition(V()) : null;
    const s = t.desc.settings;
    // What happens when the gun is pitched up 20deg by Turret.aimGun's convention.
    let pitchUp = null;
    if (t.gun) { t.gunPitch = 20 * Math.PI / 180; t.present(); t.rig.root.updateMatrixWorld(true); const d = V().set(0,0,1).transformDirection(t.gun.matrixWorld); pitchUp = deg(Math.asin(d.y)); t.gunPitch = 0; t.present(); }
    console.log(JSON.stringify({ map, cls, facing: deg(t.facing),
      nodes: { drum: t.drum?.name ?? null, gun: t.gun?.name ?? null, seat: t.seat?.name ?? null, fire: t.firePoints.map(f => f.name) },
      settings: { TurretNodeName: s.TurretNodeName, AimerNodeName: s.AimerNodeName, PilotPosition: s.PilotPosition, FirePointName: s.FirePointName, Pilot9Pose: s.Pilot9Pose, PilotAnimation: s.PilotAnimation, EyePointOffset: s.EyePointOffset, TrackCenter: s.TrackCenter, TrackOffset: s.TrackOffset, PitchLimits: s.PitchLimits, AimerPitchLimits: s.AimerPitchLimits, AimerYawLimits: s.AimerYawLimits, YawLimits: s.YawLimits },
      barrelYawVsFacing: barrel ? deg(wrap(yawOf(barrel) - t.facing)) : null, barrelPitch: barrel ? deg(Math.asin(barrel.y)) : null,
      muzzleYawVsFacing: deg(wrap(yawOf(toMuzzle) - t.facing)), muzzleDist: +toMuzzle.length().toFixed(2),
      gunPitchUp20Gives: pitchUp,
      seatVsPos: seat ? seat.clone().sub(t.position).toArray().map(v => +v.toFixed(2)) : null,
      seatYawFromPivot: seat ? deg(wrap(yawOf(seat.clone().sub(pivot)) - t.facing)) : null,
      drumBindYaw: t.drumBind ? deg(new THREE.Euler().setFromQuaternion(t.drumBind, 'YXZ').y) : null,
      gunBind: t.gunBind ? new THREE.Euler().setFromQuaternion(t.gunBind, 'YXZ').toArray().slice(0,3).map(deg) : null,
    }));
  }
  game.dispose?.();
}
process.exit(0);
