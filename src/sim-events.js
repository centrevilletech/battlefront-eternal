// What happened in the simulation this tick, for anything that reacts to it
// later: the AI's hearing and alerts, a kill feed, replays. Gameplay emits an
// event once; each reader keeps its own cursor and reads at its own pace.
//
// Records are value snapshots (positions copied into plain arrays), so a
// reader never sees a moving object's later state. The journal is a ring of
// fixed size; a reader that falls a whole ring behind loses the oldest
// records and is told how many (`dropped`), rather than reading stale slots.
//
// Audio keeps its own hook (src/audio/events.js); this journal doesn't
// replace it.

const CAPACITY = 4096;

export class SimEvents {
  constructor(capacity = CAPACITY) {
    this.capacity = capacity;
    this.ring = new Array(capacity).fill(null);
    this.seq = 0; // sequence number of the next record
    this.time = 0; // simulation time, set by the owner each tick
    this.listeners = new Set();
  }

  // Record an event. `fields` is copied; Vector3s in it become [x, y, z].
  emit(kind, fields = {}) {
    const record = { seq: this.seq, time: this.time, kind };
    for (const key in fields) {
      const value = fields[key];
      record[key] = value && value.isVector3 ? [value.x, value.y, value.z] : value;
    }
    this.ring[this.seq % this.capacity] = record;
    this.seq += 1;
    for (const listener of this.listeners) listener(record);
    return record;
  }

  // A reader with its own cursor, starting at the next event.
  reader() {
    return new EventReader(this);
  }

  // Immediate callbacks, for consumers that must not miss an event but must
  // not change the simulation from inside it (queue work for the next tick).
  listen(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  reset() {
    this.ring.fill(null);
    this.seq = 0;
  }
}

class EventReader {
  constructor(journal) {
    this.journal = journal;
    this.cursor = journal.seq;
    this.dropped = 0;
  }

  // Calls fn(record) for each event since the last read.
  read(fn) {
    const j = this.journal;
    if (j.seq - this.cursor > j.capacity) {
      this.dropped += j.seq - this.cursor - j.capacity;
      this.cursor = j.seq - j.capacity;
    }
    while (this.cursor < j.seq) {
      const record = j.ring[this.cursor % j.capacity];
      this.cursor += 1;
      if (record) fn(record);
    }
  }
}
