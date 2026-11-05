// The page reserves output storage and waits for the first PCM window.
// Network/decode work and subsequent chunks bypass the page entirely.
let serial = 0;
export function attachAudioStream(asset, node, id, waits, fail) {
  const token = ++serial, channel = new MessageChannel();
  const ready = new Promise((resolve, reject) => waits.set(token, { resolve, reject }));
  asset.onclose = () => { waits.get(token)?.reject(new DOMException("audio decoding cancelled", "AbortError")); waits.delete(token); };
  asset.onerror = (error) => { waits.get(token)?.reject(error); waits.delete(token); fail(error); };
  node.port.postMessage({ type: "stream", id, token, meta: { length: asset.length, sampleRate: asset.sampleRate, numberOfChannels: asset.numberOfChannels }, port: channel.port1 }, [channel.port1]);
  asset.attach(channel.port2);
  return ready;
}
