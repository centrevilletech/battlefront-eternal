# Node simulation host

`npm run host:prepare` assembles the pinned server, engine and updater in
`.host-runtime`. Release bundles carry the Rust WASM world without a native
addon, so packaging requires no Rust compiler. The current addon has no native
world or publication operations and stays out of these bundles.
`npm run host:addon` builds it separately for development on Linux x64 with
glibc and the pinned Rust toolchain.

A build service may stage an exact `git archive` using
`BFE_HOST_SERVER_ARCHIVE`, whose embedded revision must match the server pin.
`BFE_HOST_ENGINE_REVISION` carries the source revision when Git worktree metadata
is unavailable; the content-derived build identity still covers the engine.

The addon supplies owned Context lifecycle and checked store operations over
caller-owned numeric arenas. `src/sim/native-backend.js` binds those arenas to
the generated block readers and UTF-16 string identities. A short output reply
is retained until capacity is supplied, so retry does not execute the command
twice. Admission can grow buffers; sealed operation paths reuse their storage.
World and publication operations remain unavailable. Its handshake reports `nativeWorld: false` and
`nativePublication: false`. Battles use one Rust WASM instance each. Native
world stepping and publication require the shared instance-owned world API.

Each bundle records its format, server adapter ABI, store ABI, addon ABI,
complete generated schema fingerprint, engine build identity, exact engine
and server revisions, target tuple and minimum Node-API version. SHA256 hashes
cover the executable payload and dependencies. The server validates the bundle
and the running adapter before importing gameplay. An advertised addon that
fails loading or reports a different handshake stops startup. Removing a
packaged engine's manifest also stops startup. Source checkouts remain usable
without a bundle; they do not establish packaged-pair qualification.

`BlockBridge` uses the generated record decoders over `BlockView` fields
`words`, `base`, `length` and `generation`. WASM resolves byte pointers and
refreshes views after memory growth; native operations use independent typed
arenas. Views are borrowed until the next operation, including reentry.
Generated row loops contain no backend selection or native calls. Existing
WASM phase exports retain their memory bindings until native world operations
replace them. Native addresses never become JavaScript numbers.

`NativeHost` registers environment cleanup and keeps weak owner references.
Explicit close, garbage collection and worker teardown converge on take-once
Context release. The diagnostic addon tests caller-owned arenas that survive
close and can transfer between workers. Its copy fixture validates the generated
header and checked row/heap extents in Rust before writing output; it rejects
shared backing buffers. The copy fixture is absent from ordinary addon builds.
Record-specific span validation belongs to the operation consuming the record.

The CI Rust job builds the diagnostic addon and runs handshake, malformed
block, bit preservation, capacity, panic, close, GC and worker termination
fixtures with Node. Diagnostic resource counters are omitted from packaged
addons. Node-API compatibility alone does not qualify a different libc,
operating system, architecture or embedded Electron runtime.
