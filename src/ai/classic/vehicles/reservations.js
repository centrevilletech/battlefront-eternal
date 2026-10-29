// Seat reservations, as the 2004 game keeps them.
//
// A reservation is { candidate, time, owner, type }: the vehicle (a weak
// reference), when it was made or last refreshed, the reserving unit (a weak
// reference) and an integer type (1 for seats). It holds while
// now < time + 3 and both references still resolve; expired or broken ones
// are pruned as the table is read. Reserving again refreshes the timestamp
// of the matching record (same candidate, owner and type) instead of adding
// one.
//
// A vehicle's free capacity for a team is its seat count, less the live
// reservations of that type by units of that team, less (for seats, type 1)
// the seats players occupy; never below zero. Bots already aboard are not
// subtracted: the 2004 game counts players and reservations only. The
// boarding agent's own physical check (a free seat at the moment of entry)
// is what finally keeps two bots out of one seat.

export const RESERVATION_LIFE = 3; // s
export const SEAT = 1;

const resolve = (ref) => {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
};

export class Reservations {
  constructor(clock) {
    this.clock = clock; // () => now (s)
    this.records = [];
  }

  now() {
    return this.clock();
  }

  // Prune and return the live records.
  live() {
    const now = this.now();
    this.records = this.records.filter((r) => now < r.time + RESERVATION_LIFE && resolve(r.candidate) && resolve(r.owner));
    return this.records;
  }

  find(candidate, owner, type = SEAT) {
    for (const r of this.live()) {
      if (r.type === type && resolve(r.candidate) === candidate && resolve(r.owner) === owner) return r;
    }
    return null;
  }

  has(candidate, owner, type = SEAT) {
    return Boolean(this.find(candidate, owner, type));
  }

  // Live reservations of `type` on `candidate` by units of `team`.
  count(candidate, team, type = SEAT) {
    let n = 0;
    for (const r of this.live()) {
      if (r.type !== type || resolve(r.candidate) !== candidate) continue;
      if (resolve(r.owner)?.team === team) n += 1;
    }
    return n;
  }

  // Create, or refresh the timestamp of the matching record.
  reserve(candidate, owner, type = SEAT) {
    if (!candidate || !owner) return null;
    const found = this.find(candidate, owner, type);
    if (found) {
      found.time = this.now();
      return found;
    }
    const record = {
      candidate: { unit: candidate, generation: candidate.generation },
      owner: { unit: owner, generation: owner.generation },
      time: this.now(),
      type,
    };
    this.records.push(record);
    return record;
  }

  release(candidate, owner, type = SEAT) {
    this.records = this.records.filter((r) => !(r.type === type && r.candidate.unit === candidate && r.owner.unit === owner));
  }

  clear() {
    this.records.length = 0;
  }
}

// Free capacity. seats: the candidate's seat count; humans: the
// seats players occupy (subtracted for seat reservations only).
export function freeCapacity(reservations, candidate, team, type, seats, humans) {
  const reserved = reservations.count(candidate, team, type);
  const players = type === SEAT ? humans : 0;
  if (seats < players + reserved) return 0;
  return seats - players - reserved;
}
