// Harness-owned inputs, never authoritative state or expected fingerprints.
// Cold assets are shared by content hash; tick rows preserve f64 bits so a
// negative zero or NaN payload cannot change while passing through JSON.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { gzipSync, gunzipSync } from 'node:zlib';
import { HOST_SCHEMA } from '../src/sim/host-schema.js';
import { BlockHeaderLayout } from '../src/sim/gen/boundary.js';
import { WorldTickSetupLayout, WorldTickInputLayout } from '../src/sim/gen/world.js';
export const CORPUS_VERSION = 1;
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export function readSetup(directory) {
  const setup = JSON.parse(fs.readFileSync(path.join(directory,'setup.json'),'utf8'));
  if (setup.version !== CORPUS_VERSION || setup.schema !== HOST_SCHEMA) throw Error('World corpus version/schema mismatch');
  return setup;
}
export class WorldCorpus {
  constructor({directory, replay=false, blobs, argv=[]}) {
    this.directory=directory;this.replay=replay;this.assets={};this.frames=[];this.at=0;this.rows=[];
    this.blobs=blobs || path.join(directory,'..','blobs');
    if (replay) {
      this.setup=readSetup(directory);this.assets=this.setup.assets;
      const bytes=fs.readFileSync(path.join(directory,'inputs.json.gz'));
      if (digest(bytes)!==this.setup.inputsSha256) throw Error('World corpus input checksum mismatch');
      this.frames=JSON.parse(gunzipSync(bytes));
      if (this.frames.length!==this.setup.frames) throw Error('World corpus frame count mismatch');
      if (this.setup.runtime.v8!==process.versions.v8) throw Error('World corpus V8 mismatch');
    } else {
      fs.mkdirSync(directory,{recursive:true});fs.mkdirSync(this.blobs,{recursive:true});
      this.setup={battles:[],version:CORPUS_VERSION,schema:HOST_SCHEMA,argv,assets:this.assets,
        runtime:{node:process.version,v8:process.versions.v8},commit:process.env.BFE_COMMIT||null,
        clock:{start:1000,unix:1700000000000,step:1000/60},random:{initial:991,multiplier:1664525,increment:1013904223}};
    }
  }
  // Missing files are inputs too. Replay never consults the installed game
  // directory, even for a URL absent from the recorded admission manifest.
  readAsset(url, read) {
    if (!this.replay) {
      const bytes=read(url);const sha=bytes===null?null:digest(bytes);
      if (Object.hasOwn(this.assets,url) && (this.assets[url]?.sha256??null)!==sha) throw Error(`Cold asset changed during recording: ${url}`);
      this.assets[url]=sha===null?null:{sha256:sha,bytes:bytes.length};
      if (bytes!==null) {
        try { const file=path.join(this.blobs,`${sha}.gz`);if(!fs.existsSync(file))fs.writeFileSync(file,gzipSync(bytes),{flag:'wx'}); }
        catch(error) {if(error.code!=='EEXIST')throw error;}
      }
      return bytes;
    }
    if (!Object.hasOwn(this.assets,url)) throw Error(`Unrecorded cold asset: ${url}`);
    const asset=this.assets[url];if(asset===null)return null;
    const bytes=gunzipSync(fs.readFileSync(path.join(this.blobs,`${asset.sha256}.gz`)));
    if(bytes.length!==asset.bytes||digest(bytes)!==asset.sha256)throw Error(`Cold asset checksum mismatch: ${url}`);
    return bytes;
  }
  startBattle(index,now,draw) {
    this.battle=this.replay?this.setup.battles[index]:{index,now,draw};
    if(!this.battle||this.battle.index!==index)throw Error('World corpus battle order mismatch');
    if(!this.replay)this.setup.battles.push(this.battle);
    return this.battle;
  }
  begin(frame) {
    this.rows=[];
    if(this.replay) {
      const recorded=this.frames[this.at];
      if(!recorded||recorded.battle!==frame.battle||recorded.index!==frame.index)throw Error('World corpus tick order mismatch');
      this.current=recorded;return recorded;
    }
    this.current=frame;return frame;
  }
  finishFrame() {
    if(this.replay) {
      if(this.rows.length!==this.current.world.length)throw Error('World corpus world-step count mismatch');
    } else {this.current.world=this.rows;this.frames.push(this.current);}
    this.at++;
  }
  // The wrapper changes no production module. It calls the same coordinator
  // exports as WorldTick, after its normal preparation and before readback.
  async simulationFactory(load) {
    const instance=await load(),x=instance.exports;let address=null,argsBlock=null;
    const exports={...x,sim_call_room:(words,heap)=>{argsBlock=x.sim_call_room(words,heap);return argsBlock;},world_step_init:()=>{
      if(this.battle) {
        if(argsBlock===null)throw Error('World setup has no admission block');
        const setup=Buffer.from(x.memory.buffer,argsBlock+BlockHeaderLayout.words*8,WorldTickSetupLayout.words*8).toString('base64');
        if(this.replay&&setup!==this.battle.worldSetup)throw Error('World setup input differs');
        if(!this.replay)this.battle.worldSetup=setup;
      }
      address=x.world_step_init();return address;
    },world_step_run:()=>{
      if(address===null||!this.current)throw Error('World step outside a recorded tick');
      const bytes=Buffer.from(x.memory.buffer,address,WorldTickInputLayout.words*8);
      if(this.replay) {
        const encoded=this.current.world[this.rows.length];
        if(encoded===undefined)throw Error('Unexpected world step');
        const input=Buffer.from(encoded,'base64');
        if(input.length!==bytes.length||!input.equals(bytes))throw Error(`World input differs at battle ${this.current.battle}, tick ${this.current.index}`);
        input.copy(bytes);
      }
      this.rows.push(bytes.toString('base64'));return x.world_step_run();
    }};
    return {exports,holder:instance.holder};
  }
  finish(options) {
    if(this.replay) {
      if(this.at!==this.frames.length)throw Error('World corpus ended early');
      return;
    }
    const bytes=gzipSync(Buffer.from(JSON.stringify(this.frames)));
    fs.writeFileSync(path.join(this.directory,'inputs.json.gz'),bytes);
    Object.assign(this.setup,{options,frames:this.frames.length,inputsSha256:digest(bytes),compressedBytes:bytes.length});
    fs.writeFileSync(path.join(this.directory,'setup.json'),JSON.stringify(this.setup)+'\n');
  }
}
