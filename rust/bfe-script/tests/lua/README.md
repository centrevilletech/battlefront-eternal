# Lua 5.0 Test Fixtures

These `.lua` source files are original test scripts written for `bfe-script`.
No game data or retail scripts are included.

Each `.luac` file was compiled from the matching `.lua` file using a reference
Lua 5.0.3 `luac` built with the 2005 game's binary chunk settings:

- `lua_Number = float` (4-byte IEEE-754 single precision)
- `lua_size_t = uint32_t` (4-byte little-endian string lengths)
- `Instruction = uint32_t` (4-byte little-endian instructions, 6/8/9/9 field widths)
- `MAXSTACK = 128` (`RK(x)` constant offset = 128) and `MAXVARS = 120`

Each `.expected.txt` file records the exact stdout produced by running the
corresponding `.luac` chunk in that same reference Lua 5.0.3 interpreter, and
`tests/vm_tests.rs` verifies byte-for-byte agreement with `bfe-script`.

`corpus/` holds 241 more standalone programs: `corpus/generate.py` writes 240
from a fixed seed and then fixed cases (`cNNN.lua`), and `corpus/pack.py LUAC LUA`
compiles each with the reference `luac`, runs it in the reference `lua` under
`LC_ALL=C`, and packs the chunks with that stdout and a hash of each source into
`corpus/corpus.bin`. `generate.py --check` and `pack.py --check LUAC LUA` compare
what they would write with the committed files byte for byte.
`tests/differential_corpus.rs` checks that the bundle holds exactly the committed
sources, each as it was packed, and that `bfe-script` prints the same bytes for
every program.

To rebuild a fixture after editing its source, compile it from this folder under
its bare file name (`luac -o NAME.luac NAME.lua`) so the chunk name carries no
path, and record `NAME.expected.txt` with the reference `lua`.
