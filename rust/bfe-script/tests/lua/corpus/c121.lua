-- Generated differential program 121 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "a )%(a_[", "^%s*(.-)%s*$", 3))
  print(pcall(string.find, "by_(]]z%%(([)%", "[%a_][%w_]*", -4))
  print(pcall(string.find, "__%. z", "[%.%-]", -4))
  print(pcall(string.find, "yxc[_x", "^(%w+)", -4))
  print(pcall(string.find, "0]", "%x+", 3))
  print(pcall(string.find, ",%91,%(00.(1-", "[^%s]+"))
  print(pcall(string.gsub, "]0x,%9,(ac1 ", "[%a_][%w_]*", function() return 7 end, 1))
  print(pcall(string.gsub, "%c", "[%.%-]", function(c) return string.upper(c) end))
  print(pcall(string.gsub, "y[c-[_.b[)z(1z", "(b+)(c*)", function() return false end))
  do local out = {} for a, b in string.gfind(", x ya", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- format
do
  print(pcall(string.format, "[%#-14.14E]", -397))
  print(pcall(string.format, "[%q]", "line1\nline2"))
  print(pcall(string.format, "[%.13f]", 97e-25))
  print(pcall(string.format, "[%# 15u]", 2147483647))
  print(pcall(string.format, "[%15o]", 0))
  print(pcall(string.format, "[%015e]", 348.87816))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
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
  local b = setmetatable({v = 2}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "ann"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- trig
do
  print('atan', math.atan(10))
  print('sin', math.sin(1))
  print('log10', math.log10(2))
  print('cos', math.cos(0.5))
  print('atan2', math.atan2(0.5, 1))
  print('pi', math.pi, math.pi * 2)
end
