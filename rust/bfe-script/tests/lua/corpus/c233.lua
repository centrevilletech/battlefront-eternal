-- Generated differential program 233 (generate.py, seed 503).
-- globals
do
  gx = 44
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
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
  local a = setmetatable({v = 4}, mt)
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
-- arith
do
  do local v = (8)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((16777215) + (0.34154665)) * ((0.17198395) - (-30.807))) / (((4294967295) + (2^7)) * ((43e-22) / (860.95884))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((12e-29) / (16777215)) * (0.06))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((-536) + (2)) - ((2^5) - (8e-34))) + ((94e-35) * ((2^-4) + (16777216))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((551.2725) * (8)) + ((2147483647) + (2^-22))) + (((1/3) * (769)) + ((850.80124) / (2^11))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (1)
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(-558))
  print('ceil', math.ceil(2^-1))
  print('abs', math.abs(2^-11))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(-13.520, -3) if m == m then print('mod', m) end end
  print('minmax', math.min(2^-12, 0.757989107, 2^-20), math.max(2^-7, 1/3))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(-3, -14))
  print('pow', 1.5 ^ -2)
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(4))
  local a, b, c = pass(0, 1)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
