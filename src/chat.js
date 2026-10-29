// Local chat today; a transport can call receive() for server/player messages
// and replace send() without coupling text entry to the simulation worker.
export const CHAT_COLORS = { global: '#ffff40', team: '#50ff70', server: '#60b8ff' };
export const CHAT_LIMIT = 240;
export class ChatLog {
  constructor() { this.messages = []; }
  receive({ channel = 'global', name = '', text = '' }, now = Date.now()) {
    if (!Object.hasOwn(CHAT_COLORS, channel)) return null;
    text = String(text).replace(/[\x00-\x1f\x7f]/g, ' ').trim().slice(0, CHAT_LIMIT);
    if (!text) return null;
    const message = { channel, name: String(name).replace(/[\x00-\x1f\x7f]/g, '').slice(0, 48), text, time: now };
    this.messages.push(message);
    if (this.messages.length > 8) this.messages.shift();
    return message;
  }
}

// canOpen(): whether T or Y may open the entry now; controlsFree(): whether
// the game's controls come back when it closes (not under a screen that
// holds them, such as the spawn screen, where chat still opens).
export function createChat({ controls, parent = document.body, canOpen = () => true, controlsFree = canOpen, player = () => ({ name: 'PLAYER 1' }) }) {
  const log = new ChatLog();
  let channel = null;
  let rendered = [];
  const root = document.createElement('section');
  // bfe-chat: the front end's look restyles it (src/menus/themes/page).
  root.className = 'bfe-chat';
  root.setAttribute('aria-label', 'Chat');
  root.style.cssText = 'position:fixed;left:3%;top:20%;width:min(650px,85vw);z-index:4;font:clamp(14px,1.5vw,22px) monospace;text-shadow:1px 1px 2px #000;pointer-events:none';
  const feed = document.createElement('div');
  feed.setAttribute('role', 'log'); feed.setAttribute('aria-live', 'polite');
  const form = document.createElement('div');
  form.className = 'bfe-chat-entry';
  form.style.cssText = 'display:none;background:#001020cc;padding:6px;pointer-events:auto';
  const label = document.createElement('label');
  const prompt = document.createTextNode('');
  const input = document.createElement('input');
  input.maxLength = CHAT_LIMIT; input.autocomplete = 'off'; input.spellcheck = false;
  input.setAttribute('aria-label', 'Chat message');
  input.style.cssText = 'font:inherit;color:inherit;background:transparent;border:0;outline:0;width:75%';
  label.append(prompt, input); form.append(label); root.append(feed, form); parent.append(root);
  const render = () => {
    const shown = log.messages.filter(message => channel || Date.now() - message.time <= 12000);
    if (shown.length === rendered.length && shown.every((message, i) => rendered[i] === message)) return;
    rendered = shown;
    feed.replaceChildren();
    for (const message of shown) {
      const line = document.createElement('div');
      line.style.cssText = `color:${CHAT_COLORS[message.channel]};overflow-wrap:anywhere`;
      line.textContent = `${message.channel === 'server' ? '[SERVER] ' : message.channel === 'team' ? '[TEAM] ' : ''}${message.name && message.channel !== 'server' ? message.name + ': ' : ''}${message.text}`;
      feed.append(line);
    }
  };
  const close = () => {
    channel = null; input.value = ''; input.blur(); form.style.display = 'none';
    controls.clear(); controls.enabled = controlsFree(); render();
  };
  const chat = {
    log,
    get isOpen() { return channel !== null; },
    // Local echo for offline play. A future host transport replaces send
    // and calls receive only after accepting and routing the message.
    send(message) { chat.receive(message); },
    receive(message) { const added = log.receive(message); if (added) render(); return added; },
    server(text) { return chat.receive({ channel: 'server', text }); },
    close,
    dispose() { clearInterval(timer); window.removeEventListener('keydown', key, true); window.removeEventListener('keyup', release, true); root.remove(); },
  };
  const key = (event) => {
    if (!channel) {
      if (event.repeat || event.ctrlKey || event.metaKey || event.altKey || !canOpen()) return;
      if (event.code !== 'KeyT' && event.code !== 'KeyY') return;
      channel = event.code === 'KeyY' ? 'team' : 'global';
      controls.clear(); controls.enabled = false;
      prompt.data = channel === 'team' ? 'TEAM: ' : 'SAY: ';
      form.style.display = 'block'; form.style.color = CHAT_COLORS[channel];
      input.focus(); render(); event.preventDefault();
    } else if (event.code === 'Enter' || event.code === 'NumpadEnter') {
      event.preventDefault();
      const text = input.value.trim();
      if (text) chat.send({ ...player(), channel, text });
      close();
    } else if (event.code === 'Escape') { event.preventDefault(); close(); }
    // Tab would move the focus out of the entry (and must not open the
    // scoreboard under it).
    else if (event.code === 'Tab') event.preventDefault();
    // Native text editing still runs; game/menu listeners never see it.
    event.stopImmediatePropagation();
  };
  const release = (event) => { if (channel) event.stopImmediatePropagation(); };
  window.addEventListener('keydown', key, true);
  window.addEventListener('keyup', release, true);
  const timer = setInterval(() => { if (channel && !canOpen()) close(); else render(); }, 1000);
  return chat;
}
