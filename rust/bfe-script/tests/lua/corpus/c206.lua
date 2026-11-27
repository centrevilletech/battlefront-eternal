-- Generated differential program 206 (generate.py, seed 503).
-- arith
do
  do local v = ((0.1) * (((16777216) - (30e-14)) + (985.40312)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((4294967295) + ((9) + (2^17)))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((2) * (0.03304055)) - ((0.817300656) * (3))) * (0.0687581))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((2^15) + (0.95164)) / ((0.3) * (2^3))) - (16777216))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (2)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((2^-12) - (2/3))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (((2/3) + ((93e18) / (8))) / (((-581) / (2^10)) + ((4294967295) + (780))))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(0.0267))
  print('ceil', math.ceil(2147483647))
  print('abs', math.abs(8))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(1e-45, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.642721030, 11e-40, 5), math.max(2^-8, -22.211))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(-3, 14))
  print('pow', 10 ^ 10)
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
  local a = setmetatable({v = 0}, mt)
  local b = setmetatable({v = 4}, mt)
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
