// Rider skeletons and clips enter once. Ordinary output contains compact
// collision samples, never the hull's rig or a JavaScript bone reader.
export default { records: {
  F2RiderBone: { name: 'str', parent: 'i32', p: 'vec3', q: 'quat', s: 'vec3', bind: 'vec3|null' },
  F2RiderClip: { hull: 'ref:vehicle|null', turret: 'ref:turret|null', index: 'u32', clip: 'i32', nine: 'bool', time: 'f64|null' },
  F2RiderBirth: { unit: 'ref:unit', life: 'f64', scale: 'f64', rootBind: 'vec3|null', bones: 'list:rec:F2RiderBone', clips: 'list:rec:F2RiderClip' },
  F2RiderShape: { capsule: 'bool', head: 'bool', center: 'vec3', half: 'vec3', radius: 'f64' },
  F2RiderOut: { unit: 'ref:unit', life: 'f64', show: 'bool', clip: 'i32', pitch: 'f64', turn: 'f64', time: 'f64|null', drop: 'vec3|null', shapes: 'list:rec:F2RiderShape' },
} };
