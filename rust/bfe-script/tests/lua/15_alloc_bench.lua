-- Microbenchmark helpers for warmed tick-path host allocation checks.
function bench_add(x)
    return x + 1
end

local function inc(x)
    return x + 1
end

function bench_pcall(x)
    return pcall(inc, x)
end

local sort_tbl = {8, 3, 7, 1, 5, 2, 6, 4}
local function cmp(a, b)
    return a < b
end

function bench_sort8()
    sort_tbl[1] = 8
    sort_tbl[2] = 3
    sort_tbl[3] = 7
    sort_tbl[4] = 1
    sort_tbl[5] = 5
    sort_tbl[6] = 2
    sort_tbl[7] = 6
    sort_tbl[8] = 4
    table.sort(sort_tbl, cmp)
    return sort_tbl[1]
end

function bench_gfind(s)
    local n = 0
    for _ in string.gfind(s, ".") do
        n = n + 1
    end
    return n
end

function bench_concat(n)
    return "level.x." .. n
end

-- A listener that builds a capturing closure on every call (two open upvalues
-- over its own locals, and one inherited upvalue).
local hits = 0
function bench_capture(n)
    local base = n
    local scale = 2
    local add = function(x)
        hits = hits + 1
        return x * scale + base
    end
    return add(1)
end
