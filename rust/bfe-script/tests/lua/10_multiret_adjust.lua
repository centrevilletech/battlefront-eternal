-- Test multi-return stack adjustment across CALL, TAILCALL, SETLISTO, and TFORLOOP.

local function fmt6(a, b, c, d, e, f)
    return tostring(a) .. "," .. tostring(b) .. "," .. tostring(c)
        .. "," .. tostring(d) .. "," .. tostring(e) .. "," .. tostring(f)
end

-- 1. Short native calls expecting more results than callee pushes.
do
    local a, b, c, d, e, f = collectgarbage()
    print("short_nat0", fmt6(a, b, c, d, e, f))
    local g, h, i, j, k, l = math.abs(-7)
    print("short_nat1", fmt6(g, h, i, j, k, l))
end

-- 2. Short Lua callees expecting more results than callee frame size.
local function tiny0() end
local function tiny1() return 42 end
local function tc_nat0() return collectgarbage() end
local function tc_lua0() return tiny0() end
local function tc_lua1() return tiny1() end

do
    local a, b, c, d, e, f = tiny0()
    print("short_lua0", fmt6(a, b, c, d, e, f))
    local g, h, i, j, k, l = tiny1()
    print("short_lua1", fmt6(g, h, i, j, k, l))
    local m, n, o, p, q, r = tc_nat0()
    print("short_tcnat0", fmt6(m, n, o, p, q, r))
    local s, t, u, v, w, x = tc_lua0()
    print("short_tclua0", fmt6(s, t, u, v, w, x))
    local y1, y2, y3, y4, y5, y6 = tc_lua1()
    print("short_tclua1", fmt6(y1, y2, y3, y4, y5, y6))
end

-- 3. Open table constructors (OP_SETLISTO) with many and zero returns.
local function many12()
    return 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12
end

local function make_open_tables()
    local t1 = { many12() }
    local t2 = { "head", many12() }
    local t3 = { tiny0() }
    local t4 = { unpack({ 10, 20, 30, 40, 50, 60, 70, 80, 90 }) }
    local t5 = { collectgarbage() }
    print("setlisto", table.getn(t1), t1[1], t1[12], table.getn(t2), t2[1], t2[13], table.getn(t3), table.getn(t4), t4[9], table.getn(t5))
end
make_open_tables()

-- 4. Generic for loops (OP_TFORLOOP) with > 3 loop variables.
do
    local step = 0
    local function iter6(s, v)
        step = step + 1
        if step > 2 then return nil end
        return step, step * 10, step * 100, "d" .. step, "e" .. step, "f" .. step
    end
    for a, b, c, d, e, f in iter6, nil, nil do
        print("tfor6", fmt6(a, b, c, d, e, f))
    end
end

-- Generic for loop where iterator returns fewer values than loop variables.
do
    local step = 0
    local function iter_short(s, v)
        step = step + 1
        if step > 2 then return nil end
        return step, step + 10
    end
    for a, b, c, d, e, f in iter_short, nil, nil do
        print("tfor_short", fmt6(a, b, c, d, e, f))
    end
end

-- Generic for loop where iterator tail-calls native unpack returning 6 values.
do
    local rows = {
        { 1, "a", "b", "c", "d", "e" },
        { 2, "u", "v", "w", "x", "y" },
    }
    local idx = 0
    local function iter_tc()
        idx = idx + 1
        if rows[idx] then
            return unpack(rows[idx])
        end
    end
    for a, b, c, d, e, f in iter_tc do
        print("tfor_tc", fmt6(a, b, c, d, e, f))
    end
end

-- Generic for loop with native iterator (string.gfind) returning 6 captures.
for a, b, c, d, e, f in string.gfind("123456abcdef", "(.)(.)(.)(.)(.)(.)") do
    print("tfor_gfind6", fmt6(a, b, c, d, e, f))
end

-- 5. Chained open calls into varargs and __call metamethod.
local function count_args(...)
    return arg.n, arg[1], arg[arg.n]
end
print("open_chain", count_args(many12()))
print("open_zero", count_args(tiny0()))
print("open_unpack", count_args(unpack({ 7, 8, 9, 10, 11, 12, 13, 14 })))

local callable_short = setmetatable({}, {
    __call = function(self) return 99 end
})
local callable_long = setmetatable({}, {
    __call = function(self) return 1, 2, 3, 4, 5, 6, 7, 8 end
})
do
    local a, b, c, d, e, f = callable_short()
    print("call_mm_short", fmt6(a, b, c, d, e, f))
    local t = { callable_long() }
    print("call_mm_long", table.getn(t), t[1], t[8])
end
