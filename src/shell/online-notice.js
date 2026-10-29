// A real modal keeps long notice text out of the fixed-size canvas popups.
// Share one pending choice between repeated menu clicks and hosting.
const pending = new WeakMap();
export function onlineNotice(bridge, notice) {
  if (notice.accepted) return Promise.resolve(true);
  if (pending.has(bridge)) return pending.get(bridge);
  const choice = new Promise(resolve => {
    const previousFocus = document.activeElement;
    const dialog = document.createElement('dialog');
    dialog.className = 'bfe-online-notice'; dialog.tabIndex = -1;
    dialog.setAttribute('aria-labelledby', 'bfe-online-notice-title');
    const style = document.createElement('style');
    style.textContent = `
      .bfe-online-notice { box-sizing:border-box; width:min(600px,calc(100vw - 32px)); max-height:calc(100dvh - 32px); overflow:auto; margin:auto; padding:24px; color:#f3f1dc; background:#101e32; border:1px solid #80c9ed; border-radius:8px; font:17px/1.5 system-ui,sans-serif; text-align:left; }
      .bfe-online-notice::backdrop { background:rgba(3,10,22,.8); }
      .bfe-online-notice h2 { font:600 28px/1.2 system-ui,sans-serif; margin:0 0 20px; }
      .bfe-online-notice p { margin:0 0 16px; overflow-wrap:anywhere; }
      .bfe-online-notice .notice-label { color:#94cce9; font-size:13px; letter-spacing:.1em; text-transform:uppercase; margin-bottom:8px; }
      .bfe-online-notice .notice-error { color:#ffb7a8; }
      .bfe-online-notice footer { display:flex; flex-wrap:wrap; gap:12px; padding-top:8px; }
      .bfe-online-notice button { flex:1 1 140px; min-height:48px; padding:10px 16px; border:1px solid #8eb4ce; border-radius:4px; background:#20334c; color:#fff; font:600 16px/1.3 system-ui,sans-serif; cursor:pointer; }
      .bfe-online-notice button[data-accept] { background:#f1e5a6; color:#142136; border-color:#f1e5a6; }
      .bfe-online-notice button:focus-visible { outline:3px solid #7bdbff; outline-offset:3px; }
      .bfe-online-notice button:disabled { opacity:.6; cursor:wait; }
      @media(max-width:400px) { .bfe-online-notice { padding:18px; font-size:16px; } }
    `;
    const paragraph = (text, className) => {
      const p = document.createElement('p'); p.textContent = text; if (className) p.className = className; return p;
    };
    const title = document.createElement('h2'); title.id = 'bfe-online-notice-title'; title.textContent = 'Before you play online';
    const status = paragraph(''); status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); status.hidden = true;
    const error = paragraph('', 'notice-error'); error.setAttribute('role', 'alert'); error.hidden = true;
    const footer = document.createElement('footer');
    const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = 'Not now'; cancel.autofocus = true;
    const accept = document.createElement('button'); accept.type = 'button'; accept.textContent = 'Accept and continue'; accept.dataset.accept = '';
    let saving = false, finished = false;
    const finish = value => {
      if (finished) return; finished = true;
      dialog.close(); dialog.remove(); style.remove(); previousFocus?.focus(); resolve(value);
    };
    cancel.onclick = () => { if (!saving) finish(false); };
    accept.onclick = async () => {
      if (saving || finished) return;
      saving = true; cancel.disabled = accept.disabled = true; error.hidden = true; dialog.focus();
      status.textContent = 'Saving your choice…'; status.hidden = false;
      try {
        if (await bridge.request('accept', notice.version) !== true) throw Error('The notice has changed. Close this dialog and open it again.');
        finish(true);
      } catch (cause) {
        error.textContent = `Your choice could not be saved. ${cause.message || 'Please try again.'}`; error.hidden = false;
      } finally {
        saving = false; cancel.disabled = accept.disabled = false; status.hidden = true;
        if (!finished) accept.focus();
      }
    };
    dialog.addEventListener('cancel', event => { event.preventDefault(); if (!saving) finish(false); });
    // Menu input listens on window. Keep keyboard/wheel input inside the modal.
    dialog.addEventListener('keydown', event => event.stopPropagation());
    dialog.addEventListener('wheel', event => event.stopPropagation());
    footer.append(cancel, accept);
    dialog.append(paragraph('Online privacy', 'notice-label'), title,
      paragraph(`Online services are operated by ${notice.operator || 'Josh Lambert'}.`),
      paragraph('The service sees your IP address and public player identity. Reports are retained for 30 days. TURN relay traffic is forwarded.'),
      paragraph(`Accept to contact ${notice.master || 'https://online.battlefronteternal.com'}. Your choice is saved on this computer.`),
      status, error, footer);
    document.head.append(style); document.body.append(dialog); dialog.showModal(); cancel.focus();
  });
  pending.set(bridge, choice);
  void choice.finally(() => pending.delete(bridge));
  return choice;
}
