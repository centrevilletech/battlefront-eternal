// Probe-only Linux traffic shaping. Every qdisc lives in a disposable network
// namespace; never alter the developer's real interfaces or routing table.
import {spawn,execFileSync} from 'node:child_process';
import {readFile,readlink,mkdtemp,rm} from 'node:fs/promises';
import {createSocket} from 'node:dgram';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

const run=(command,args)=>execFileSync(command,args,{encoding:'utf8',stdio:['ignore','pipe','pipe']});
const privileged=(command,args)=>process.getuid()===0?run(command,args):run('sudo',['-n',command,...args]);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
// Some iproute2 releases print text even with -j for HTB classes.
const jsonOrText=text=>{try{return JSON.parse(text);}catch{return text;}};
export async function enterProbeNamespace(args){
  if(!args.some(value=>value.startsWith('--shape-up-kbps=')&&Number(value.split('=')[1])>0))return;
  if(process.env.BFE_NET_PROBE_ISOLATED==='1'){
    if(await readlink('/proc/self/ns/net')===await readlink('/proc/1/ns/net'))throw Error('Refusing to shape the host network namespace');
    return;
  }
  const name=`bfe-net-probe-${process.pid}`;
  privileged('ip',['netns','add',name]);
  try{
    for(const command of [['link','set','lo','up'],['link','add','bench0','type','dummy'],['addr','add','10.244.99.1/24','dev','bench0'],['link','set','bench0','up']])privileged('ip',['-n',name,...command]);
    const child=spawn('sudo',['-n','ip','netns','exec',name,'env','BFE_NET_PROBE_ISOLATED=1',process.execPath,process.argv[1],...args],{stdio:'inherit'});
    const interrupt=()=>child.kill('SIGTERM');process.once('SIGINT',interrupt);process.once('SIGTERM',interrupt);
    const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);});
    process.removeListener('SIGINT',interrupt);process.removeListener('SIGTERM',interrupt);
    process.exitCode=code??1;
  }finally{
    const remaining=privileged('ip',['netns','pids',name]).trim().split(/\s+/).filter(value=>/^\d+$/.test(value));
    if(remaining.length)try{privileged('kill',['-TERM',...remaining]);}catch{}
    privileged('ip',['netns','delete',name]);
  }
  process.exit(process.exitCode??0);
}
export async function shapeProbeTraffic(pairs,{upKbps,downKbps,delayMs,jitterMs,lossPercent,competingUpKbps,queuePackets}){
  if(!upKbps)return null;
  if(process.env.BFE_NET_PROBE_ISOLATED!=='1'||await readlink('/proc/self/ns/net')===await readlink('/proc/1/ns/net'))throw Error('Shaping requires the isolated probe namespace');
  for(const value of [upKbps,downKbps,delayMs,jitterMs,lossPercent,competingUpKbps,queuePackets])if(!Number.isFinite(value)||value<0)throw Error('Invalid shaping value');
  if(upKbps<50||downKbps<100||lossPercent>20||queuePackets<4||queuePackets>1000)throw Error('Shaping value outside probe bounds');
  const tc=args=>run('tc',args),ports=[],sockets=[],timers=[];
  tc(['qdisc','replace','dev','lo','root','handle','1:','htb','default','1']);
  tc(['class','add','dev','lo','parent','1:','classid','1:1','htb','rate','1gbit','quantum','1500']);
  // Upstream is what leaves the client's port; downstream is what arrives at
  // it. Matching downstream by destination keeps per-client queues separate
  // when one server port serves every client (the native UDP game port).
  const filter=(port,id,field='sport')=>tc(['filter','add','dev','lo','parent','1:','protocol','ip','prio','1','u32','match','ip','protocol','17','0xff','match','ip',field,String(port),'0xffff','flowid',`1:${id}`]);
  for(const [index,pair] of pairs.entries()){
    if(!pair||pair.local.address.includes(':'))throw Error('Shaped probe requires a selected IPv4 UDP pair');
    const upId=10+index*2,downId=upId+1;
    for(const [id,rate,port,field] of [[upId,upKbps,pair.local.port,'sport'],[downId,downKbps,pair.local.port,'dport']]){
      tc(['class','add','dev','lo','parent','1:','classid',`1:${id}`,'htb','rate',`${rate}kbit`,'ceil',`${rate}kbit`,'burst','1500','cburst','1500','quantum','1500']);
      tc(['qdisc','add','dev','lo','parent',`1:${id}`,'handle',`${id}:`,'netem','limit',String(queuePackets),'delay',`${delayMs}ms`,`${jitterMs}ms`,'loss',`${lossPercent}%`]);
      filter(port,id,field);
    }
    if(competingUpKbps){
      const sender=createSocket('udp4'),sink=createSocket('udp4');sockets.push(sender,sink);sink.on('message',()=>{});
      await new Promise(resolve=>sink.bind(0,pair.local.address,resolve));await new Promise(resolve=>sender.bind(0,pair.local.address,resolve));
      ports.push(sender.address().port);filter(sender.address().port,upId);
      const payload=Buffer.alloc(400),interval=(payload.length+28)*8/competingUpKbps;
      timers.push(setInterval(()=>sender.send(payload,sink.address().port,pair.local.address),interval));
    }
  }
  return {ports,stats:()=>({upKbps,downKbps,delayMs,jitterMs,lossPercent,competingUpKbps,queuePackets,accounting:'HTB charges the loopback packet including its link header; captured IP rates are slightly below the configured link rate.',classes:jsonOrText(tc(['-j','-s','class','show','dev','lo'])),qdiscs:jsonOrText(tc(['-j','-s','qdisc','show','dev','lo']))}),stop(){for(const timer of timers)clearInterval(timer);for(const socket of sockets)socket.close();}};
}

