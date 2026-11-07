#!/usr/bin/env python3
"""Privileged farm checks for the supervisor, independent of world solvers."""
import argparse
import json
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--backend', required=True, choices=['wasm', 'native'])
parser.add_argument('--out', required=True)
args = parser.parse_args()
helper = Path(__file__).with_name('cgroup.py')
results = []
with tempfile.TemporaryDirectory(prefix='host-cgroup-check-') as directory:
    # Each fixture uses its own small ceiling. These exercise kernel evidence
    # and teardown; the real paired service still has the exact 1 GiB ceiling.
    for name in ['normal', 'ignored-signal', 'oom']:
        destination = Path(directory) / (name + '.json')
        source = "print('ready',flush=True)"
        if name == 'ignored-signal':
            source = "import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);print('ready',flush=True);time.sleep(60)"
        elif name == 'oom':
            source = "a=bytearray(384*1048576);a[::4096]=b'x'*len(a[::4096])"
        command = [sys.executable, str(helper), '--out', str(destination), '--bytes', '268435456', '--', sys.executable, '-c', source]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
        try:
            if name == 'ignored-signal':
                assert select.select([process.stdout], [], [], 10)[0], 'fixture did not become ready'
                assert process.stdout.readline().strip() == 'ready'
                process.send_signal(signal.SIGTERM)
            process.communicate(timeout=20)
        finally:
            # A test assertion must still give the supervisor its own grace
            # window to reclaim the payload and write its final report.
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                process.communicate(timeout=15)
        result = json.loads(destination.read_text())
        assert result['cleaned'] and not result['remainingPids'], result
        assert result['memory']['maximum'] == 268435456, result
        assert process.returncode == (0 if name == 'normal' else 1), result
        if name == 'ignored-signal':
            assert result['forcedShutdown'] and result['exitCode'] == -signal.SIGKILL, result
        if name == 'oom':
            assert result['memory']['events']['oom_kill'] > 0, result
        results.append({'case': name, **result})
# Successful signal/OOM fixtures prove OS reclamation, never Rust Drop calls.
value = {'passed': True, 'backend': args.backend, 'scope': 'cgroup supervisor', 'cases': results}
destination = Path(args.out)
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(json.dumps(value, indent=2) + '\n')
print(json.dumps({'passed': True, 'cases': len(results), 'out': args.out}))
