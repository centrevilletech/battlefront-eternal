#!/usr/bin/env python3
"""Private actual-game Theed match runner. Never changes the public server."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.request


def git(path, *args):
    return subprocess.check_output(['git', '-C', str(path), *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['engine', 'server', 'data', 'dist', 'output', 'playwright']:
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--static-sha', required=True, help='Declared build-source commit; artifact hashes are recorded separately')
    parser.add_argument('--seconds', type=int, default=1800)
    parser.add_argument('--port', type=int, default=19451)
    args = parser.parse_args()
    if not 60 <= args.seconds <= 3600 or not 1024 <= args.port <= 65530:
        parser.error('seconds must be 60–3600 and port 1024–65530')
    for name in ['engine', 'server', 'data', 'dist', 'playwright']:
        setattr(args, name, getattr(args, name).resolve(strict=True))
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'startedUTC': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
              'engine': git(args.engine, 'rev-parse', 'HEAD'),
              'harness': git(Path(__file__).resolve().parent.parent, 'rev-parse', 'HEAD'),
              'server': git(args.server, 'rev-parse', 'HEAD'), 'declaredStaticSource': args.static_sha,
              'cpuOnly': True, 'impairment': None, 'publicServerTouched': False,
              'rotation': ['swbf1/nab2:rep:conquest', 'swbf1/bes2:rep:conquest'],
              'seconds': args.seconds, 'passed': False}
    for label, path in [('engine', args.engine), ('server', args.server)]:
        report[label + 'Dirty'] = git(path, 'status', '--porcelain', '--untracked-files=no')
        if report[label + 'Dirty']:
            raise RuntimeError(f'{label} tracked source must be clean')
    difference = git(args.engine, 'diff', '--name-only', args.static_sha, 'HEAD', '--', 'src', 'electron')
    if difference:
        raise RuntimeError('Declared static source differs from engine runtime: ' + difference)
    report['staticArtifactSHA256'] = {str(p.relative_to(args.dist)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(args.dist.rglob('*')) if p.is_file()}
    state = args.output / 'state'
    state.mkdir()
    (state / 'config.json').write_text(json.dumps({'name': 'Private Theed acceptance',
        'engine': str(args.engine), 'data': str(args.data), 'rotation': report['rotation'],
        'onlineNoticeAccepted': False, 'maxPlayers': 8,
        'gamePort': args.port, 'discoveryPort': args.port + 1}))
    http_script = args.output / 'static.mjs'
    http_script.write_text('''import http from 'node:http';import {createReadStream,statSync} from 'node:fs';import path from 'node:path';
const roots=ROOTS,mime={'.js':'text/javascript','.html':'text/html','.json':'application/json','.css':'text/css','.png':'image/png','.jpg':'image/jpeg','.svg':'image/svg+xml','.woff2':'font/woff2'};
http.createServer((req,res)=>{const name=decodeURIComponent(new URL(req.url,'http://localhost').pathname);for(const root of roots){const file=path.resolve(root,'.'+(name==='/'?'/index.html':name));if(!file.startsWith(root+'/'))continue;try{const st=statSync(file);if(!st.isFile())continue;res.writeHead(200,{'Content-Type':mime[path.extname(file)]||'application/octet-stream','Content-Length':st.size,'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'});createReadStream(file).pipe(res);return;}catch{}}res.writeHead(404);res.end();}).listen(PORT,'127.0.0.1');
'''.replace('ROOTS', json.dumps([str(args.dist), str(args.data)])).replace('PORT', str(args.port + 3)))
    children, logs = [], []
    def launch(command, name, env=None, cwd=None):
        log = (args.output / (name + '.log')).open('w')
        logs.append(log)
        child = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                                 env=env, cwd=cwd, start_new_session=True)
        children.append(child)
        return child
    try:
        env = dict(os.environ, BFE_STATE_DIR=str(state), PORT=str(args.port + 2))
        server = launch(['node', str(args.server / 'src/main.js'), '--serve'], 'server', env, args.server)
        launch(['node', str(http_script)], 'http')
        ready_deadline = time.monotonic() + 180
        while True:
            if server.poll() is not None:
                raise RuntimeError('Private server exited; inspect server.log')
            try:
                with urllib.request.urlopen(f'http://127.0.0.1:{args.port}/info', timeout=2) as response:
                    report['info'] = json.load(response)
                break
            except OSError:
                if time.monotonic() >= ready_deadline:
                    raise RuntimeError('Private server readiness timed out')
                time.sleep(0.5)
        env = dict({k: v for k, v in os.environ.items() if not k.startswith('BFE_TEST_')}, PLAYWRIGHT_MODULE=str(args.playwright), BFE_TEST_CPU='1',
                   BFE_TEST_FULL_MATCH='1', BFE_TEST_MATCH_SECONDS=str(args.seconds),
                   BFE_TEST_ADDRESS='127.0.0.1', BFE_TEST_PORT=str(args.port),
                   BFE_TEST_URL=f'http://127.0.0.1:{args.port + 3}')
        env.pop('BFE_TEST_SERVER_ID', None)
        client = launch(['node', str(Path(__file__).resolve().parent.parent / 'tests/multiplayer-browser.mjs')], 'gameplay', env, args.engine)
        report['exitCode'] = client.wait(timeout=args.seconds + 900)
        report['passed'] = report['exitCode'] == 0
        if not report['passed']:
            raise RuntimeError('Full match gate failed; inspect gameplay.log')
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        for child in reversed(children):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
        for child in reversed(children):
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
        for log in logs:
            log.close()
        report['finishedUTC'] = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
