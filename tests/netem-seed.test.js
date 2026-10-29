import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';

test('netem seed preserves queue configuration and replaces only PRNG attribute',{skip:process.platform!=='linux'},()=>{
 const result=spawnSync('python3',['-c',`
import importlib.util, struct
spec=importlib.util.spec_from_file_location('seed','tools/netem-seed.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
header=bytes(range(24))
original=header+module.attribute(10,struct.pack('=q',50000000))+module.attribute(11,struct.pack('=q',15000000))+module.attribute(14,struct.pack('=Q',1))
changed=module.seeded_options(original,20261012)
assert changed[:24]==header
attrs=dict(module.attributes(changed[24:]))
assert attrs[10]==struct.pack('=q',50000000)
assert attrs[11]==struct.pack('=q',15000000)
assert attrs[14]==struct.pack('=Q',20261012)
assert len(list(module.attributes(changed[24:])))==3
try: list(module.attributes(b'\\x03\\x00\\x01\\x00'))
except ValueError: pass
else: raise AssertionError('invalid attribute accepted')
`],{encoding:'utf8'});
 assert.equal(result.status,0,result.stderr);
});
