// The local world's messages to the page, a step's worth at a time: one
// postMessage (one structured clone, one task on each side) for a tick's
// datagrams, acknowledgements and events instead of one each. send() queues
// in order and flush() posts the queue; whatever is sent outside a step goes
// once the current task's microtasks run. A lone message goes as before.
// Each post carries when it left (`sentAt`, milliseconds on the clock the
// page and the worker share: timeOrigin + now): the world's own time for
// its packets, however long a busy page takes to take them.
export function batchedPoster(post, now = () => performance.timeOrigin + performance.now()) {
  let items = null, transfer = null;
  const flush = () => {
    if (!items) return;
    const message = items.length === 1 ? items[0] : { batch: items }, moved = transfer;
    message.sentAt = now();
    items = transfer = null;
    post(message, moved);
  };
  const send = (channel, payload) => {
    if (!items) { items = []; transfer = []; queueMicrotask(flush); }
    items.push({ channel, payload });
    if (payload instanceof ArrayBuffer) transfer.push(payload);
  };
  return { send, flush };
}
// The page's side: a message's (channel, payload) pairs, in order.
export const postedItems = data => data.batch ?? [data];
