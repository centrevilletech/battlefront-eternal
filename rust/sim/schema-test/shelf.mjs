// The generator's own test schema (tools/sim-schema.mjs): every type a
// field can have, so the generated code for each is compiled and tested
// (rust/sim/src/gen_test, tests/sim-gen) without being in the module.
// Nothing in the game has these kinds.
export default {
  enums: {
    Colour: ['red', 'green', 'blue'],
  },
  unions: {
    // Null, undefined, a literal, a kind the store refers to and one it holds.
    Holder: ['null', 'undefined', 'lit:nobody', 'ref:person', 'ref:thing'],
    // A number or a Vector3 (a shape's half: a radius, or a box's half size).
    Half: ['f64', 'vec3'],
    // One of two records with different fields, told apart by class.
    Body: ['rec:Legs', 'rec:Wheels'],
  },
  records: {
    Spot: { at: 'vec3', turn: 'quat', label: 'str|null', mark: 'enum:Colour', near: 'ref:person|undefined', kept: 'list:thing' },
    Ask: { n: 'f64', who: 'ref:person|null', things: 'list:thing', spot: 'rec:Spot|null' },
    Legs: { $class: 'Legs', stride: 'f64', side: 'list:f64' },
    Wheels: { $class: 'Wheels', count: 'u8', grip: 'vec3' },
    Shape: { radius: 'f64', half: 'union:Half' },
  },
  commands: { double: { args: 'Ask', result: 'Ask', rust: 'gen_tests::double' } },
  events: { noted: 'Spot' },
  imports: { lookUp: { args: 'Ask', result: 'Spot' } },
  kinds: {
    shelf: {
      singleton: true,
      fields: {
        things: 'list:thing',
        byName: 'map:list:thing',
        weights: 'map:f64',
        spots: 'map:rec:Spot',
        owners: 'map:ref:person',
        names: 'map:str|null',
      },
    },
    thing: {
      home: 'shelf.things',
      fields: {
        count: 'i32', size: 'u32', small: 'u8', light: 'f32', mass: 'f64', on: 'bool',
        where: 'vec3', facing: 'quat', pair: 'f64x2', name: 'str', colour: 'enum:Colour', def: 'def:goods',
        owner: 'ref:person|null', holder: 'union:Holder', spot: 'rec:Spot|null|undefined', friends: 'set:person',
        next: 'ref:thing|null', tags: 'map:str|undefined',
        hidden: { type: 'f64', mirror: false },
        boxed: { type: 'f64', js: 'box.size', mirror: false },
        // Lists of every sort of item, a matrix kept in place, and a path
        // through an array.
        marks: 'list:f64', probes: 'list:vec3', flags: 'list:bool', spots: 'list:rec:Spot', holders: 'list:union:Holder',
        matrix: 'mat4',
        firstSize: { type: 'f64', js: 'boxes.0.size', mirror: false },
        // Records in a list, and one of them by identity; a body of one of two classes.
        shapes: 'list:rec:Shape', hitSet: 'index:shapes|null', body: 'union:Body',
      },
      skip: { mesh: 'drawn by the page' },
    },
    person: { key: 'id', class: 'Person' },
  },
};
