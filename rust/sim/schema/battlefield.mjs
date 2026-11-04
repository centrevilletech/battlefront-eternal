// Boundary configuration and live countdown commands. Death callbacks
// return the current Character list so its iteration stays live.
export default {
  enums: {
    BfOperation: ['outside', 'controlsOutside', 'update', 'warning', 'alpha'],
    BfQueryOp: ['body', 'vehicle', 'combatDead'],
    BfActionOp: ['kill', 'killDroid', 'wreck'],
  },
  records: {
    BfPolygon: { edges: 'list:f64x4' },
    BfConfig: { polygons: 'list:rec:BfPolygon' },
    BfWarning: { shown: 'bool', value: 'f64', rate: 'f64', tick: 'f64' },
    BfInput: { op: 'enum:BfOperation', index: 'f64', dt: 'f64', characters: 'list:ref:character', present: 'bool', position: 'f64x3', droid: 'f64x3|null', warning: 'rec:BfWarning|null', tick: 'f64', outside: 'bool' },
    BfCountdown: { character: 'ref:character', value: 'f64' },
    BfResult: { flag: 'bool', alpha: 'f64', warning: 'rec:BfWarning|null', countdowns: 'list:rec:BfCountdown' },
    BfQueryArgs: { op: 'enum:BfQueryOp', unit: 'ref:unit' },
    BfBody: { removed: 'bool', alive: 'bool', combatDead: 'bool', place: 'f64x3', vehicle: 'ref:vehicle|null', droid: 'f64x3|null' },
    BfQueryResult: { body: 'rec:BfBody|null', vehicle: 'ref:vehicle|null', flag: 'bool' },
    BfActionArgs: { op: 'enum:BfActionOp', unit: 'ref:unit|null', vehicle: 'ref:vehicle|null', countdowns: 'list:rec:BfCountdown' },
    BfCharacters: { characters: 'list:ref:character' },
  },
  imports: {
    battlefieldQuery: { args: 'BfQueryArgs', result: 'BfQueryResult' },
    battlefieldAction: { args: 'BfActionArgs', result: 'BfCharacters' },
  },
};
