import { NetBinding as B } from "./network-layout.js";
// The display binding is a cold identity/resource operation. Each frame calls
// the native ring once; body and hull phases read it directly in Rust, without
// a per-entity JS sample or a pose copied back into their phase inputs.
export class SnapshotPresentation {
  constructor(network, { maxEntities = 768, display = true } = {}) {
    this.timeline = network;
    this.network = network;
    network.timelineSetup();
    this.epoch = network.epoch;
    this.maxEntities = maxEntities;
    this.display = display;
    this.bindings = new Map();
    this.reported = false;
  }
  bindSoldier(id, generation, target) {
    return this.bind(id, generation, "soldier", target);
  }
  bindVehicle(id, generation, target) {
    return this.bind(id, generation, "vehicle", target);
  }
  // Identity/resource admission is cold. The retained binding names native
  // actor or rig handles; a displayed frame performs no JS binding walk.
  // Rebinding another generation invalidates its previous resource link.
  bind(id, generation, kind, target) {
    if (!Number.isSafeInteger(id) || id < 0 || !Number.isSafeInteger(generation) || generation < 0 || !target)
      throw Error("invalid snapshot presentation binding");
    if (this.epoch !== this.network.epoch) this.reset();
    const old = this.bindings.get(id);
    // A sampled life/class mismatch retires the native link. A later packet
    // may bind the same JS resource again, just as the prior binding map did.
    if (old) old.owner.refresh();
    if (
      old?.target === target &&
      old.generation === generation &&
      old.kind === kind &&
      target.nativeNetworkOwner === this.network &&
      old.owner.networkStates[old.key * B.STRIDE + B.ACTIVE]
    )
      return target;
    // Additional foreign views are refused once instead of growing a frame
    // buffer or throwing while a slow page drains a long receive backlog.
    if (!old && this.bindings.size === this.maxEntities) {
      // The native frame retires links without a JS map walk. Reclaim those
      // records only when cold admission actually reaches its fixed budget.
      for (const row of this.bindings.values()) {
        row.owner.refresh();
        if (!row.owner.networkStates[row.key * B.STRIDE + B.ACTIVE]) this.unbind(row.id);
      }
    }
    if (!old && this.bindings.size === this.maxEntities) {
      if (!this.reported) {
        this.reported = true;
        console.warn("snapshot presentation: additional views exceed the binding records");
      }
      return target;
    }
    this.unbind(id);
    const row = target.nativeDisplay;
    // Headless consumers need gameplay state without displayed resources.
    // A displayed page must have registered its resource before admission.
    if (!row) {
      if (this.display) throw Error("snapshot target has no native display resource");
      return target;
    }
    const owner = kind === "vehicle" ? row.owner : row.units,
      key = kind === "vehicle" ? row.rig.id : row.id;
    owner.refresh();
    owner.kernel.bind_network(
      this.network.kernel,
      key,
      id,
      generation,
      kind === "vehicle" ? 1 : kind === "droideka" ? 2 : kind === "soldier" ? 0 : NaN,
    );
    if (!owner.networkStates[key * B.STRIDE + B.ACTIVE]) return target;
    target.nativeNetworkOwner = this.network;
    target.nativeNetwork = true;
    this.bindings.set(id, { id, generation, kind, target, owner, key });
    return target;
  }
  // The native owner drops the identity link before this host record goes.
  // Nothing can keep writing samples into a reused actor through an old key.
  unbind(id) {
    const row = this.bindings.get(id);
    if (!row) return;
    if (row.target.nativeNetworkOwner === this.network) {
      row.owner.kernel.unbind_network(row.key);
      row.target.nativeNetwork = false;
      row.target.nativeNetworkOwner = null;
    }
    this.bindings.delete(id);
  }
  // Epoch changes and battle disposal release every link at a cold boundary.
  // Native ring storage is recycled by its receive owner, separately.
  reset() {
    for (const id of this.bindings.keys()) this.unbind(id);
    this.epoch = this.network.epoch;
  }
  // All body and hull resources see this same display clock and exclusion.
  // The frame returns no sampled objects or per-entity result readers.
  update(tick, playerId = null, vehicleId = null) {
    if (!Number.isFinite(tick)) throw Error("invalid snapshot presentation time");
    if (this.epoch !== this.network.epoch) this.reset();
    this.network.present(tick, playerId, vehicleId);
  }
}
