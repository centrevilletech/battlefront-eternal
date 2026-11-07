#!/usr/bin/env python3
"""Prove disposable cgroup and network controls on a configured worker."""
import json
import os
from pathlib import Path
import subprocess
import uuid


def main():
    prefix = [] if os.geteuid() == 0 else ['sudo', '-n']
    run = lambda command: subprocess.run(prefix + command, check=True, capture_output=True, text=True)
    name = 'bfe-qualification-probe-' + uuid.uuid4().hex[:12]
    group = Path('/sys/fs/cgroup') / name
    namespace = False
    created = False
    try:
        run(['mkdir', str(group)]); created = True
        # Write through privileged Python so shell expansion and a shared
        # existing group's controls cannot enter this capability probe.
        run(['python3', '-c', 'from pathlib import Path; import sys; p=Path(sys.argv[1]); (p/"memory.max").write_text("1073741824"); (p/"memory.swap.max").write_text("0")', str(group)])
        run(['ip', 'netns', 'add', name]); namespace = True
        run(['ip', '-n', name, 'link', 'set', 'lo', 'up'])
        run(['ip', 'netns', 'exec', name, 'tc', 'qdisc', 'add', 'dev', 'lo', 'root', 'netem', 'delay', '50ms', '15ms', 'loss', '5%'])
        print(json.dumps({'passed': True, 'cgroupBytes': int((group / 'memory.max').read_text()), 'networkNamespace': True, 'netem': True}))
    finally:
        if namespace:
            run(['ip', 'netns', 'delete', name])
        if created:
            run(['rmdir', str(group)])


if __name__ == '__main__':
    main()
