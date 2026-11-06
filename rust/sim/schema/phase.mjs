// Foreign observations and events for the native battle phase. Battle,
// posts, Characters and spawning use their existing owned store kinds.
export default {
  enums: {
    BpQueryOp: ['members', 'buildingAttacker', 'vehicleAttacker', 'victims', 'draw'],
    BpActionOp: ['post', 'mobile', 'kill', 'effect', 'over'],
  },
  records: {
    BpInput: { dt: 'f64', spawning: 'bool' },
    BpMember: { unit: 'ref:unit', soldier: 'bool', alive: 'bool', removed: 'bool', seated: 'bool', held: 'bool', captures: 'bool', playerMounted: 'bool', team: 'f64', position: 'f64x3', character: 'ref:character|null' },
    BpHull: { vehicle: 'ref:vehicle', alive: 'bool', position: 'f64x3' },
    BpQueryArgs: { op: 'enum:BpQueryOp', building: 'ref:building|null', vehicle: 'ref:vehicle|null' },
    BpQueryResult: { members: 'list:rec:BpMember', hulls: 'list:rec:BpHull', character: 'ref:character|null', number: 'f64' },
    BpActionArgs: { op: 'enum:BpActionOp', post: 'rec:PostEvent|null', mobile: 'rec:PsMobileEvent|null', unit: 'ref:unit|null', vehicle: 'ref:vehicle|null', name: 'str|null', position: 'f64x3', number: 'f64' },
  },

};
