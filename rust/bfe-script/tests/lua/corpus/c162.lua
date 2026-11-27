-- Generated differential program 162 (generate.py, seed 503).
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
  local b = setmetatable({v = 1}, mt)
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
-- loops
do
  do local n, last = 0, nil for i = 1, -5, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 29 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 10 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- arith
do
  do local v = (16777215)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (4e-21)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((4294967295) + ((-2.690) * (2^-28))) / (-17.444))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((7) + (9)) / ((2/3) + (237.72192))) - ((0.783) / (0.9709891840)))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(0.2))
  print('ceil', math.ceil(582.96209))
  print('abs', math.abs(-25.849))
  print('sqrt', math.sqrt(3))
  do local m = math.mod(33554433, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(48e15, 0.3, 477.49167), math.max(8388609, 2^-3))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(1, -19))
  print('pow', 2 ^ 3)
end
