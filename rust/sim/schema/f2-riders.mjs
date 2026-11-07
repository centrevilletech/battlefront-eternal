// Clip aliases enter once; geometry stays with the native unit sampler.
// Ordinary output contains animation and death-drop facts, not hit geometry.
export default { records: {
  F2RiderClip: { hull: 'ref:vehicle|null', turret: 'ref:turret|null', index: 'u32', clip: 'i32', nine: 'bool', time: 'f64|null' },
  F2RiderBirth: { unit: 'ref:unit', life: 'f64', scale: 'f64', clips: 'list:rec:F2RiderClip' },
  F2RiderOut: { unit: 'ref:unit', life: 'f64', show: 'bool', clip: 'i32', pitch: 'f64', turn: 'f64', time: 'f64|null', drop: 'vec3|null' },
} };
