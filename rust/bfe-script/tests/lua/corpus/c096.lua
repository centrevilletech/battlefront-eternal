-- Generated differential program 096 (generate.py, seed 503).
-- arith
do
  do local v = (54e8)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((0.59056) / ((593) * ((-9.39) / (33554433))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((484) * (0.0825969628)) * ((940) * (0.3203))) * (705))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (4)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (-427)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(30))
  print('ceil', math.ceil(0.61588000955))
  print('abs', math.abs(9e-7))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(1e-45, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(79e-2, 54e28, 90e10), math.max(2147483647, 1))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(0.75, 17))
  print('pow', 2 ^ 3)
end
-- meta
do
  local mt = {}
  local function val(x) if type(x) == 'table' then return rawget(x, 'v') end return x end
  mt.__add = function(a, b) return 'add(' .. tostring(val(a)) .. ',' .. tostring(val(b)) .. ')' end
  mt.__index = function(t, k) return 'missing ' .. tostring(k) end
  mt.__newindex = function(t, k, v) rawset(t, k, v .. '!') end
  mt.__eq = function(a, b) return rawget(a, 'v') == rawget(b, 'v') end
  mt.__lt = function(a, b) return rawget(a, 'v') < rawget(b, 'v') end
  mt.__le = function(a, b) return rawget(a, 'v') <= rawget(b, 'v') end
  mt.__concat = function(a, b) return 'cat' end
  mt.__unm = function(a) return 'neg' end
  mt.__call = function(self, x) return 'called ' .. tostring(x) end
  local a = setmetatable({v = 3}, mt)
  local b = setmetatable({v = 3}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "cy"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 8 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(5), counter(11)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- coerce
do
  print("  12  " + 0)
  print(0.1 .. "")
  print("5" + "0.25")
  print(123456789 .. "")
  print(-1/0 .. "")
  print(16777217 .. "")
  print("10" + 5)
  print("0x10" + 0)
  print("1 2", tonumber("1 2"))
  print(".", tonumber("."))
  print("inf", tonumber("inf"))
  print("-7.5", tonumber("-7.5"))
  print("1e400", tonumber("1e400"))
  print("12", tonumber("12"))
  print("0x", tonumber("0x"))
  print(9, tonumber("7424", 9))
  print(8, tonumber("5 0", 8))
  print(30, tonumber("1ep05c", 30))
  print(33, tonumber("o5po7", 33))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
