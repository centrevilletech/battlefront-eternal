# Lua execution records

Each line of the decompressed JSON contains a program name, stdout bytes, primary
error, return values, VM state digest and ordinary work count. Numeric returns
are binary32 bits in hexadecimal. Other returns are Lua type names. The order
of names and the order of columns are part of the fixture.

`previous.jsonl.gz` contains the generation that charged two units for some
cached string hits during collection. In `current.jsonl.gz`, exactly the 37
programs named in `cache-hits.txt` use two fewer units each. Only the 2,360 digests
in `current.jsonl.gz` include the collector's pending state. The default crate
tests check both generations and execute the 241 programs whose bytecode is
bundled in this crate.

The other 2,119 invented programs are stored as UTF-8 source in
`sources.jsonl.gz`, one JSON pair of program name and source per line.
To compile all 2,360, use the Lua 5.0.3 compiler configuration in
`docs/scripts.md` and an output directory outside the checkout. From the
repository root, set `LUA50_LUAC` to that compiler and `BFE_LUA_RECORD_PROGRAMS`
to the absolute output directory.

```sh
export BFE_LUA_RECORD_PROGRAMS
python3 rust/bfe-script/tests/lua/records/compile.py "$LUA50_LUAC" "$BFE_LUA_RECORD_PROGRAMS"
cargo test --manifest-path rust/Cargo.toml -p bfe-script --test differential_corpus replay_all_execution_records -- --ignored
```

Setting `BFE_LUA_RECORD_OUTPUT` to an output filename also writes the canonical
uncompressed JSON lines before comparing them. To check that file byte for byte,
run `gzip -dc rust/bfe-script/tests/lua/records/current.jsonl.gz | cmp - "$BFE_LUA_RECORD_OUTPUT"`.
