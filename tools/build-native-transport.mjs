// Build the in-process transport/receiver interoperability host. The device
// executable links Rust directly and never loads this Node observer surface.
import {mkdirSync,copyFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
const result=spawnSync('cargo',['build','--locked','--release','-p','bfe-transport','--features','node'],{cwd:root+'rust',stdio:'inherit'});
if(result.error||result.status)throw result.error??Error('native transport build failed');
const name=process.platform==='win32'?'bfe_transport.dll':process.platform==='darwin'?'libbfe_transport.dylib':'libbfe_transport.so';
mkdirSync(root+'.transport-addon',{recursive:true});copyFileSync(root+'rust/target/release/'+name,root+'.transport-addon/bfe-transport.node');
