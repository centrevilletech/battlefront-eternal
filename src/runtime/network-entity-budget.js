// Fixed profile budget charges offered application bytes until an offer is
// dropped; admitted bytes remain charged. Uncharged own-correction fallback
// packets and SCTP/DTLS/IP overhead are outside this token counter.
//
// This is server-to-client entity payload, not the client's upload budget.
// The default 100 KB/s is 800 kbit/s of download before transport overhead.
// Raising it also increases the client's reverse-direction SCTP receipts;
// plentiful server bandwidth does not make that upload cost free. Operators
// configure stateBytesPerSecond for the clients' constrained paths, and use
// tools/net-probe.mjs shaped-link measurements to check actual IP traffic.
export const DEFAULT_STATE_BYTES_PER_SECOND=100000;
export const MIN_STATE_BYTES_PER_SECOND=20000, MAX_STATE_BYTES_PER_SECOND=500000;
// Roughly one publication's worth of datagrams may be sent at once; a burst
// must still admit at least two 1,100-byte messages (own state and remotes).
export const burstFor=bytesPerSecond=>Math.max(2200,Math.min(11000,Math.round(bytesPerSecond/15)));
export function readStateBudget({stateBytesPerSecond}={}) {
  const rate=stateBytesPerSecond===undefined||stateBytesPerSecond===null?DEFAULT_STATE_BYTES_PER_SECOND:stateBytesPerSecond;
  if(!Number.isSafeInteger(rate)||rate<MIN_STATE_BYTES_PER_SECOND||rate>MAX_STATE_BYTES_PER_SECOND)throw Error(`stateBytesPerSecond must be an integer between ${MIN_STATE_BYTES_PER_SECOND} and ${MAX_STATE_BYTES_PER_SECOND}`);
  return {bytesPerSecond:rate,burstBytes:burstFor(rate)};
}
export class EntityBudget {
  constructor({bytesPerSecond=DEFAULT_STATE_BYTES_PER_SECOND,burstBytes=burstFor(bytesPerSecond),now=performance.now()}={}) {
    if(!Number.isFinite(bytesPerSecond)||bytesPerSecond<=0||!Number.isSafeInteger(burstBytes)||burstBytes<1100)throw Error('invalid entity budget');
    this.rate=bytesPerSecond;this.burst=burstBytes;this.tokens=burstBytes;this.at=now;this.offeredBytes=0;this.denied=0;
  }
  available(now=performance.now()) {
    if(!Number.isFinite(now))throw Error('invalid budget clock');
    this.tokens=Math.min(this.burst,this.tokens+Math.max(0,now-this.at)*this.rate/1000);this.at=Math.max(this.at,now);
    return Math.floor(this.tokens);
  }
  take(bytes,now=performance.now()) {
    if(!Number.isSafeInteger(bytes)||bytes<1||bytes>1100)throw Error('invalid entity message size');
    if(this.available(now)<bytes){this.denied++;return false;}
    this.tokens-=bytes;this.offeredBytes+=bytes;return true;
  }
  // Admission downstream (GameSession) can still drop an offered packet;
  // those bytes were never sent and must not stay charged to the budget.
  refund(bytes,now=performance.now()) {
    if(!Number.isSafeInteger(bytes)||bytes<1||bytes>1100)throw Error('invalid entity message size');
    this.available(now);this.tokens=Math.min(this.burst,this.tokens+bytes);this.offeredBytes=Math.max(0,this.offeredBytes-bytes);
  }
}
