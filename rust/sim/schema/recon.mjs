// Authoritative recon contexts and droids. Presentation and private page
// replay keep their own objects; these rows belong to the world only.
export default {
  enums: { F3ReconAction: ['attach', 'rebind', 'retire', 'exit', 'look', 'firstPerson', 'kill', 'enter'] },
  records: {
    // Content is encoded with the shared weapon loader, then converted once.
    // No class names or dynamic property lookup cross the flight boundary.
    F3ReconClass: { id: 'u32', value: 'list:f64', loadout: 'list:def:weapon', explosion: 'u32|null', muzzle: 'vec3' },
    F3ReconLevel: { weapon: 'def:weapon', level: 'u32', value: 'list:f64', ordnance: 'def:ordnance|null', strike: 'u32|null' },
    F3ReconStrike: { id: 'u32', value: 'list:f64', ordnance: 'def:ordnance' },
    F3ReconBinding: { action: 'enum:F3ReconAction', key: 'u32', owner: 'rec:F3UnitKey', seed: 'u32',
      droid: 'ref:reconDroid|null', dx: 'f64', dy: 'f64', sensitivity: 'f64', flag: 'bool' },
    F3ReconWeapon: { weapon: 'def:weapon', class: 'u32', immediate: 'bool' },
    // Bind fresh facades before later state notices decode references to them.
    F3ReconContextCreated: { slot: 'u32', generation: 'u32', key: 'u32', owner: 'rec:F3UnitKey' },
    F3ReconDroidCreated: { slot: 'u32', generation: 'u32', context: 'ref:reconContext', owner: 'rec:F3UnitKey',
      launchOwner: 'ref:ownerSnapshot', weapon: 'u32', class: 'u32', team: 'f64' },
    F3ReconContextState: { context: 'ref:reconContext', owner: 'rec:F3UnitKey', retired: 'bool', lastTick: 'f64', seed: 'u32',
      firstPerson: 'bool', camera: 'rec:ReconCamera', piloted: 'ref:reconDroid|null', lookX: 'f64', lookY: 'f64', zoom: 'bool',
      droids: 'list:reconDroid', strikes: 'list:reconDroid' },
    F3ReconDroidState: { droid: 'ref:reconDroid', position: 'vec3', velocity: 'vec3', yaw: 'f64', pitch: 'f64',
      health: 'f64', dead: 'bool', eye: 'vec3', aim: 'vec3', target: 'vec3', countdown: 'f64',
      salvo: 'rec:ReconSalvo|null', weapons: 'list:rec:F3WeaponView', selected: 'f64x2' },
    F3ReconRetired: { context: 'bool', slot: 'u32', generation: 'u32' },
    F3ReconShot: { droid: 'ref:reconDroid', owner: 'ref:ownerSnapshot', class: 'u32', weapon: 'u32', level: 'u32',
      position: 'vec3', yaw: 'f64', pitch: 'f64', channel: 'f64' },
    ReconCamera: { position: 'vec3', rotation: 'quat', reach: 'f64', fresh: 'bool', fov: 'f64' },
    ReconSalvo: { strike: 'u32', at: 'vec3', x: 'vec3', y: 'vec3', z: 'vec3', left: 'f64', wait: 'f64' },
  },
  events: {
    f3ReconContextCreated: 'F3ReconContextCreated', f3ReconDroidCreated: 'F3ReconDroidCreated',
    f3ReconContext: 'F3ReconContextState', f3ReconDroid: 'F3ReconDroidState',
    f3ReconRetired: 'F3ReconRetired', f3ReconShot: 'F3ReconShot',
  },
  kinds: {
    reconContext: { identity: true, fields: {
      key: 'u32', owner: 'rec:F3UnitKey', retired: 'bool', lastTick: 'f64', seed: 'u32',
      firstPerson: 'bool', camera: 'rec:ReconCamera', piloted: 'ref:reconDroid|null', lookX: 'f64', lookY: 'f64', zoom: 'bool',
      droids: 'list:reconDroid', strikes: 'list:reconDroid',
    } },
    reconDroid: { identity: true, fields: {
      context: 'ref:reconContext', owner: 'rec:F3UnitKey', launchOwner: 'ref:ownerSnapshot', weapon: 'u32', class: 'u32', team: 'f64',
      position: 'vec3', velocity: 'vec3', yaw: 'f64', pitch: 'f64', health: 'f64', dead: 'bool',
      eye: 'vec3', aim: 'vec3', target: 'vec3', countdown: 'f64', salvo: 'rec:ReconSalvo|null',
      weapons: 'list:rec:Weapon', selected: 'f64x2',
    } },
  },
};
