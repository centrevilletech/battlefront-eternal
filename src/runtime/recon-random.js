import {RandomStreams} from '../ai/random.js';
// The private stream registry cannot grow with disconnected owners. Native
// replay reads and publishes its fixed seed word without boxing a JS uint32;
// ordinary callers retain the same seeded draw and checkpoint interface.
export function reconRandom(seed,key){
  const stream=new RandomStreams(seed).get(`network-recon:${key}`);
  const stateWords=new Uint32Array([stream.seed]);
  const random=()=>{
    if(stream.seed!==stateWords[0])stream.seed=stateWords[0];
    const value=stream.uniform();stateWords[0]=stream.seed;return value;
  };
  random.stateWords=stateWords;
  Object.defineProperty(random,'seed',{get:()=>stateWords[0],set:value=>{stream.seed=value;stateWords[0]=value;}});
  return random;
}
