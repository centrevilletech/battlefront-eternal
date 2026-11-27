-- Math library, errors, pcall/xpcall, assert, unpack, and function environments.
print("math1",
    math.abs(-3.5),
    math.floor(2.9),
    math.floor(-2.1),
    math.ceil(2.1),
    math.ceil(-2.9),
    math.sqrt(16),
    math.mod(10, 3),
    math.mod(-10, 3)
)
print("math2",
    math.min(5, -2, 8, 1),
    math.max(5, -2, 8, 1),
    math.pow(3, 4),
    math.floor(math.deg(math.pi) + 0.5),
    math.floor(math.rad(180) * 1000 + 0.5)
)
local m, e = math.frexp(12)
print("frexp_ldexp", m, e, math.ldexp(m, e))
print("trig",
    math.floor(math.sin(0) * 1000 + 0.5),
    math.floor(math.cos(0) * 1000 + 0.5),
    math.floor(math.tan(0) * 1000 + 0.5),
    math.floor(math.sin(math.pi / 2) * 1000 + 0.5),
    math.floor(math.exp(0) * 1000 + 0.5),
    math.floor(math.log(1) * 1000 + 0.5),
    math.floor(math.log10(1000) * 1000 + 0.5),
    math.floor(math.atan2(1, 1) * 1000 + 0.5)
)

-- assert (Lua 5.0 returns only first arg)
local a1, a2 = assert(7, "extra")
print("assert_ok", a1, a2)
local ok_a, err_a = pcall(assert, false, "boom")
print("assert_fail", ok_a, err_a)

-- error with non-string payload and level 0
local err_tbl = { code = 42 }
local ok_t, got_t = pcall(function() error(err_tbl) end)
print("err_table", ok_t, got_t == err_tbl, got_t.code)
local ok_s, got_s = pcall(function() error("plain", 0) end)
print("err_lvl0", ok_s, got_s)

-- xpcall
local ok_x, res_x = xpcall(
    function() error("oops", 0) end,
    function(e) return "handled:" .. tostring(e) end
)
print("xpcall", ok_x, res_x)

-- unpack and type
local u1, u2, u3 = unpack({ 10, 20, 30 })
print("unpack", u1, u2, u3, type(nil), type(true), type(1), type("s"), type({}), type(print))

-- getfenv / setfenv
local env = { x = 99 }
local function read_x()
    return x
end
setfenv(read_x, env)
print("fenv", read_x(), getfenv(read_x) == env)
