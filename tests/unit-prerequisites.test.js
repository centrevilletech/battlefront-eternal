import test from 'node:test';
import assert from 'node:assert/strict';
import { UnitFactory } from '../src/units.js';

test('client prerequisites include infantry and droidekas but exclude probe droids', async () => {
  const factory = new UnitFactory({
    imp: { units: { imp_inf_soldier: { base: 'soldier' }, imp_droid_probe: { base: 'droid' } } },
    cis: { units: { cis_inf_droideka: { base: 'walkerdroid' } } },
    ewk: { units: { ewk_inf_soldier: { base: 'soldier' } } },
  }, {});
  const prepared = [];
  factory.prepare = async name => { prepared.push(name); };
  await factory.prepareSoldiers();
  assert.deepEqual(prepared.sort(), ['cis_inf_droideka', 'ewk_inf_soldier', 'imp_inf_soldier']);
});
