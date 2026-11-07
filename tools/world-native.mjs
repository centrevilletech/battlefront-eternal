// Native arithmetic runs in the Rust driver. Node only decodes the versioned
// corpus and canonicalizes JSON/hash bytes exactly as world-hash.mjs does.
import path from 'node:path';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { WorldCorpus } from './world-corpus.mjs';
const args=process.argv.slice(2),option=(key,fallback)=>{const at=args.indexOf(`--${key}`);return at<0?fallback:args[at+1];};
const directory=option('corpus',null),driver=option('driver',null);
if(!directory||!driver)throw Error('Usage: world-native --corpus DIR --driver BINARY [--scenario NAME]');
const corpus=new WorldCorpus({directory,replay:true});
const driverArgs=args.flatMap((arg,index)=>arg==='--driver-arg'?[args[index+1]]:[]);
const child=spawn(path.resolve(driver),[...driverArgs,'--scenario',option('scenario','replay')],{stdio:['pipe','pipe','inherit']});
const completion=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',code=>code===0?resolve():reject(Error(`Native world replay exited ${code}`)));});
const hash=data=>crypto.createHash('sha1').update(data).digest('hex').slice(0,16);
const output=(async()=>{
  for await(const line of readline.createInterface({input:child.stdout})) {
    const observation=JSON.parse(line),published=crypto.createHash('sha1');
    // These are native results, never values read from the input corpus.
    for(const packet of observation.packets){published.update(packet.channel);published.update(Object.hasOwn(packet,'bytes')?Buffer.from(packet.bytes,'base64'):packet.text);}
    process.stdout.write(`${observation.prefix||''}${observation.tick} ${hash(JSON.stringify(JSON.parse(observation.stateJson)))} ${hash(JSON.stringify(JSON.parse(observation.digestJson)))} ${published.digest('hex').slice(0,16)}\n`);
  }
})();
async function send(value){if(!child.stdin.write(JSON.stringify(value)+'\n'))await once(child.stdin,'drain');}
const assets=[];
for(const url of Object.keys(corpus.assets)) {
  const bytes=corpus.readAsset(url,()=>{throw Error('Installed content unavailable during replay');});
  assets.push({url,hex:bytes===null?null:bytes.toString('hex')});
}
await send({kind:'admit',setup:corpus.setup,assets});
for(const frame of corpus.frames) {
  const rows=frame.world.map(encoded=>{
    const bytes=Buffer.from(encoded,'base64'),bits=[];
    for(let at=0;at<bytes.length;at+=8)bits.push(bytes.readBigUInt64LE(at).toString(16).padStart(16,'0'));
    return bits;
  });
  await send({kind:'frame',...frame,world:rows});
}
child.stdin.end();await Promise.all([completion,output]);
