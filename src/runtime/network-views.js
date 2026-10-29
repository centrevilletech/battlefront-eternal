import * as THREE from 'three';

// AI scheduling uses authoritative bodies and aim, never a client camera
// position. The broad view conservatively covers first/third person cameras.
export function networkViews(game, players) {
  const world = game.ai?.classic?.ai?.world;
  if (!world) return () => {};
  const views = [];
  const direction = new THREE.Vector3(), target = new THREE.Vector3();
  world.networkViews = true;
  world.cameras = views;
  return () => {
    views.length = 0;
    for (const player of players.values()) {
      const unit = player.unit;
      if (player.spectator || player.disconnectedAt || !unit?.alive) continue;
      const camera = player.aiView ??= new THREE.PerspectiveCamera(100, 4, 0.1, 2000);
      camera.position.copy(unit.position);
      camera.position.y += 1.7;
      if (unit.seat?.vehicle && unit.seat.index === 0) {
        direction.set(0,0,1).applyQuaternion(unit.seat.vehicle.quaternion);
      } else {
        const seat = unit.seat?.turret ?? unit.seat?.vehicle?.seats[unit.seat.index];
        const yaw = seat?.aimYaw ?? unit.soldier.aimYaw;
        const pitch = seat?.pitch ?? unit.soldier.aimPitch;
        direction.set(Math.sin(yaw)*Math.cos(pitch), Math.sin(pitch), Math.cos(yaw)*Math.cos(pitch));
      }
      camera.lookAt(target.copy(camera.position).add(direction));
      camera.updateMatrixWorld();
      views.push(camera);
    }
  };
}
