// Verify the actual archives, then execute the current architecture's network
// modules inside packaged Electron: a real native UDP session over loopback
// between the client transport in app.asar and the bundled host server. No
// game data, native module or WebRTC stack is involved.
import { readdirSync, existsSync } from 'node:fs';
import { resolve, join, dirname, basename } from 'node:path';
import { spawnSync } from 'node:child_process';
import { listPackage } from '@electron/asar';
const root = resolve(process.argv[2] || 'release');
function archives(directory, depth = 0) {
  if (depth > 7) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = join(directory, entry.name);
    return entry.isDirectory() ? archives(file, depth + 1) : entry.name === 'app.asar' ? [file] : [];
  });
}
const files = archives(root);
if (!files.length) throw Error('No packaged app.asar was found');
for (const archive of files) {
  const paths = new Set(listPackage(archive).map(path => path.replaceAll('\\', '/')));
  for (const file of ['/src/runtime/network-contract.js', '/src/battle-limits.js', '/electron/network.js', '/electron/preload.cjs', '/electron/net-host.js', '/electron/net-process.js', '/electron/net-service.js', '/src/net/endpoint.js', '/src/net/connection.js', '/src/net/handshake.js', '/src/net/crypto.js', '/src/net/codec.js', '/src/net/stun.js', '/src/net/turn.js', '/node_modules/ws/index.js', '/node_modules/ws/wrapper.mjs', '/node_modules/ws/package.json', '/dist-desktop/index.html', '/electron/boot.js', '/electron/update/index.js', '/electron/update/install.js', '/electron/update/feed.js', '/electron/update/keys.js', '/electron/update/fs.js', '/src/launch.js', '/electron/presence/index.js', '/electron/presence/activity.js', '/electron/presence/discord.js', '/electron/presence/application.js', '/electron/steam/index.js', '/electron/steam/client.js', '/electron/steam/launcher.js', '/electron/steam/relaunch.js', '/electron/steam/shortcut.js', '/electron/steam/vdf.js', '/electron/steam/overlay.js', '/electron/assets/steam/capsule.jpg', '/electron/assets/steam/portrait.jpg', '/electron/assets/steam/hero.jpg', '/electron/assets/steam/logo.png', '/electron/assets/steam/icon.png']) if (!paths.has(file)) throw Error(`Missing ${file} in ${archive}`);
  if ([...paths].some(path => /^\/(?:public|data|extract)\//.test(path) || /^\/dist-desktop\/(?:maps|sides|common|audio)\//.test(path))) throw Error(`Retail data entered ${archive}`);
  console.log('Packaged resources verified:', archive);
  const resources = dirname(archive), app = dirname(resources);
  for (const file of ['server/src/main.js', 'server/src/udp-server.js', 'server/node_modules/ws/package.json', 'engine/src/game.js', 'engine/src/net/endpoint.js', 'engine/node_modules/three/package.json']) if (!existsSync(join(resources, 'host-runtime', file))) throw Error(`Missing bundled host resource ${file}`);
  for (const stack of ['node-datachannel', 'werift']) if (existsSync(join(resources, 'host-runtime/server/node_modules', stack))) throw Error(`Host package must not contain the retired WebRTC stack (${stack})`);
  if (!existsSync(join(resources, 'tools/content-version.json'))) throw Error('Missing preparation version resource');
  // The updater reuses an installed build's files by this index (build/tree-index.cjs).
  if (!existsSync(join(resources, 'bfe-tree.json'))) throw Error('Missing the build file index (resources/bfe-tree.json)');
  // Cross-built Windows archives receive all structural checks above.
  // Executing their Windows runtime is a separate Windows smoke test.
  if (process.platform === 'linux' && basename(app).startsWith('win')) continue;
  let executable;
  if (process.platform === 'darwin') {
    const arm = archive.includes('mac-arm64'); if (arm !== (process.arch === 'arm64')) continue;
    executable = join(app, 'MacOS', 'Battlefront Eternal');
  } else if (process.platform === 'linux') {
    const arm = basename(app).includes('arm64'); if (arm !== (process.arch === 'arm64')) continue;
    executable = join(app, 'battlefront-eternal');
  } else executable = join(app, 'Battlefront Eternal.exe');
  if (!existsSync(executable)) throw Error(`Missing executable ${executable}`);
  const code = `const {pathToFileURL}=require('node:url');const fs=require('node:fs');const os=require('node:os');const path=require('node:path');(async()=>{await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/content-archive.js'))}));const {discordActivity,sceneOf}=await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/presence/activity.js'))}));if(discordActivity({...sceneOf('app://bfe/?map=nab2&era=cw'),started:1,since:1}).details!=='Instant Action: NAB2')throw Error('Packaged presence mismatch');await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/presence/index.js'))}));await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/steam/index.js'))}));await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/steam/relaunch.js'))}));await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/steam/overlay.js'))}));const {CONTENT_VERSION}=await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/prepare.js'))}));if(CONTENT_VERSION.maps!==JSON.parse(fs.readFileSync(${JSON.stringify(join(resources, 'tools/content-version.json'))})).maps)throw Error('Packaged preparation versions differ');const {createNetworkBridge}=await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/network.js'))}));const dir=fs.mkdtempSync(path.join(os.tmpdir(),'bfe-package-'));try{const bridge=createNetworkBridge(dir,()=>{});const identity=await bridge('identity');if(!/^[a-f0-9]{64}$/.test(identity.id)||identity.privateKey)throw Error('Invalid packaged identity');const {startUdpServer}=await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/server/src/udp-server.js'))}));const {loadIdentity,keyId}=await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/server/src/identity.js'))}));const {NETWORK_CONTRACT}=await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/engine/src/runtime/network-contract.js'))}));const {createNetService}=await import(pathToFileURL(${JSON.stringify(join(archive, 'electron/net-service.js'))}));const serverKeys=loadIdentity(path.join(dir,'server'));const udp=await startUdpServer({engine:${JSON.stringify(join(resources, 'host-runtime/engine'))},keys:serverKeys,port:0,host:'127.0.0.1'});const world=new (require('node:events').EventEmitter)();world.networkContract=NETWORK_CONTRACT;world.postMessage=()=>{};udp.setBattle({world,content:'package'});const service=createNetService({identity:JSON.parse(fs.readFileSync(path.join(dir,'network-identity.json'),'utf8')),listen:{port:0,families:['IPv4']}});const connected=await new Promise((resolve,reject)=>{let handler;service.serve({post:m=>{if(m.op==='connected')resolve(m);else if(m.op==='error')reject(Error(m.message));},onMessage:h=>{handler=h;},onClose(){},close(){}});handler({op:'connect',serverId:keyId(serverKeys.publicKey),candidates:[{address:'127.0.0.1',port:udp.port}],hello:{protocol:NETWORK_CONTRACT.protocol,profile:NETWORK_CONTRACT.profile,simulation:NETWORK_CONTRACT.simulation,content:'package'},timeoutMs:5000});});if(connected.server.hello.protocol!==NETWORK_CONTRACT.protocol)throw Error('Packaged native UDP handshake mismatch');service.close();await udp.close();await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/engine/src/game.js'))}));const {DEFAULTS}=await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/engine/src/settings.js'))}));const {InputMailbox}=await import(pathToFileURL(${JSON.stringify(join(resources, 'host-runtime/engine/src/runtime/transport.js'))}));const mailbox=new InputMailbox();mailbox.push({epoch:1,sequence:1,tick:1,generation:1,down:[],actions:[],look:[],flightSettings:{sensitivity:DEFAULTS.flightSensitivity,invert:DEFAULTS.flightInvertY,curve:DEFAULTS.flightResponseCurve,bindings:DEFAULTS.flightBindings},gamepad:null});if(mailbox.take(1,1).flightSettings.curve!==DEFAULTS.flightResponseCurve)throw Error('Packaged desktop input contract mismatch');require('node:sqlite');console.log('Packaged networking and hosting passed',process.platform,process.arch);}finally{fs.rmSync(dir,{recursive:true,force:true});}})().catch(error=>{console.error(error);process.exitCode=1;});`;
  const run = spawnSync(executable, ['-e', code], { env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' }, encoding: 'utf8', timeout: 30000 });
  process.stdout.write(run.stdout || ''); process.stderr.write(run.stderr || '');
  if (run.error || run.status !== 0) throw run.error || Error(`Packaged Electron networking failed (${run.status})`);
}
