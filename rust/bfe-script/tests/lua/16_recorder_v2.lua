-- Synthetic mission script exercising ImportProfile ("pc-fresh"), callable bindings,
-- indirect table calls, audio stream handles, unresolved queries, and lossless values.
function ScriptInit()
    -- Guarded here, but another script of the archive calls it unguarded, so
    -- the recorder derives it as a host function and the guarded call runs.
    if GuardedHostCall then
        GuardedHostCall("example_pool", 16)
    end
    -- Caller-provided optional API guarded by `if`.
    if OptionalAPI then
        OptionalAPI(1)
    end
    -- Only ever called behind a test: stays nil and is listed as probed.
    if AbsentOptionalHook then
        AbsentOptionalHook(99)
    end
    -- Function stored in table and called indirectly.
    local f = { TableStoredAPI }
    f[1](2)

    -- Queries and a global answered by values the preparation step supplies.
    local plat = QueryPlatform()
    local saved = QuerySavedSetup()
    RecordPlatform(gPlatform, plat, saved)

    -- Sequential deterministic audio stream handles propagated to append calls.
    local s1 = OpenAudioStream("stream_a.lvl", 1)
    local s2 = OpenAudioStream("stream_b.lvl", 1)
    AudioStreamAppendSegments("seg_a", s1)
    AudioStreamAppendSegments("seg_b", s2)

    -- Value-returning host call not answered by profile -> surfaced in unresolved_queries.
    local q = CreateTimer("timer_a")
    RecordTimerResult(q)

    -- Lossless encoding: mixed key collisions, -0.0, raw non-UTF8 bytes vs UTF-8,
    -- and ordinary table containing the reserved "$bfe" key.
    local mixed = { [1] = "n", ["1"] = "s", [true] = "b", ["true"] = "s2" }
    local fake_marker = { ["$bfe"] = "bytes", hex = "e9" }
    Record({ { mixed } }, -1 * 0, string.char(233), string.char(195, 169), fake_marker, 0 / 0, 1 / 0, -1 / 0)
end
