// Main-process side of the network utility process (electron/net-process.js).
// Each battle gets a fresh MessageChannel: one end to the utility process,
// the other straight to the renderer, so game packets never pass through
// this process. The utility process restarts on demand if it exits.
import { MessageChannelMain, utilityProcess } from 'electron';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'net-process.js');
export function createNetHost({ identity }) {
  let child = null;
  const pages = new Set();
  const ensure = () => {
    if (child) return child;
    const started = child = utilityProcess.fork(script, [], { serviceName: 'Battlefront Eternal Network' });
    child.postMessage({ type: 'init', identity: identity() });
    // A web MessagePort never reports that its other end died: tell the
    // pages so a battle fails with a reason instead of freezing.
    child.once('exit', code => {
      if (child === started) child = null;
      for (const contents of pages) if (!contents.isDestroyed()) contents.send('bfe:net-exit', { code });
      pages.clear();
    });
    return child;
  };
  return {
    open(webContents, id) {
      const { port1, port2 } = new MessageChannelMain();
      ensure().postMessage({ type: 'session' }, [port1]);
      pages.add(webContents);
      webContents.postMessage('bfe:net-port', { id }, [port2]);
    },
    stop() { child?.kill(); child = null; },
  };
}
