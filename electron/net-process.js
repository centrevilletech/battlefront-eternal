// The desktop client's privileged network process (an Electron utility
// process). It owns the UDP sockets and the player's identity key; the
// sandboxed renderer gets one MessagePort per battle and sees only
// authenticated session messages (net-service.js). Packets never wait
// behind the render thread or the main process's window and IPC work.
import './slow-links.js';
import { createNetService } from './net-service.js';

const service = createNetService();
process.parentPort.on('message', ({ data, ports }) => {
  if (data?.type === 'init') service.setIdentity(data.identity);
  else if (data?.type === 'session' && ports?.[0]) {
    const port = ports[0];
    service.serve({
      post: message => port.postMessage(message),
      onMessage: handler => port.on('message', event => handler(event.data)),
      onClose: handler => port.on('close', handler),
      close: () => port.close(),
    });
    port.start();
  }
});
