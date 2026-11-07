// Stream full diagnostics to a farm file and retain only a bounded tail.
// Children own their cleanup; the watchdog signals only the child's PID.
import {spawn} from 'node:child_process';
import {createWriteStream} from 'node:fs';
import {mkdir} from 'node:fs/promises';
import {dirname} from 'node:path';
export async function run(command,args,{log,env={},cwd,timeout=7200000}={}){
  if(log)await mkdir(dirname(log),{recursive:true});
  const file=log?createWriteStream(log):null;
  const child=spawn(command,args,{cwd,env:{...process.env,...env},stdio:['ignore','pipe','pipe']});
  let tail='',timedOut=false;
  const append=bytes=>{file?.write(bytes);tail=(tail+bytes).slice(-6000);};
  child.stdout.on('data',append);child.stderr.on('data',append);
  const timer=setTimeout(()=>{timedOut=true;child.kill('SIGTERM');},timeout);
  const kill=setTimeout(()=>child.kill('SIGKILL'),timeout+30000);
  try{
    const exit=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',(code,signal)=>resolve({code,signal}));});
    return {...exit,timedOut,tail};
  }finally{clearTimeout(timer);clearTimeout(kill);if(file)await new Promise(resolve=>file.end(resolve));}
}
export function requireRun(result){if(result.code!==0||result.timedOut)throw Error(`Child failed (${result.code??result.signal}): ${result.tail}`);return result;}
