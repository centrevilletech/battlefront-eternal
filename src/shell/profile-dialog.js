// Native form controls keep the profile name entry accessible in the canvas
// menu, as the direct connection dialog does. Resolves the name, or null.
export const PLAYER_NAME_LIMIT = 32;
export const DEFAULT_PLAYER_NAME = 'PLAYER 1';

// A name as the server and the other players will see it: printable, trimmed,
// at most PLAYER_NAME_LIMIT characters; empty falls back to the default.
export function cleanPlayerName(value, fallback = DEFAULT_PLAYER_NAME) {
  const text = String(value ?? '').replace(/[\x00-\x1f\x7f]/g, '').replace(/\s+/g, ' ').trim().slice(0, PLAYER_NAME_LIMIT);
  return text || fallback;
}

// message: a line above the name (why the dialog asks, when a join does).
export function profileDialog(current = DEFAULT_PLAYER_NAME, { message = '' } = {}) {
  return new Promise(resolve => {
    const dialog = document.createElement('dialog');
    dialog.style.cssText = 'background:#101a28;color:white;border:1px solid #819abc;padding:24px;font:18px sans-serif;max-width:90vw';
    const form = document.createElement('form');
    const heading = document.createElement('h2'); heading.textContent = 'Profile';
    const note = document.createElement('p'); note.textContent = message; note.hidden = !message;
    const label = document.createElement('label'); label.textContent = 'Player name ';
    const input = document.createElement('input'); input.required = true; input.maxLength = PLAYER_NAME_LIMIT; input.value = current; input.autocomplete = 'off'; input.spellcheck = false; label.append(input);
    const ok = document.createElement('button'); ok.type = 'submit'; ok.textContent = 'Save';
    const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = 'Cancel';
    const finish = value => { dialog.close(); dialog.remove(); resolve(value); };
    cancel.onclick = () => finish(null);
    dialog.addEventListener('cancel', event => { event.preventDefault(); finish(null); });
    dialog.addEventListener('keydown', event => event.stopPropagation());
    form.onsubmit = event => { event.preventDefault(); finish(cleanPlayerName(input.value)); };
    form.append(heading, note, label, ok, cancel); dialog.append(form); document.body.append(dialog); dialog.showModal(); input.focus(); input.select();
  });
}
