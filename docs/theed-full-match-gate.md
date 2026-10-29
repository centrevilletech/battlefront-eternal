# Actual Theed match gate

Run the existing `tests/multiplayer-browser.mjs` with
`BFE_TEST_FULL_MATCH=1`. Configure the local server rotation with Theed first
(`swbf1/nab2:rep:conquest`) and a distinct second supported mission. The static
client build, harness and server must agree on the required engine contract.
Use owned game assets through the existing asset mount; do not commit them.

`BFE_TEST_MATCH_SECONDS` bounds observation to 60 to 3600 seconds (default 1800).
The fixture uses two actual clients and the ordinary shared authoritative game.
It observes ticket depletion and command-post changes, requests a normal player
suicide/respawn through the network spawn port, observes authoritative match end,
then requires both clients to reconnect to the next configured map and side,
receive at least 120 advancing authoritative ticks and complete another normal
respawn. Match-end observations survive page rotation.
It does not reduce starting tickets, force ownership, alter victory timers or
accelerate simulation. A timeout is an incomplete/failed gate, not a pass.

Preserve the full log and exact engine, static-build, harness and server SHAs.
Record whether `BFE_TEST_CPU=1` disables GPU rendering and any network impairment
profile. Do not overlap the run with a CPU benchmark: this fixture loads two
full scenes.

This gate complements movement/combat/loss checks; it does not establish
rendering in the packaged desktop app on real hardware, cross-machine
interoperability, carrier connectivity or 64-player performance.

Use `python3 tools/multiplayer-match.py --help` for the local runner. Provide
clean engine/server checkouts, owned data, an existing desktop static build and
its declared source SHA, Playwright module path and a new output directory. The
runner checks source equivalence, hashes static artifacts, starts local
server and HTTP processes and records revision/contract/log evidence in
`report.json`. It terminates only processes it started. Default ports are
19451 to 19454.
