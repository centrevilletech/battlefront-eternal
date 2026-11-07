import { open, readFile } from 'node:fs/promises';
import { gunzipSync } from 'node:zlib';

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
      if(blob.compression && blob.compression!=='gzip')throw Error('Unsupported replay compression');
      // Earlier stage captures compress each blob. Decode their container,
      // then use the same bounded argument transport as raw actor captures.
      const decoded=blob.compression==='gzip'?gunzipSync(await readFile(blob.file)):null;
      const file = decoded ? null : await open(blob.file, 'r');
      try {
        const size = decoded ? decoded.length : (await file.stat()).size;
        await call(function (key, type, bytes) { this[key] = new window[type](new ArrayBuffer(bytes)); return true; }, [key, blob.type, size]);
        let offset = 0;
        while (offset < size) {
          const length=Math.min(chunkBytes,size-offset);
          const bytesRead=decoded ? length : (await file.read(buffer,0,length,offset)).bytesRead;
          if(decoded)buffer.set(decoded.subarray(offset,offset+bytesRead));
          if (!bytesRead) throw Error('Short replay blob read');
          const written = await call(function (key, offset, text) {
            const bytes = new Uint8Array(this[key].buffer), raw = atob(text);
            for (let i = 0; i < raw.length; i++) bytes[offset + i] = raw.charCodeAt(i);
            return raw.length;
          }, [key, offset, buffer.subarray(0, bytesRead).toString('base64')]);
          if (written?.value !== bytesRead) throw Error('Short replay blob upload');
          offset += bytesRead;
        }
      } finally { await file?.close(); }
    }
  } finally { await connection.psend('Runtime.releaseObject', { objectId: table.objectId }); }
}
