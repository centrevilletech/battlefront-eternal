# Node simulation host

`npm run host:prepare` assembles the pinned server, engine and updater in
`.host-runtime`. On Linux x64 with glibc it also builds `rust/bfe-node` using
the pinned Rust toolchain. `npm run host:addon` stages that addon separately.
A build service may stage an exact `git archive` using
`BFE_HOST_SERVER_ARCHIVE`, whose embedded revision must match the server pin.
`BFE_HOST_ENGINE_REVISION` carries the source revision when Git worktree metadata
is unavailable; the content-derived build identity still covers the engine.
Other targets carry the Rust WASM world without an advertised addon.

The addon currently supplies owned Context lifecycle and a checked numeric
block boundary. Its handshake reports `nativeWorld: false` and
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
Context release. Caller-owned output arenas survive close and can transfer
between workers. Numeric block validation checks the generated header and
checked row/heap extents before writing output. Record-specific span validation
belongs to the operation that consumes that record.

The CI Rust job builds the diagnostic addon and runs handshake, malformed
block, bit preservation, capacity, panic, close, GC and worker termination
fixtures with Node. Diagnostic resource counters are omitted from packaged
addons. Node-API compatibility alone does not qualify a different libc,
operating system, architecture or embedded Electron runtime.
