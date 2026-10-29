// Bounded ordered action edges. Set-compatible facade for existing debug
// scripts, with counts instead of silently collapsing repeated presses.
export class ActionQueue {
  constructor(capacity = 2048) {
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error("invalid action capacity");
    this.capacity = capacity; this.items = new Array(capacity);
    this.read = 0; this.write = 0; this.length = 0; this.counts = new Map();
  }
  get size() { return this.length; }
  has(name) { return (this.counts.get(name) || 0) > 0; }
  add(name) {
    if (this.write - this.read === this.capacity) {
      const active = [...this];
      if (active.length === this.capacity) throw new Error('input action queue overflow');
      this.items = new Array(this.capacity);
      active.forEach((value, index) => { this.items[index] = value; });
      this.read = 0; this.write = active.length;
    }
    this.items[this.write++ % this.capacity] = name; this.length++;
    this.counts.set(name, (this.counts.get(name) || 0) + 1);
    return this;
  }
  delete(name) {
    if (!this.has(name)) return false;
    for (let i = this.read; i < this.write; i++) if (this.items[i % this.capacity] === name) {
      this.items[i % this.capacity] = null; this.length--;
      this.counts.set(name, this.counts.get(name) - 1);
      while (this.read < this.write && this.items[this.read % this.capacity] === null) this.read++;
      if (this.read === this.write) this.clear();
      return true;
    }
    return false;
  }
  clear() { this.read = this.write = this.length = 0; this.counts.clear(); }
  *values() { for (let i = this.read; i < this.write; i++) if (this.items[i % this.capacity] !== null) yield this.items[i % this.capacity]; }
  [Symbol.iterator]() { return this.values(); }
}
