import { onlineNotice } from './online-notice.js';
import { MAX_TEAM_SIZE, MAX_MATCH_PLAYERS } from '../battle-limits.js';

export function hostDialog(host, network, maps = []) {
  return new Promise(resolve => {
    const dialog = document.createElement('dialog');
    dialog.style.cssText = 'background:#101a28;color:white;border:1px solid #819abc;padding:24px;font:17px sans-serif;max-width:90vw';
    const form = document.createElement('form'), heading = document.createElement('h2'); heading.textContent = 'Host a battle';
    const fields = {};
    const field = (name, label, options) => {
      const row = document.createElement('label'); row.style.cssText = 'display:block;margin:12px 0'; row.textContent = label + ' ';
      const input = document.createElement(options ? 'select' : 'input');
      if (options) for (const [value, text] of options) { const option = document.createElement('option'); option.value = value; option.textContent = text; input.append(option); }
      fields[name] = input; row.append(input); form.append(row); return input;
    };
    form.append(heading);
    field('name', 'Server name').value = 'My Battlefront server'; fields.name.maxLength = 80; fields.name.required = true;
    field('mode', 'Visibility', [['lan', 'LAN only'], ['public', 'Public online']]);
    field('map', 'Map', maps.length ? maps.map(map => [map.id, map.name || map.id]) : [['nab2', 'Naboo: Theed']]);
    if (maps.some(map => map.id === 'nab2')) fields.map.value = 'nab2';
    field('side', 'Side', [['rep', 'Republic'], ['cis', 'CIS'], ['all', 'Alliance'], ['imp', 'Empire']]);
    field('maxPlayers', 'Human players (total)').value = '8'; fields.maxPlayers.type = 'number'; fields.maxPlayers.min = '2'; fields.maxPlayers.max = String(MAX_MATCH_PLAYERS); fields.maxPlayers.required = true;
    const population = document.createElement('p');
    population.textContent = `Up to ${MAX_TEAM_SIZE} per team, including bots. Human players replace bots.`;
    form.append(population);
    field('relay', 'Connection', [['auto', 'Automatic'], ['always', 'Relay only'], ['never', 'Direct only']]);
    const status = document.createElement('p'); status.setAttribute('role', 'status');
    const start = document.createElement('button'); start.type = 'submit'; start.textContent = 'Host and join';
    const join = document.createElement('button'); join.type = 'button'; join.textContent = 'Join running server'; join.hidden = true;
    const stop = document.createElement('button'); stop.type = 'button'; stop.textContent = 'Stop server'; stop.hidden = true;
    const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = 'Close';
    const finish = value => { dialog.close(); dialog.remove(); resolve(value); };
    const update = async () => { try { const result = await host.request('status'); join.hidden = stop.hidden = !result.active; start.disabled = result.active; status.textContent = result.active ? `Your server is ${result.status}. ${result.players?.length || 0} players connected.` : 'Hosting uses this computer and your imported game. It keeps running when minimized.'; } catch (error) { status.textContent = error.message; } };
    form.onsubmit = async event => {
      event.preventDefault(); start.disabled = true;
      try {
        const count = Number(fields.maxPlayers.value);
        if (!Number.isInteger(count) || count < 2 || count > MAX_MATCH_PLAYERS) throw Error(`Choose 2 to ${MAX_MATCH_PLAYERS} human players total.`);
        if (fields.mode.value === 'public') {
          const notice = await network.request('notice');
          if (!await onlineNotice(network, notice)) { start.disabled = false; return; }
        }
        status.textContent = 'Starting your server...';
        const options = Object.fromEntries(Object.entries(fields).map(([key, field]) => [key, field.value])); options.maxPlayers = Number(options.maxPlayers);
        finish(await host.request('start', options));
      } catch (error) { status.textContent = error.message; start.disabled = false; }
    };
    join.onclick = async () => { try { finish(await host.request('join')); } catch (error) { status.textContent = error.message; } };
    stop.onclick = async () => { try { await host.request('stop'); status.textContent = 'Server is stopping...'; setTimeout(update, 1000); } catch (error) { status.textContent = error.message; } };
    cancel.onclick = () => finish(null);
    dialog.addEventListener('cancel', event => { event.preventDefault(); finish(null); });
    dialog.addEventListener('keydown', event => event.stopPropagation());
    form.append(status, start, join, stop, cancel); dialog.append(form); document.body.append(dialog); dialog.showModal(); update();
  });
}
