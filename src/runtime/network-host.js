import { NetworkSession } from './network-session.js';
import { UdpTransport } from './udp-transport.js';

// The desktop online entry point (native UDP through the desktop network
// process); local battles inject LocalTransport.
export class NetworkHost extends NetworkSession {
  constructor(options = {}) { super({...options, transport:options.transport || new UdpTransport(options)}); }
}
