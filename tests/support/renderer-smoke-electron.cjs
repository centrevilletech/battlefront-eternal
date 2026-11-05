const fs = require('node:fs');
// The Windows GUI executable does not reliably forward its console to CI.
const report = (value) => fs.appendFileSync(process.env.BFE_SMOKE_REPORT, JSON.stringify(value) + '\n');
report({ stage: 'runtime', electron: process.versions.electron, chrome: process.versions.chrome });
const { app, BrowserWindow } = require('electron');
if (!process.versions.electron || !app) throw new Error('Renderer smoke requires the Electron app runtime');
console.log(`Electron app runtime ${process.versions.electron}, Chromium ${process.versions.chrome}`);
// A URL followed by another argument is rejected by Electron on Windows.
const url = process.env.BFE_SMOKE_URL, backend = process.env.BFE_SMOKE_BACKEND;
app.commandLine.appendSwitch('use-gl', 'angle');
app.commandLine.appendSwitch('use-angle', backend === 'warp' ? 'd3d11-warp' : backend);
app.commandLine.appendSwitch('ignore-gpu-blocklist');
if (backend === 'swiftshader') app.commandLine.appendSwitch('enable-unsafe-swiftshader');
app.whenReady().then(async () => {
  const window = new BrowserWindow({ width: 320, height: 240, useContentSize: true, show: false, webPreferences: { backgroundThrottling: false, contextIsolation: true, nodeIntegration: false } });
  report({ stage: 'window' });
  const errors = [];
  window.webContents.on('console-message', (event) => {
    if (event.message.startsWith('renderer smoke:')) console.log(event.message);
    if (event.level === 'error') errors.push(event.message);
  });
  window.webContents.on('render-process-gone', (_event, details) => { report({ crash: details }); app.exit(1); });
  try {
    // Vite's test entry imports only code and generated fixtures.
    const result = [];
    report({ stage: 'fixtures', backend });
    for (const query of ['', '?noMulti']) {
      await window.loadURL(`${url}tests/support/renderer-smoke.html${query}`);
      result.push(await window.webContents.executeJavaScript(`import('/tests/renderer-smoke-checks.js').then(m => m.checkRenderer(${JSON.stringify(backend)}))`));
    }
    if (errors.length) throw new Error(errors.join('\n'));
    report({ versions: process.versions, runs: result });
    app.exit(result.every(run => run.ok) ? 0 : 1);
  } catch (error) { report({ error: String(error), stack: error.stack }); app.exit(1); }
}).catch(error => { report({ error: String(error), stack: error.stack }); app.exit(1); });
