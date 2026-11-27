-- Test GC rooting across native stdlib calls and hostile callbacks.

-- 1. print() must keep the initial _G.tostring alive even if __tostring replaces _G.tostring and runs GC.
do
    local orig_tostring = tostring
    local obj = setmetatable({}, {
        __tostring = function()
            _G.tostring = function() return "REPLACED" end
            collectgarbage()
            collectgarbage()
            return "FIRST"
        end,
    })
    -- Replace _G.tostring with a fresh closure that has no other reference once overwritten.
    _G.tostring = function(x) return orig_tostring(x) end
    print(obj, "SECOND")
    _G.tostring = orig_tostring
end

-- 2. table.sort() must root the pivot and compared values even if comparator wipes table & runs GC.
do
    local t = {}
    for i = 1, 10 do
        t[i] = { k = 11 - i, tag = "item_" .. tostring(i) }
    end
    local steps = 0
    table.sort(t, function(a, b)
        steps = steps + 1
        if steps == 3 then
            -- Drop references from another slot temporarily and force GC.
            local tmp = {}
            for j = 1, 20 do tmp[j] = "junk_" .. j end
            collectgarbage()
        end
        return a.k < b.k
    end)
    local out = {}
    for i = 1, 10 do out[i] = tostring(t[i].k) end
    print("sort_gc", table.concat(out, ","))
end

-- 3. table.foreach() must root current key if callback deletes it and triggers GC.
do
    local t = {}
    t["k_" .. tostring(1)] = 10
    t["k_" .. tostring(2)] = 20
    t["k_" .. tostring(3)] = 30
    local visited = 0
    table.foreach(t, function(k, v)
        visited = visited + v
        t[k] = nil
        collectgarbage()
    end)
    print("foreach_gc", visited)
end

-- 4. string.find() with numeric arguments and multiple captures.
do
    local a, b = string.find(1234567, 456)
    local s1, s2, c1, c2, c3, c4 = string.find("alpha-beta-gamma-delta", "(%a+)%-(%a+)%-(%a+)%-(%a+)")
    print("find_num", a, b, s1, s2, c1, c2, c3, c4)
end

-- 5. string.gsub() with function replacement that runs GC on every match.
do
    local res, n = string.gsub("one two three", "(%a+)", function(w)
        collectgarbage()
        return string.upper(w)
    end)
    print("gsub_gc", res, n)
end
