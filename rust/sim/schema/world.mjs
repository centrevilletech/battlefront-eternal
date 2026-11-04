// Values at the remaining world-step boundaries. The crowd pass needs
// only cylinders, eligibility and positions while other phases migrate.
export default {
  records: {
    WsMember: { id: 'f64', position: 'vec3', tickCount: 'f64', walker: 'bool', posture: 'enum:Posture', seat: 'bool', held: 'bool', removed: 'bool', alive: 'bool', dead: 'bool' },
    WsSeparation: { members: 'list:rec:WsMember', player: 'f64|null', playerOut: 'bool', global: 'bool', camera: 'vec3' },
    WsSeparated: { position: 'vec3', tickCount: 'f64' },
    WsSeparationResult: { members: 'list:rec:WsSeparated' },
  },
};
