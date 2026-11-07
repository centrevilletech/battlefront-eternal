// The supervisor is outside the tested service cgroup. A killed or hung
// payload rejects every waiter and still leaves a bounded diagnostic record.
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
export class Service {
  constructor(command,args){
    this.child=spawn(command,args,{stdio:['pipe','pipe','pipe']});this.pending=new Map();this.next=0;this.stderr='';this.logs=[];
    this.ready=new Promise((resolve,reject)=>{this.readyResolve=resolve;this.readyReject=reject;});
    this.child.stderr.on('data',chunk=>{this.stderr=(this.stderr+chunk).slice(-16000);});
    this.child.stdin.on('error',error=>this.fail(error));
    this.child.on('error',error=>this.fail(error));
    this.exit=new Promise(resolve=>this.child.once('exit',(code,signal)=>{this.ended=true;this.fail(Error(`Service exited ${code??signal}`));resolve({code,signal});}));
    this.lines=createInterface({input:this.child.stdout});
    this.lines.on('line',line=>{
      let value;try{value=JSON.parse(line);}catch{if(this.logs.length<8)this.logs.push(line.slice(0,500));return;}
      if(value.type==='ready'){this.readyResolve(value);return;}
      const p=this.pending.get(value.id);if(!p)return;this.pending.delete(value.id);clearTimeout(p.timer);
      value.error?p.reject(Error(value.error)):p.resolve(value.result);
    });
  }
  fail(error){this.readyReject(error);for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(error);}this.pending.clear();}
  async call(op,args={},timeout=180000){
    await this.ready;if(this.ended)throw Error('Service exited');
    const id=++this.next;
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{this.pending.delete(id);reject(Error(`Service ${op} timed out`));},timeout);
      this.pending.set(id,{resolve,reject,timer});this.child.stdin.write(JSON.stringify({id,op,...args})+'\n');
    });
  }
  async close(){
    if(!this.ended){try{await this.call('shutdown',{},15000);}catch{this.child.kill('SIGTERM');}}
    const timeout=setTimeout(()=>this.child.kill('SIGKILL'),20000);
    try{return await this.exit;}finally{clearTimeout(timeout);this.lines.close();}
  }
}
