#!/usr/bin/env python3
"""Run only the server payload in a disposable, hard-limited cgroup v2.

The load generator stays outside. Kernel peak/events survive an OOM-killed
payload and are recorded before removing the group. No host limits change.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid


def memory(group):
    def text(name):
        return (group / name).read_text().strip()
    return {'current': int(text('memory.current')), 'peak': int(text('memory.peak')),
            'maximum': int(text('memory.max')),
            'events': {k: int(v) for k, v in (line.split() for line in text('memory.events').splitlines())}}


def kill_group(group):
    if (group / 'cgroup.kill').exists():
        (group / 'cgroup.kill').write_text('1')
    else:
        for pid in (group / 'cgroup.procs').read_text().split():
            try:
                os.kill(int(pid), signal.SIGKILL)
            except ProcessLookupError:
                pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True)
    parser.add_argument('--bytes', type=int, default=1073741824)
    parser.add_argument('--uid', type=int, default=os.getuid())
    parser.add_argument('--gid', type=int, default=os.getgid())
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or not 268435456 <= args.bytes <= 8589934592:
        parser.error('A bounded memory limit and command are required')
    if os.geteuid() != 0:
        os.execvp('sudo', ['sudo', '-n', sys.executable, __file__, '--out', args.out,
                          '--bytes', str(args.bytes), '--uid', str(args.uid), '--gid', str(args.gid), '--', *command])
    root = Path('/sys/fs/cgroup')
    if not (root / 'cgroup.controllers').exists():
        # ip netns exec remounts sysfs in its private mount namespace. Reach
        # the same host cgroup filesystem through init's mount namespace;
        # this does not mount anything or change an existing group's limits.
        root = Path('/proc/1/root/sys/fs/cgroup')
    if not (root / 'cgroup.controllers').exists():
        raise RuntimeError('cgroup v2 is required; no heap-limit substitute')
    current = next(line.split('::', 1)[1] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))
    parent = root / current.lstrip('/')
    group = root / ('bfe-host-qualification-' + uuid.uuid4().hex)
    destination = Path(args.out); destination.parent.mkdir(parents=True, exist_ok=True)
    child = None
    report = {'limit': args.bytes, 'group': str(group), 'samples': 0, 'cleaned': False}
    affinity = os.sched_getaffinity(0)
    group.mkdir()
    try:
        (group / 'memory.max').write_text(str(args.bytes))
        (group / 'memory.swap.max').write_text('0')
        (group / 'memory.oom.group').write_text('1')
        # Retain the farm/operator CPU quota as well as inherited affinity.
        for name in ('cpu.max', 'pids.max'):
            if (parent / name).exists() and (group / name).exists():
                (group / name).write_text((parent / name).read_text())

        def enter():
            (group / 'cgroup.procs').write_text(str(os.getpid()))
            os.sched_setaffinity(0, affinity)
            os.setgroups([]); os.setgid(args.gid); os.setuid(args.uid)

        shutdown_deadline = None
        def shutdown(*_):
            nonlocal shutdown_deadline
            if shutdown_deadline is None:
                shutdown_deadline = time.monotonic() + 5
                if child is not None and child.poll() is None:
                    child.send_signal(signal.SIGTERM)
        for sig in (signal.SIGTERM, signal.SIGINT):
            signal.signal(sig, shutdown)
        child = subprocess.Popen(command, preexec_fn=enter)
        report['pid'] = child.pid
        destination.write_text(json.dumps(report) + '\n')
        while child.poll() is None:
            # Native work may never return to JS to handle SIGTERM. The
            # supervisor must kill its own group and finish its report before
            # an outer watchdog could terminate the supervisor itself.
            if shutdown_deadline is not None and time.monotonic() >= shutdown_deadline:
                report['forcedShutdown'] = True
                kill_group(group)
                shutdown_deadline = None
            report['memory'] = memory(group); report['samples'] += 1
            time.sleep(0.1)
        report['exitCode'] = child.returncode
        report['memory'] = memory(group)
    finally:
        # Kill only PIDs in this newly created group, including a child that
        # outlived its launcher. Never signal a process by a command pattern.
        kill_group(group)
        if child is not None:
            child.wait()
        for _ in range(100):
            if (group / 'cgroup.events').read_text().find('populated 0') >= 0:
                break
            time.sleep(0.02)
        report['remainingPids'] = (group / 'cgroup.procs').read_text().split()
        report['memory'] = memory(group)
        try:
            group.rmdir(); report['cleaned'] = True
        finally:
            destination.write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report.get('exitCode') == 0 and report['cleaned'] else 1


if __name__ == '__main__':
    sys.exit(main())
