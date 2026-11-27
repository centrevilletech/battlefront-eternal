-- Generated differential program 216 (generate.py, seed 503).
-- strings
do
  local s = "1eZ1cYb"
  print(string.sub(s, 14, -8), string.sub(s, 14), string.byte(s, -8))
  print(string.sub(s, -3, 15), string.sub(s, -3), string.byte(s, 15))
  print(string.sub(s, 0, -13), string.sub(s, 0), string.byte(s, -13))
  print(string.sub(s, -6, -9), string.sub(s, -6), string.byte(s, -9))
  print(string.sub(s, -5, 7), string.sub(s, -5), string.byte(s, 7))
  print(string.sub(s, 6, 15), string.sub(s, 6), string.byte(s, 15))
  print(string.sub(s, -6, 15), string.sub(s, -6), string.byte(s, 15))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 4))
  print(string.char(50, 56, 80))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
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
  local a = setmetatable({v = 2}, mt)
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
-- format
do
  print(pcall(string.format, "[%g]", 8388609))
  print(pcall(string.format, "[%02x]", -1))
  print(pcall(string.format, "[%-19.6g]", 16777216))
  print(pcall(string.format, "[%d]", 0))
  print(pcall(string.format, "[%-10o]", 123456))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
