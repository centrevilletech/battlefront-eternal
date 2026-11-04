import { selectedWeapon, weaponAI } from './ai/classic/weapons-ai.js';

function* levels(unit) {
  for(const weapon of unit?.combat?.arsenal?.weapons||[])
    for(const level of weapon.levels||[weapon.def])yield level;
}
export class NativeRanges {
  constructor(sim) {
    this.sim=sim;this.frames=[];
    sim.bridge.register({aiRange:args=>{
      const op=args.op,id=args.id,number=args.number,frame=this.frames[this.frames.length-1],{unit,controller}=frame;
      const result={flag:false,number:0,values:[]};
      const scalar=value=>{result.flag=typeof value==='number';if(result.flag)result.number=value;};
      const compare=(left,right)=>{frame.left=left;frame.right=right;result.flag=typeof left==='number'&&typeof right==='number';if(result.flag)result.values=[left,right];};
      if(op==='speed'){const cls=unit?.classDef;result.number=cls?.ForwardSpeed??cls?.MaxSpeed??0;}
      else if(op==='start'){frame.ai=weaponAI(frame.def);result.flag=Boolean(frame.ai);}
      else if(op==='max'){frame.range=frame.ai.maxRange;scalar(frame.range);}
      else if(op==='implicit')result.flag=Boolean(frame.ai.implicitRange);
      else if(op==='kind'){const kind=unit?.kind;result.number=typeof kind==='number'?kind:NaN;}
      else if(op==='double')result.number=frame.range+frame.range;
      else if(op==='store')frame.range=number;
      else if(op==='scale')result.number=controller?.rangeScale??1;
      else if(op==='number')result.number=frame.range;
      else if(op==='scaledExtreme')compare(number,frame.ai.extremeRange);
      else if(op==='extremeRange')compare(frame.ai.extremeRange,frame.range);
      else if(op==='compare')result.flag=frame.left<=frame.right;
      else if(op==='publish'){frame.result=id===0?number:id===1?frame.range:frame.ai.extremeRange;scalar(frame.result);}
      else if(op==='add')result.number=frame.result+number;
      else if(op==='subtract')result.number=frame.result-number;
      else if(op==='selected'){const weapon=selectedWeapon(unit,controller);result.flag=Boolean(weapon);if(weapon)frame.def=weapon.def;}
      else if(op==='levels')frame.iterator=levels(unit);
      else if(op==='next'){const next=frame.iterator.next();result.flag=!next.done;if(result.flag)frame.def=next.value;}
      else if(op==='band'){frame.weapon=selectedWeapon(unit,controller);frame.band=weaponAI(frame.weapon?.def);result.flag=Boolean(frame.band);}
      else if(op==='minimum'){frame.minimum=frame.band.minRange;scalar(frame.minimum);}
      else if(op==='minimumAdd')result.number=frame.minimum+10;
      else if(op==='optimalCompare')compare(frame.band.optimalRange,number);
      else if(op==='optimalResult'){frame.result=frame.band.optimalRange;scalar(frame.result);}
      else if(op==='bandDef')frame.def=frame.weapon.def;
      else if(op==='highCompare')compare(number,frame.band.optimalRange);
      else if(op==='highSelect')frame.high=id===0?number:frame.band.optimalRange;
      else if(op==='sight')result.number=frame.sight;
      else if(op==='sightHigh')compare(number,frame.high);
      else if(op==='highResult'){frame.result=frame.high;scalar(frame.result);}
      else throw Error(`unknown range callback ${op}`);
      return result;
    }});
  }
  run(mode,unit,controller,def,sight) {
    const frame={unit,controller,def,sight};this.frames.push(frame);
    try {this.sim.x.ai_weapon_ranges(mode);return frame.result;}
    catch(error){try{frame.iterator?.return?.();}catch{}throw error;}
    finally{this.frames.pop();}
  }
}
