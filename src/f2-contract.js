// Store snapshots rather than references to mutable vectors. The caller supplies
// only the fields owned by F2; foreign seat/weapon/tether state is excluded.
export class F2MirrorCheck {
  constructor() { this.values = new Map(); }
  published(object, fields) {
    let saved = this.values.get(object);
    if (!saved || saved.length !== fields.length) {
      saved = new Array(fields.length);
      for (let i = 0; i < fields.length; i++) saved[i] = [fields[i][0], fields[i][1]];
      this.values.set(object, saved);
    } else for (let i = 0; i < fields.length; i++) {
      saved[i][0] = fields[i][0]; saved[i][1] = fields[i][1];
    }
  }
  check(object, read) {
    for (const [path, value] of this.values.get(object) ?? []) {
      if (!Object.is(read(object, path), value)) throw Error(`F2 mirror write: ${path}`);
    }
  }
  retire(object) { this.values.delete(object); }
}
