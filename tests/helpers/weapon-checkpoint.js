// Codec fixtures obtain dirty weapon state through the production mounted
// prediction frame. No JavaScript timer, spread or reload controller is kept.
import {CameraFrame} from './cameras.js';
import {createClientPrediction} from '../../src/runtime/client-prediction.js';
export function weaponCheckpoint(rows,ticks=13,{pressed=true,heldTicks=ticks}={}){
  const host=new CameraFrame(),collision=host.collision;
  const session=createClientPrediction(collision.fixtureInstance,collision);
  const owners=rows.map(row=>row.state?row:{state:row,barrel:0});
  const entries=owners.map(owner=>({weapon:owner.state,owner,barrel:owner.barrel,advancesBarrel:true,towHeld:false}));
  const unit={id:1,generation:1,seat:{}};
  const commands=Array.from({length:ticks},(_,i)=>({tick:i+1,sequence:i+1,generation:1,down:i<heldTicks?['Mouse0','Mouse2']:[],actions:pressed&&i===0?['firePrimary','fireSecondary']:[],look:[]}));
  try{session.mounted(unit,entries,commands,0,[]);return owners.map(owner=>owner.barrel);}
  finally{session.dispose();}
}
