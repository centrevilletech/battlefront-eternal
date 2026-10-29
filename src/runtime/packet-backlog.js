// Ordered delta streams must recover through a complete keyframe whenever a
// renderer stalls. Dropping just one delta would corrupt both state and tables.
export class PacketBacklog {
  constructor({ read, resync = () => {}, onReset = () => {}, maxPackets = 120, maxTicks = 120, maxBytes = 8 * 1024 * 1024 } = {}) {
    this.read = read; this.resync = resync; this.onReset = onReset;
    this.maxPackets = maxPackets; this.maxTicks = maxTicks; this.maxBytes = maxBytes;
    this.packets = []; this.bytes = 0; this.latest = 0; this.epoch = null; this.awaitKeyframe = false;
  }
  reset() {
    this.packets.length = 0; this.bytes = 0;
    if (this.awaitKeyframe) return;
    this.awaitKeyframe = true; this.onReset(); this.resync();
  }
  receive(buffer) {
    const packet = this.read(buffer, meta => {
      if (!Number.isSafeInteger(meta.tick) || meta.tick < 0) throw Error('Invalid packet tick');
      // A same-tick roster mutation starts a fresh immutable journal epoch.
      // Validate it before ordering and before mutating binary tables.
      const epoch=meta.snapshotAck?.epoch ?? null;
      if(epoch!==null && (!Number.isSafeInteger(epoch)||epoch<0))throw Error('Invalid snapshot epoch');
      if(epoch!==null && this.epoch!==null && epoch<this.epoch)return false;
      const newEpoch=epoch!==this.epoch;
      if(newEpoch && !meta.keyframe){this.reset();return false;}
      if (!newEpoch && meta.tick <= this.latest && this.latest !== 0) return false;
      if (!meta.keyframe && meta.baseTick !== undefined && meta.baseTick !== this.latest) this.reset();
      if (!meta.keyframe && (this.packets.length >= this.maxPackets || this.bytes + buffer.byteLength > this.maxBytes ||
          this.packets.length && meta.tick - this.packets[0].tick > this.maxTicks)) this.reset();
      if (this.awaitKeyframe && !meta.keyframe) return false;
      if (meta.keyframe) {
        if (!meta.defs?.full) throw Error('Keyframe is missing complete binary tables');
        this.packets.length = 0; this.bytes = 0; this.awaitKeyframe = false;
      }
      this.epoch = epoch;
      this.latest = meta.tick;
      return true;
    });
    if (!packet) return null;
    this.packets.push(packet); this.bytes += packet.bytes;
    return packet;
  }
  shift() {
    const packet = this.packets.shift();
    if (packet) this.bytes -= packet.bytes;
    return packet;
  }
}
