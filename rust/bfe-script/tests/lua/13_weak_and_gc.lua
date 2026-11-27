-- Lua 5.0.3 weak tables (__mode = "k", "v", "kv") and string non-weakness.

-- Weak values: array part and hash part drop unreachable tables, keep reachable ones and strings.
local wv = setmetatable({}, { __mode = "v" })
local keep_v = { tag = "alive" }
wv[1] = {}
wv[2] = keep_v
wv.dead = {}
wv.live = keep_v
wv.str = string.char(97, 98, 99) .. string.char(100, 101)
collectgarbage()
print("wv", wv[1] == nil, wv[2] == keep_v, wv.dead == nil, wv.live == keep_v, wv.str)

-- Weak keys: unreachable table keys are removed; reachable keys and string keys survive.
local wk = setmetatable({}, { __mode = "k" })
local keep_k = {}
wk[{}] = 100
wk[keep_k] = 200
wk[string.char(107, 101, 121)] = 300
collectgarbage()
print("wk", wk[keep_k], wk.key)
local wk_count = 0
for _ in pairs(wk) do wk_count = wk_count + 1 end
print("wk_count", wk_count)

-- A weak key table is empty once its dead table key has been collected.
local wk2 = setmetatable({}, { __mode = "k" })
wk2[{}] = 1
collectgarbage()
print("wk2_empty", next(wk2) == nil)

-- Weak keys and values ("kv"): entry cleared if either key or value is unreachable.
local wkv = setmetatable({}, { __mode = "kv" })
local k1, k2, v1 = {}, {}, {}
wkv[k1] = v1
wkv[k2] = {}
wkv[{}] = v1
collectgarbage()
local wkv_count = 0
for _ in pairs(wkv) do wkv_count = wkv_count + 1 end
print("wkv", wkv[k1] == v1, wkv[k2] == nil, wkv_count)
