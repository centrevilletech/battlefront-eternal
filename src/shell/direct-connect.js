// Native form controls keep direct address entry accessible in the canvas menu.
export function directConnect(bridge) {
  return new Promise(resolve => {
    const dialog = document.createElement('dialog');
    dialog.style.cssText = 'background:#101a28;color:white;border:1px solid #819abc;padding:24px;font:18px sans-serif;max-width:90vw';
    const form = document.createElement('form');
    const heading = document.createElement('h2'); heading.textContent = 'Direct connection';
    const addressLabel = document.createElement('label'); addressLabel.textContent = 'Server IPv4 or IPv6 address ';
    const address = document.createElement('input'); address.required = true; address.placeholder = '192.168.1.10'; address.autocomplete = 'off'; addressLabel.append(address);
    const portLabel = document.createElement('label'); portLabel.textContent = ' Port ';
    const port = document.createElement('input'); port.type = 'number'; port.min = '1'; port.max = '65535'; port.value = '18451'; port.required = true; port.style.width = '90px'; portLabel.append(port);
    const error = document.createElement('p'); error.setAttribute('role', 'status');
    const join = document.createElement('button'); join.type = 'submit'; join.textContent = 'Connect';
    const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = 'Cancel';
    const finish = value => { dialog.close(); dialog.remove(); resolve(value); };
    cancel.onclick = () => finish(null);
    dialog.addEventListener('cancel', event => { event.preventDefault(); finish(null); });
    dialog.addEventListener('keydown', event => event.stopPropagation());
    form.onsubmit = async event => {
      event.preventDefault(); join.disabled = true; error.textContent = 'Contacting server...';
      try { const target = { address: address.value.trim().replace(/^\[|\]$/g, ''), port: Number(port.value) }; const info = await bridge.request('directInfo', target); finish({ ...info, ...target }); }
      catch (failure) { error.textContent = failure.message; join.disabled = false; }
    };
    form.append(heading, addressLabel, portLabel, error, join, cancel); dialog.append(form); document.body.append(dialog); dialog.showModal(); address.focus();
  });
}
