import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { Units } from '../src/registry.js';
import { assignUnitId } from '../src/runtime/entity-identity.js';

test('out-of-order replica admission retains canonical registry order', () => {
  const units = new Units(), list = units.list;
  const make = id => {
    const unit = units.add({generation:1,team:1,kind:0,alive:true,removed:false,
      position:new Vector3(),classDef:{},yaw:0});
    return assignUnitId(units,unit,id);
  };
  const high = make(80), low = make(2), middle = make(40);
  assert.equal(units.list,list);
  assert.deepEqual(units.list,[low,middle,high]);
  assignUnitId(units,high,3);
  assert.deepEqual(units.list,[low,high,middle]);
  assert.equal(units.byId(80),null);assert.equal(units.byId(3),high);
});

test('rejected replica identity changes leave order and bindings intact', () => {
  const units = new Units(), a = units.add({}), b = units.add({});
  assert.throws(()=>assignUnitId(units,b,a.id),/collision/);
  assert.deepEqual(units.list,[a,b]);
  assert.equal(units.byId(a.id),a);assert.equal(units.byId(b.id),b);
});
