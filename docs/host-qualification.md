# Host qualification

Run `tools/host-qualification.mjs` against an exact bundle produced by
`npm run host:prepare`. The service driver executes the bundle's server and
validates its pinned engine/server identities before starting a world. Imported
content stays external. Use Node with `--expose-gc` for lifecycle checks.

Every invocation requires `--out=FILE`; results are bounded JSON summaries.
`--backend=wasm` exercises the bundled WASM host. `--backend=native` requires explicit
harness adapters with native world/publication capabilities. It fails if those
capabilities are absent. These inputs never select the shipped application's
backend. A foundation Context test does not qualify a native battle.

```sh
node --expose-gc tools/host-qualification.mjs addon --backend=wasm --addon=.host-addon/bfe-node.node --out=addon.json
node --expose-gc tools/host-qualification.mjs lifecycle --backend=wasm --engine=.host-runtime/engine --data=CONTENT --map=nab2 --side=rep --seconds=1800 --battles=24 --ticks=120 --clients=4 --out=lifecycle.json
node tools/host-qualification.mjs service --backend=wasm --engine=.host-runtime/engine --data=CONTENT --clients=64 --concurrency=1 --seconds=1800 --rotate=1 --out=memory.json
node tools/host-qualification.mjs network --backend=wasm --engine=.host-runtime/engine --data=CONTENT --clients=4 --seconds=60 --blackout-seconds=3 --shape-up-kbps=1000 --shape-down-kbps=2000 --out=network.json
```

Repeat lifecycle for `nab2/rep`, `tat1/rep`, `bes1/rep`, and `hot1/imp`.
`--ticks` controls each sequential/rejoin window. `--stress-ticks=12000`
(default) runs an additional tick-driven battle. The long window records wall
seconds, real ticks and battle completion; a naturally finished battle is visible
in the result. CI can use `--seconds=1 --battles=2 --ticks=10 --stress-ticks=60 --clients=2`.
Run service cases with `--concurrency=2` and `4` under the same memory ceiling.
A capacity failure is a failed qualification result, including OOM evidence.
It does not authorize concurrent battle admission.

The service payload alone runs in a disposable 1 GiB cgroup v2; authenticated
clients and the supervisor stay outside it. Linux cgroup control, passwordless
sudo and `ip`/`tc` are required for farm runs. No host interface is shaped:
network runs create a disposable namespace. Every client direction receives
50 ± 15 ms delay and 5% kernel loss, followed by a bounded total blackout and
recovery. Fresh reconnect endpoints receive rebuilt shaping; reconnect handshakes
run before that shaping is installed. Rotation retains the server identity and
UDP endpoint. Slow memory clients retain transport but withhold snapshot ACKs.

Results include cgroup current/peak/limit/events and teardown, RSS/PSS, JS
heap/external/array buffers, WASM linear memory, observed queue high water,
publication/session counters, and kernel shaping statistics. Native capacity
fields are explicitly unavailable for the WASM backend. Samples are observations,
not a proof that an unsampled transient queue never grew.

`workers` runs the real battle worker termination cases independently; lifecycle
also includes them. The step-loop case records completed ticks before termination;
only the diagnostic addon proves termination requested inside an active native
import. These are separate observations.

The diagnostic addon is built with `node tools/build-node-host.mjs --fixtures`.
The default addon path is `.host-addon/bfe-node.node`, matching that build.
The production bundle omits the probe feature. Its allocation-free owner counters cover early acquisition errors,
double close, collection, and forced worker termination during a real active
Context import. A killed cgroup reports OS reclamation, never fabricated Rust
Drop counts. WASM lifecycle checks require collection of instance/memory weak
references and do not claim native owner drops.

A future native lifecycle adapter supplied with `--backend-module=MODULE` exports
`createQualificationBackend({engine,manifest})`: capability flags, acquisition
stage names, process-wide `counts()` with created/released/live totals, and `open(config)`. Its battle contract is the one exercised in
`tools/host-qualification/lifecycle.mjs`; the WASM adapter documents the methods.
Native service qualification accepts `--world-worker=MODULE`, a harness worker
exporting `qualificationBackend = 'native'` and the production worker protocol,
including diagnostics with actual used/reserved and created/released/live counts.
The bundle itself must support that native backend before it can qualify.

The supervisor also has privileged farm self-tests:
`python3 tools/host-qualification/cgroup-selftest.py --backend=wasm --out=cgroup-check.json`.
They verify normal exit, a payload ignoring SIGTERM, and OOM reclamation.

CI runs content-free argument/telemetry/queue self-tests and diagnostic addon
lifecycle tests. Content soaks, privileged cgroup and network cases are farm
commands. Preserve the exact bundle and harness revision throughout a long run;
keep raw data on the farm and retain only summaries for review.
