import test from 'node:test';
import assert from 'node:assert/strict';
import {recordOf,personalStats,awards,clock} from '../src/battle-stats.js';
import {Character} from '../src/battle.js';
const character=(team,name,index=0)=>Object.assign(new Character(index,team),{name});

// The page formats native published records. Tallying and ring mutation are
// covered by the native phase fixtures, so these goldens supply their values.
test('DETAILS formats published favourites, streaks and victim tables',()=>{
  const me=character(1,'ME'),foe=character(2,'UNIT 538'),other=character(2,'UNIT 182');
  const own=recordOf(me).slots[0];
  own.headshots=2;own.lifeBest=187.9;
  own.classes.push({key:'rep_inf_trooper',label:'TROOPER',time:187.9});
  own.weapons.push({key:'rep_weap_rifle',label:'REP_WEAP_RIFLE',time:187.9});
  own.victims.set(other,1);recordOf(foe).slots[1].victims.set(me,2);
  const values=Object.fromEntries(personalStats(me,[me,foe,other],{label:()=>null}).map(r=>[r.label,r.value]));
  assert.deepEqual(values,{'HEAD SHOTS':'2','FAVORITE CLASS':'TROOPER 3:07','FAVORITE WEAPON':'REP_WEAP_RIFLE 3:07',
    'FAVORITE VEHICLE':'','LONGEST LIVING STREAK':' 3:07','NARROW ESCAPES':'0','VEHICLES DESTROYED':'0',
    NEMESIS:'UNIT 538 (2)',BAIT:'UNIT 182 (1)'});
  assert.equal(clock(59.99),'0:59');assert.equal(clock(3600),'60:00');
});
test('AWARDS retains team-slot and roster order for equal published tallies',()=>{
  const a=character(1,'A'),b=character(2,'B'),c=character(1,'C');
  recordOf(a).slots[0].campBest=125.7;recordOf(a).slots[0].deaths=1;
  recordOf(b).slots[1].vehiclesDestroyed=1;recordOf(b).slots[1].deaths=3;
  recordOf(c).slots[0].vehiclesDestroyed=1;recordOf(c).slots[0].headshots=1;
  assert.deepEqual(Object.fromEntries(awards([a,b,c]).map(r=>[r.label,r.text])),{
    'TANK BUSTER':'C (1)','KILLING SPREE':'',CAMPER:'A (2:05)',SURVIVALIST:'',TRAITOR:'','PUBLIC ENEMY':'',
    'BANTHA FODDER':'B (3)','DEAD EYE':'C (1)'});
});
