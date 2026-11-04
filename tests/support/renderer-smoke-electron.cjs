const { app, BrowserWindow } = require('electron');
if (!process.versions.electron || !app) throw new Error('Renderer smoke requires the Electron app runtime');
console.log(`Electron app runtime ${process.versions.electron}, Chromium ${process.versions.chrome}`);
const [url, backend] = process.argv.slice(-2);
app.commandLine.appendSwitch('use-gl', 'angle');
app.commandLine.appendSwitch('use-angle', backend === 'warp' ? 'd3d11-warp' : backend);
app.commandLine.appendSwitch('ignore-gpu-blocklist');
if (backend === 'swiftshader') app.commandLine.appendSwitch('enable-unsafe-swiftshader');
app.whenReady().then(async () => {
  const window = new BrowserWindow({ width: 320, height: 240, useContentSize: true, show: false, webPreferences: { backgroundThrottling: false, contextIsolation: true, nodeIntegration: false } });
  const errors = [];
  window.webContents.on('console-message', (event) => {
    if (event.level === 'error') errors.push(event.message);
  });
  window.webContents.on('render-process-gone', (_event, details) => { console.error(details); app.exit(1); });
  try {
    // Vite's test entry imports only code and generated fixtures.
    const result = [];
    for (const query of ['', '?noMulti']) {
      await window.loadURL(`${url}tests/support/renderer-smoke.html${query}`);
      result.push(await window.webContents.executeJavaScript(`import('/tests/renderer-smoke-checks.js').then(m => m.checkRenderer(${JSON.stringify(backend)}))`));
    }
    if (errors.length) throw new Error(errors.join('\n'));
    console.log(JSON.stringify({ versions: process.versions, runs: result }, null, 2));
    app.exit(result.every(run => run.ok) ? 0 : 1);
  } catch (error) { console.error(error); app.exit(1); }
}).catch(error => { console.error(error); app.exit(1); });
