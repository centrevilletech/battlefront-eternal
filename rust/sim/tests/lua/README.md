# Mission Script Host Test Fixtures

These `.lua` files are original test scripts written independently from the
host interface rules for `bfe_sim::battle::script` unit tests. No game data or
retail scripts are included.

Each `.luac` binary is compiled directly from its sibling `.lua` file inside
this directory so the embedded chunk name is the bare filename (for example,
`@01_arena_match.lua` with no directory path):

```sh
cd rust/sim/tests/lua
luac_swbf2 -o <name>.luac <name>.lua
```

`luac_swbf2` is a reference Lua 5.0.3 `luac` built with the 2005 game's binary
chunk header (`lua_Number = float`, `lua_size_t = uint32_t`,
`Instruction = uint32_t`, `MAXSTACK = 128`, `MAXVARS = 120`).