// Transport counters are application payload, not IP traffic. Read the actual
// UDP captures, including every header, acknowledgement and retransmission.
export async function captureProbeTraffic(pairs,{path=null,extraPorts=[]}={}){
  const directory=path?null:await mkdtemp(join(tmpdir(),'bfe-wire-')),filename=path??join(directory,'traffic.pcap');
  const ports=[...pairs.map(pair=>pair.local.port),...extraPorts],args=['-U','-n','-i','lo','-w',filename,'udp and ('+ports.map(port=>`port ${port}`).join(' or ')+')'];
  const child=process.getuid()===0?spawn('tcpdump',args,{stdio:['ignore','ignore','pipe']}):spawn('sudo',['-n','tcpdump',...args],{stdio:['ignore','ignore','pipe']});
  let stderr='';child.stderr.on('data',chunk=>{stderr+=chunk;});
  const exited=new Promise(resolve=>child.once('exit',resolve));
  await sleep(200);if(child.exitCode!==null)throw Error('Packet capture failed: '+stderr);
  let stopped=false;
  return {async stop({begunUnix=-Infinity,endedUnix=Infinity}={}){
    if(stopped)return null;stopped=true;privileged('kill',['-INT',String(child.pid)]);await exited;
    const bytes=await readFile(filename),little=bytes.readUInt32LE(0)===0xa1b2c3d4,u32=at=>little?bytes.readUInt32LE(at):bytes.readUInt32BE(at),link=u32(20);
    const clients=pairs.map(()=>({upBytes:0,downBytes:0,upPackets:0,downPackets:0,upOneSecondBytes:[]}));let competingBytes=0;
    for(let at=24;at+16<=bytes.length;){
      const time=u32(at)+u32(at+4)/1e6,size=u32(at+8);at+=16;const frame=bytes.subarray(at,at+size);at+=size;if(time<begunUnix||time>=endedUnix)continue;
      const offset=link===1?14:link===113?16:link===276?20:null;if(offset===null)throw Error('Unsupported capture link type '+link);
      const ip=frame.subarray(offset);let length,udp;
      if(ip[0]>>4===4){if(ip[9]!==17)continue;length=ip.readUInt16BE(2);udp=ip.subarray((ip[0]&15)*4);}
      else if(ip[0]>>4===6){if(ip[6]!==17)continue;length=40+ip.readUInt16BE(4);udp=ip.subarray(40);}else continue;
      const source=udp.readUInt16BE(0),destination=udp.readUInt16BE(2);
      if(extraPorts.includes(source))competingBytes+=length;
      for(let i=0;i<pairs.length;i++){const c=clients[i];if(source===pairs[i].local.port){c.upBytes+=length;c.upPackets++;if(Number.isFinite(begunUnix)){const bin=Math.floor(time-begunUnix);c.upOneSecondBytes[bin]=(c.upOneSecondBytes[bin]??0)+length;}}else if(destination===pairs[i].local.port){c.downBytes+=length;c.downPackets++;}}
    }
    if(directory)await rm(directory,{recursive:true,force:true});
    return {pcap:path,seconds:endedUnix-begunUnix,clients,competingBytes,meaning:'Captured IP bytes on isolated loopback; includes UDP and transport headers, acknowledgements and retransmissions, excludes link-layer framing'};
  }};
}
