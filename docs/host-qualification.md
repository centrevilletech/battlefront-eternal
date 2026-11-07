# Host qualification

Run `tools/host-qualification.mjs` against an exact bundle produced by
`npm run host:prepare`. The service driver executes the bundle's server and
validates its pinned engine/server identities before starting a world. Imported
content stays external. Use Node with `--expose-gc` for lifecycle checks.

Every invocation requires `--out=FILE`; results are bounded JSON summaries.
Authenticated synthetic clients run in separate workers, each with its own
production receiver, sender, WASM memory and event loop. Their memory stays
outside the service cgroup. A single shared realm can invalidate all receiver
views when another client grows memory, starving transport keepalives and
confusing client-driver overload with server admission failure. Rejoin creates
a new worker and receiver while retaining the authenticated identity.
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

## Farm acceptance runner

`node tools/host-qualification/runner.mjs --farm=CONFIG --backend=wasm --out=RESULTS --proof`
runs Q1 through Q9's server leg across five configured workers and prints one
page of verdicts. Omit `--proof` for the full duration. Proof changes only the
1,800-second lifecycle and memory windows to 300 seconds. Sequential counts,
stress ticks, authenticated clients, CPU windows and network impairment retain
their acceptance values. `--only=REGEX` selects job IDs for focused diagnosis.
Focused reruns can reuse matching setup receipts with `--reuse-probes`.
After retries, `--summarize --reuse-probes` reads every current verdict without
running payloads again. Overall wall time includes time since the result
directory was created; each job also records its own payload wall time.
Run from a clean committed checkout; prepare the paired bundle from that tree.


The operator JSON is private and stays outside the repository. It supplies:

```json
{
  "slot": "SLOT_WRAPPER",
  "node": "NODE_24_EXECUTABLE",
  "node22": "NODE_22_EXECUTABLE",
  "hosts": [
    {"host": "WORKER_A", "group": "parity"},
    {"host": "WORKER_B", "group": "parity"},
    {"host": "WORKER_C", "group": "parity"},
    {"host": "WORKER_D", "group": "cpu"},
    {"host": "WORKER_E", "group": "cpu"}
  ],
  "engine": "PAIRED_BUNDLE/engine",
  "bundleArchive": "PAIRED_BUNDLE_ARCHIVE",
  "serverArchive": "PINNED_SERVER_SOURCE_ARCHIVE",
  "data": "EXTERNAL_CONTENT",
  "references": "FROZEN_WORLD_HASHES",
  "packetReferences": "FROZEN_F6_PUBLICATIONS",
  "worldCorpus": "FROZEN_WORLD_INPUTS",
  "raw": "FARM_RAW_DIRECTORY",
  "out": "SUMMARY_DIRECTORY",
  "nativeClient": "NATIVE_CLIENT_EXECUTABLE",
  "dist": "MINIFIED_DESKTOP_DIRECTORY",
  "playwright": "PLAYWRIGHT_MODULE",
  "chromium": "CHROMIUM_EXECUTABLE",
  "electron": "ELECTRON_44_EXECUTABLE",
  "display": "X11_DISPLAY",
  "sync": "EXTRA_INPUTS"
}
```

Paths must be visible under identical names on the workers. If the slot
wrapper omits dependency directories, supply an uncompressed `bundleArchive`
whose top-level directory matches the bundle directory. Under a worker-local
lock, jobs compare its manifest and hashed files, and extract only incomplete
or changed bundles. A complete bundle is never rewritten beneath a running
world. Keep the archive immutable until every job finishes. The wrapper must
mirror the source, bundle and small job configuration, and return only the
summary directory. Raw transcripts and logs stay in `raw` on their workers.
The runner opens at most eight slot wrappers concurrently, and each matrix
shard uses four child processes. CPU roles start after other jobs finish.
The operator should reserve that worker for the CPU window; taskset affinity
alone cannot prevent unrelated jobs from competing for the core.

