# World replay diagnostics

The standalone world-hash matrix and corpus executables have been retired.
Phase replay fixtures remain in `rust/sim-replay` and the simulation tests;
see [sim-replay.md](sim-replay.md) for their commands.

`tools/world-hash-matrix.txt` remains as the battle list used by admission
coverage. It does not invoke a separate world runner.

Exercise complete battles through the shipped `bfe-client` and `bfeserver`
pipeline with prepared content. See [native-client.md](native-client.md) for
client modes and [host-qualification.md](host-qualification.md) for server
lifecycle and network acceptance checks.
