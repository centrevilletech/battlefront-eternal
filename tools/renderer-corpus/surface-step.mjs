// Browser tools serialize the real transport hosts, so fixtures use one
// installed native surface owner instead of retired per-component updates.
import {SurfaceFrame} from '../../src/environment/frame.js';
import {SurfaceHost} from '../../src/environment/surfaces.js';
import * as layout from '../../src/environment/layout.js';
import {stepBattleSurfaces} from '../../tests/helpers/surfaces.js';
export function surfaceStepSource(){
 const aliases={FRAME:'SurfaceFrame',RESULT:'SurfaceResult',LIMITS:'Surface',CONFIG:'SurfaceConfig',COMMAND:'SurfaceCommand',KIND:'SurfaceCommandKind',TRIANGLE:'ShoreTriangle'};
 return `(()=>{${Object.entries(aliases).map(([name,key])=>`const ${name}=${JSON.stringify(layout[key])};`).join('')}const empty=[],battles=new WeakMap();const SurfaceHost=${SurfaceHost.toString()};const SurfaceFrame=${SurfaceFrame.toString()};window.__stepBattleSurfaces=${stepBattleSurfaces.toString()};})()`;
}
