// Each role gets a fresh process pinned by the parent. CPU windows exclude
// admission, compilation and warmup; packet delivery still uses real codecs.
import {writeFile} from 'node:fs/promises';
import {backendProvider,manifest} from './common.mjs';
const config=JSON.parse(process.env.BFE_QUALIFICATION_CASE);
const provider=await backendProvider({backend:config.backend,module:config.module,engine:config.engine,manifest:await manifest(config.engine)});
const battle=await provider.open({query:`?map=${config.map}&side=${config.side}&ai=classic&seed=1`,data:config.data,clients:4,measure:true});
try{
  for(let i=0;i<4;i++)await battle.join(`cpu-${i}`);
  battle.step(3000);
  battle.resetMeasurements?.();
  const start=performance.now(),cpu=process.cpuUsage(),thread=process.threadCpuUsage(),before=battle.tick();
  battle.step(6000);
  const elapsed=performance.now()-start,processCpu=process.cpuUsage(cpu),threadCpu=process.threadCpuUsage(thread);
  const tick=battle.tick(),diagnostics=battle.diagnostics(),measurements=battle.measurements?.();
  if(tick-before!==6000)throw Error('Incomplete CPU window');
  if(!measurements||measurements.steps!==6000||measurements.publications!==6000)throw Error('Backend must expose separate step/publication measurements');
  if(config.backend==='native'){for(const field of ['allocations','allocationBytes'])if(!Number.isFinite(measurements[field]))throw Error(`Missing native ${field}`);if(!measurements.boundary)throw Error('Missing native boundary counters');}
  await writeFile(config.out,JSON.stringify({backend:config.backend,map:config.map,warmTicks:3000,ticks:6000,tickDriven:true,droppedTicks:0,wallMs:elapsed,processCpu,threadCpu,
    threadMicrosPerTick:(threadCpu.user+threadCpu.system)/6000,measurements,rss:process.memoryUsage().rss,diagnostics}));
}finally{battle.close();}
