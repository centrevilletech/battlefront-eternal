# Actual Theed match gate

Run a full authoritative Theed match (`swbf1/nab2:rep:conquest`) followed by a
rotation to a distinct second supported mission (`swbf1/tat1:rep:conquest` or
`swbf1/bes1:rep:conquest`) using `bfeserver` and `bfe-client`. Both binaries
are built from the same workspace and agree on the required engine and network
contract. Use owned game assets from a prepared content directory; do not commit
them.

`BFE_TEST_MATCH_SECONDS` bounds observation to 60 to 3600 seconds (default 1800).
The gate uses two connected clients and the shared authoritative `bfeserver`
world. It observes ticket depletion and command-post changes, requests a normal
player suicide/respawn through the network spawn port, observes authoritative
match end, then requires both clients to reconnect to the next configured map
and side, receive at least 120 advancing authoritative ticks and complete
another normal respawn. Match-end observations survive map rotation.
It does not reduce starting tickets, force ownership, alter victory timers or
accelerate simulation. A timeout is an incomplete/failed gate, not a pass.

Preserve the full log and exact workspace commit SHA. Do not overlap the run
with a CPU benchmark: this check loads full scenes and steps live 60 Hz battles.

This gate complements movement/combat/loss checks (`rust/bfe-server` and
`rust/bfe-transport` integration tests) and `bfe-client --script gate`.
