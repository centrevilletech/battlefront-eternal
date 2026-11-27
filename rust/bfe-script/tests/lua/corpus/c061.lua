-- Generated differential program 061 (generate.py, seed 503).
-- arith
do
  do local v = (2^17)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2^7)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((423.91175) * (-438)) - ((2^2) + (2^-8))) / (-4.707))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (0.7)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((-6) + (-25.733)) + ((68e28) - (2^3))) * (((444) / (-10.157)) * ((-16.16) - (2/3))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (-12.72)
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (2^-11)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = (((8388609) * (3.4e38)) * (((10e7) + (0.8002)) - ((5) * (0.3))))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(168.38106))
  print('ceil', math.ceil(0.933))
  print('abs', math.abs(29e-8))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(9, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(-2.592, 0.7606246, 444.1592), math.max(407, 46e-34))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(1, -2))
  print('pow', 1.5 ^ -2)
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
  local a = setmetatable({v = 1}, mt)
  local b = setmetatable({v = 0}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "bo"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 3, -1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 32 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 18 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- format
do
  print(pcall(string.format, "[%16.3s]", "hello"))
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(pcall(string.format, "[%6c]", 48))
  print(pcall(string.format, "[%6u]", 2147483647))
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%11c]", 126))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
