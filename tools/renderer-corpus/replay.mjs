// Frozen GPU inputs come from the native battle frame. Replays restore each
// observed program, resource and raster state; they never drive game phases.
export function installReplay(createStore) {
  const T=window.__corpus;
  // Each checkpoint replaces the blob bank; old draws retain no live resources.
  T.records=[];T.blobs={};let owner,sequence=0;
  T.store=createStore?createStore(T):data=>{if(owner!==T.blobs){owner=T.blobs;sequence=0;}const key=String(sequence++);T.blobs[key]=data;return key;};
  T.replay=async(records,blobs)=>{
    const metrics=[];T.want=false;
    for(const row of records){if(row.kind!=='gl-effect')throw Error('Unknown GPU input record '+row.kind);metrics.push(await T.replayGL(row,blobs));}
    return metrics;
  };
  return 1;
}
