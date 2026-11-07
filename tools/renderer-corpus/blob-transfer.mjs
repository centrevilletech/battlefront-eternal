import { open } from 'node:fs/promises';

// Send bytes as CDP arguments, not script literals retained by the debugger.
// A bounded chunk also avoids Uint8Array.from(string)'s large iterable array.
export async function uploadReplayBlobs(connection, blobs, chunkBytes = 1024 * 1024) {
  if (!Number.isInteger(chunkBytes) || chunkBytes < 1) throw Error('Invalid replay upload chunk size');
  const checked = reply => {
    if (reply.exceptionDetails) throw Error('Replay upload: ' + (reply.exceptionDetails.exception?.description || reply.exceptionDetails.text));
    return reply.result;
  };
  const table = checked(await connection.psend('Runtime.evaluate', { expression: 'window.__corpus.payload.blobs', returnByValue: false }));
  if (!table?.objectId) throw Error('Replay blob table unavailable');
  const call = async (fn, values) => checked(await connection.psend('Runtime.callFunctionOn', {
    objectId: table.objectId, functionDeclaration: fn.toString(),
    arguments: values.map(value => ({ value })), returnByValue: true,
  }));
  const buffer = Buffer.allocUnsafe(chunkBytes);
  try {
    for (const [key, blob] of Object.entries(blobs)) {
      const file = await open(blob.file, 'r');
      try {
        const { size } = await file.stat();
        await call(function (key, type, bytes) { this[key] = new window[type](new ArrayBuffer(bytes)); return true; }, [key, blob.type, size]);
        let offset = 0;
        while (offset < size) {
          const { bytesRead } = await file.read(buffer, 0, Math.min(chunkBytes, size - offset), offset);
          if (!bytesRead) throw Error('Short replay blob read');
          const written = await call(function (key, offset, text) {
            const bytes = new Uint8Array(this[key].buffer), raw = atob(text);
            for (let i = 0; i < raw.length; i++) bytes[offset + i] = raw.charCodeAt(i);
            return raw.length;
          }, [key, offset, buffer.subarray(0, bytesRead).toString('base64')]);
          if (written?.value !== bytesRead) throw Error('Short replay blob upload');
          offset += bytesRead;
        }
      } finally { await file.close(); }
    }
  } finally { await connection.psend('Runtime.releaseObject', { objectId: table.objectId }); }
}
