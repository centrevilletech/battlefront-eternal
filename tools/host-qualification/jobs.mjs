// Job definitions are shared by the farm scheduler and its content-free tests.
// Durations change only through the explicit proof flag; CPU ticks never do.
export const maps=[['nab2','rep'],['tat1','rep'],['bes1','rep'],['hot1','imp']];
export function jobs({proof=false}={}){
  const seconds=proof?300:1800;
  return [
    ...[0,1,2].map(shard=>({id:`Q1-${shard+1}`,kind:'matrix',role:shard,shard})),
    {id:'Q2',kind:'packets',role:2},
    ...maps.map(([map,side])=>({id:`Q3-${map}`,kind:'lifecycle',role:1,map,side,seconds})),
    {id:'Q4',kind:'memory',role:4,seconds},
    ...maps.slice(0,2).map(([map,side])=>({id:`Q6-${map}`,kind:'network',role:0,map,side,seconds:60})),
    {id:'Q7',kind:'desktop',role:0},
    {id:'Q8',kind:'mismatch',role:3},
    {id:'Q9',kind:'server-suite',role:3},
    {id:'Q5',kind:'cpu',role:2,exclusive:true},
  ];
}
export function shardRows(text,index,count=3){
  const rows=text.split('\n').map(row=>row.replace(/#.*/,'').trim()).filter(Boolean);
  // Keep the standing manifest's order and every original option. The
  // matrix executable compares all rows in its input, so pass a real subset.
  return rows.filter((_,i)=>i%count===index).join('\n')+'\n';
}
