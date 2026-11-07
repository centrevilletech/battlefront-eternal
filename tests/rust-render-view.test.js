import {sceneComponent,sceneProperty} from "../src/rust-render/scene-changes.js";
import {prepareSceneFixture} from "./support/rust-standin.mjs";
// First-person rigid parts use native source/appearance rows. Scroll changes
// replace the cold appearance; live lighting still writes the retained row.
import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {RustRender} from '../src/rust-render/index.js';
import {actorAsset,actorNode,actorLook} from '../src/actor-resources.js';
import {standinModule,standinThree} from './support/rust-standin.mjs';
globalThis.location??={search:''};
test('a scrolling repair-tool part retains its native descriptor until appearance replacement',()=>{
 const {BfeRenderer,memory,calls}=standinModule(),{graphics,renderer}=standinThree(calls),core=prepareSceneFixture(new RustRender(graphics,{memory},BfeRenderer));
 const node=actorNode(actorAsset({kind:'plane'}),actorLook('dynamic',{scroll:[.25,-.5]}));sceneComponent(node.position,"z","=",-2);graphics.viewScene.add(node);
 const draw=()=>{graphics.viewScene.updateMatrixWorld(true);core.cover('view',renderer,graphics.viewScene,graphics.viewCamera);};
 draw();draw();let rows=calls.filter(c=>c[0]==='actor_material');assert.equal(rows.length,1);assert.deepEqual(JSON.parse(rows[0][2]).scroll,[.25,-.5]);assert.equal(rows[0][6],true);
 sceneProperty(node,"material",actorLook('dynamic',{scroll:[-1,2]}));draw();rows=calls.filter(c=>c[0]==='actor_material');assert.equal(rows.length,2);assert.deepEqual(JSON.parse(rows[1][2]).scroll,[-1,2]);
 sceneProperty(node,"material",actorLook('dynamic'));draw();assert.equal(JSON.parse(calls.filter(c=>c[0]==='actor_material').at(-1)[2]).scroll,undefined);assert.equal(node.geometry,undefined);
});