Before scheduling, probe **every** configured worker through its slot wrapper:
passwordless `sudo -n true`, `ip`, `tc`, cgroup v2, and `sudo -n unshare --net true`.
Then run `cgroup-selftest.py` through a slot to prove creation, OOM and teardown.
Q4 uses the fifth worker; change the operator's worker order if privilege is
available elsewhere. Q6 needs a GPU worker, an X11 display and the native client
binary. No host interface is shaped. Build the binary and minified desktop
through the farm build wrapper; keep the external content out of both artifacts.

| Job | Required observations |
| --- | --- |
| Q1-1..3 | Disjoint standing manifest shards; 61 cases and 189,000 ticks total, three exact frozen hashes, backend factory injection |
| Q2 | All frozen publication JSONL cases, exact bytes and output order, ACK/edge/reliable/fault fields; boundary and baseline fixtures |
| Q3-map | Four maps, 24 sequential/rejoin battles each, two-owner isolation, 12,000 stress ticks, wall soak, collection, double close, worker termination |
| Q4 | 32 clients in one battle, then 64 each in one/two/four battles, real UDP, rotation, fresh-receiver reconnect and held ACKs inside the real 1 GiB service cgroup |
| Q5 | Four maps, fresh pinned process per role, 3,000 warm and 6,000 measured ticks; WASM/WASM proof, native/WASM/WASM/native acceptance |
| Q6-map | Authenticated service, four clients, 60 seconds, 50 ± 15 ms each direction, 5% loss, 3-second blackout, reconnect; two actual native executables and two Chromium pages for two 30-second epochs |
| Q7 | Actual Electron 44 embedded host receives authenticated clients, clean shutdown; paired loader checks on Node 22 and 24 |
| Q8 | Disposable bundle format/schema/build/ABI/artifact failures before readiness; valid-bundle control; native target cases required when selected |
| Q9 | Paired server suite against the exact source tree (engine-build digest checked), engine content, zero skips; final exact-tree gate remains a separate operator invocation |

Run the repository's exact-tree gate once on the final clean tree after merging
current integration and regenerating generated outputs through the farm build
wrapper with copy-back. This gate supplies the engine content suite, Rust checks,
bridges and frozen matrix. Do not run it per iteration. It must pass alongside
all selected qualification jobs before reporting acceptance. The CPU results
here describe the server; the target laptop's performance decision is separate.

Every job writes `JOB.json` with backend, package identity, wall time, pass/fail
and bounded evidence. A failed child or missing verdict fails the runner even
when an older verdict exists. Native unavailable fields stay unavailable. WASM
GC and OS reclamation do not stand in for native resource-drop counters.

### Native adapter requirements

The existing `createQualificationBackend` contract also supplies
`simulationFactory` for Q1's bridge route and Q2's publication route.
Capture factories also receive `sourceRoot`: resolve their bridge classes from
that graph while loading the declared artifact from `engine`, so source
entities never meet a second copy of the type registry. The
preload lives only in child tools; the application has no qualification switch.
Q1 additionally requires `replayMatrix({matrix,references,corpus,raw})`, returning
`{passed:true,...}` after the direct native replay route has compared all rows.
No native run passes by merely loading an addon or running the WASM factory.

A battle opened with `measure:true` supplies `resetMeasurements()` and
`measurements()`: completed `steps`, `publications`, separate `stepMicros` and
`publishMicros`, packet bytes, boundary calls/words/bytes and actual allocation
counts. The WASM proof counts the simulation allocator across the complete measured
tick loop; JavaScript and client-presentation allocations remain outside that
scope. Process/thread CPU and RSS are measured. Native diagnostics must include
reserved/used and created/released/live counts. Q8 calls adapter export
`qualifyBundleMismatches({engine,raw})` for corrupted/missing addon, N-API/glibc
floor and unsupported-target selection verdicts. `module` and `worldWorker` in
the operator JSON provide the adapter and worker protocol paths. Native support
must be in the exact paired package before these jobs can qualify it.
