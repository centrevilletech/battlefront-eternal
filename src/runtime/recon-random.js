import {RandomStreams} from '../ai/random.js';
// The temporary stream registry is discarded; disconnected owner names cannot
// accumulate in the world's global RNG registry. State is explicit for replay.
export function reconRandom(seed,key){const stream=new RandomStreams(seed).get(`network-recon:${key}`),random=()=>stream.uniform();Object.defineProperty(random,'seed',{get:()=>stream.seed,set:value=>{stream.seed=value;}});return random;}
