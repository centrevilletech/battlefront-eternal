#!/usr/bin/env python3
"""Run the operator's RTC harness under real UDP loss in a private Linux namespace.

Only the namespace's UDP loopback traffic gets netem. The host's routes,
interfaces, DNS, SSH and application traffic remain outside this namespace.
Requires passwordless sudo, iproute2 and locally owned gameplay data.
"""
import argparse
import json
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import sys
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server-repo', type=Path, default=Path(__file__).resolve().parents[2] / 'battlefront-eternal-server')
    parser.add_argument('--engine', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--data', type=Path)
    parser.add_argument('--client', choices=('chromium',), default='chromium')
    parser.add_argument('--playwright-module', type=Path, default=os.environ.get('PLAYWRIGHT_MODULE'))
    parser.add_argument('--seed', type=int, default=20261012, help='Verified kernel netem PRNG seed; changed packet cadence still changes loss events')
    parser.add_argument('--players', type=int, choices=(1, 2, 8, 32, 64), default=2)
    parser.add_argument('--seconds', type=int, default=20)
    parser.add_argument('--delay', type=int, default=50, help='UDP delay in milliseconds in each direction')
    parser.add_argument('--jitter', type=int, default=15)
    parser.add_argument('--loss', type=float, default=5, help='Actual UDP packet loss percentage')
    parser.add_argument('--warmup', type=int, default=2)
    parser.add_argument('--input-lead-ms', type=int, help='Command lead; defaults to the scenario round trip plus jitter')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not 0 < args.seed < 2**64:
        parser.error('Seed must be an unsigned nonzero 64-bit integer')
    if not (1 <= args.seconds <= 600 and 0 <= args.delay <= 500 and 0 <= args.jitter <= 200 and 0 <= args.loss <= 20):
        parser.error('Scenario exceeds the bounded local test limits')
    if not 0 <= args.warmup <= 60:
        parser.error('Warmup must be between zero and 60 seconds')
    if args.input_lead_ms is None:
        args.input_lead_ms = 2 * (args.delay + args.jitter)
    if not 0 <= args.input_lead_ms <= 1400:
        parser.error('Input lead exceeds the bounded local test limits')
    if not args.playwright_module or not args.playwright_module.is_file():
        parser.error('Chromium requires --playwright-module or PLAYWRIGHT_MODULE')
    node = shutil.which('node')
    ip = shutil.which('ip')
    tc = shutil.which('tc') or '/usr/sbin/tc'
    if not node or not ip or not Path(tc).is_file():
        parser.error('node, ip and tc are required')
    namespace = 'bfe-netem-' + uuid.uuid4().hex[:10]
    username = pwd.getpwuid(os.getuid()).pw_name
    sudo = ['sudo', '-n']
    run = lambda command, **kwargs: subprocess.run(command, check=True, **kwargs)
    ns = sudo + [ip, 'netns', 'exec', namespace]
    created = False
    try:
        run(sudo + [ip, 'netns', 'add', namespace]); created = True
        # A dummy address gives ICE a non-loopback host candidate. Its local
        # route still crosses this namespace's loopback UDP queue.
        run(ns + [ip, 'link', 'set', 'lo', 'up'])
        run(ns + [ip, 'link', 'add', 'bfe0', 'type', 'dummy'])
        run(ns + [ip, 'addr', 'add', '192.0.2.2/32', 'dev', 'bfe0'])
        run(ns + [ip, 'link', 'set', 'bfe0', 'up'])
        run(ns + [tc, 'qdisc', 'add', 'dev', 'lo', 'root', 'handle', '1:', 'prio'])
        run(ns + [tc, 'qdisc', 'add', 'dev', 'lo', 'parent', '1:3', 'handle', '30:', 'netem',
                  'delay', f'{args.delay}ms', f'{args.jitter}ms', 'loss', f'{args.loss}%', 'limit', '10000'])
        run(ns + [tc, 'filter', 'add', 'dev', 'lo', 'protocol', 'ip', 'parent', '1:', 'prio', '1',
                  'u32', 'match', 'ip', 'protocol', '17', '0xff', 'flowid', '1:3'])
        run(ns + [sys.executable, str(Path(__file__).with_name('netem-seed.py')), str(args.seed)], capture_output=True, text=True)
        command = ns + ['runuser', '-u', username, '--', 'env',
                        f'BFE_ENGINE_DIR={args.engine.resolve()}',
                        f'BFE_DATA_DIR={(args.data or args.engine / "public").resolve()}',
                        f'BFE_SERVER_REPO={args.server_repo.resolve()}',
                        f'PLAYWRIGHT_MODULE={args.playwright_module.resolve()}', node,
                        'tools/multiplayer-chromium.mjs', f'--players={args.players}',
                        f'--seconds={args.seconds}', f'--warmup={args.warmup}',
                        f'--input-lead-ms={args.input_lead_ms}']
        command_cwd = args.engine.resolve()
        result = subprocess.run(command, cwd=command_cwd, capture_output=True, text=True,
                                timeout=args.seconds + args.warmup + 180)
        if result.stderr:
            sys.stderr.write(result.stderr)
        try:
            report = json.loads(result.stdout)
        except json.JSONDecodeError:
            if not result.returncode:
                raise
            report = {'harnessOutput': result.stdout[-8000:]}
        entity_mismatch = (report.get('profile') != 'entity-v1' or len(report.get('negotiated', [])) != args.players or not all(c.get('entity') for c in report.get('negotiated', [])))
        if entity_mismatch:
            report['protocolFailure'] = 'Every actual Chromium peer must negotiate entity-v1; fallback is not a qualification result'
        if result.returncode or entity_mismatch:
            report['failed'] = True
            report['harnessFailure'] = {'exitCode': result.returncode, 'stderr': result.stderr[-12000:]}
        report['netemSeed'] = args.seed
        report['netemSeedMeaning'] = 'Verified kernel PRNG initialization; changed packet cadence changes which packets encounter loss'
        report['clientBackend'] = args.client
        report['requestedProtocol'] = {'revision': 'entity-v1', 'protocol': 2, 'simulation': 'bfe-sim-6', 'stateLifeMs': 50}
        report['applicationImpairment'] = report.get('impairment')
        report['impairment'] = {'kind': 'Linux tc netem on isolated namespace UDP traffic',
                                'delayEachDirectionMs': args.delay, 'jitterMs': args.jitter, 'packetLossPercent': args.loss}
        report['netemCounters'] = run(ns + [tc, '-s', 'qdisc', 'show', 'dev', 'lo'], capture_output=True, text=True).stdout
        report['netemCountersScope'] = 'Entire scenario, including connection setup and warmup; both UDP directions'
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'report': str(args.output), 'players': args.players, 'status': 'failed' if result.returncode or entity_mismatch else 'completed'}))
        if entity_mismatch:
            raise RuntimeError(f'Entity profile was not negotiated; diagnostic report preserved at {args.output}')
        if result.returncode:
            raise RuntimeError(f'RTC harness exited {result.returncode}; diagnostic report preserved at {args.output}')
    finally:
        if created:
            # Reap only processes inside the namespace this invocation made.
            for signal in ('TERM', 'KILL'):
                pids = subprocess.run(sudo + [ip, 'netns', 'pids', namespace], capture_output=True, text=True).stdout.split()
                if not pids:
                    break
                subprocess.run(sudo + ['kill', '-' + signal, *pids], check=False)
                time.sleep(.2)
            subprocess.run(sudo + [ip, 'netns', 'delete', namespace], check=False)


if __name__ == '__main__':
    main()
