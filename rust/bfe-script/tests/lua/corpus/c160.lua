-- Generated differential program 160 (generate.py, seed 503).
-- strings
do
  local s = " 0X"
  print(string.sub(s, 12, 7), string.sub(s, 12), string.byte(s, 7))
  print(string.sub(s, 11, -8), string.sub(s, 11), string.byte(s, -8))
  print(string.sub(s, -7, 5), string.sub(s, -7), string.byte(s, 5))
  print(string.sub(s, 7, 8), string.sub(s, 7), string.byte(s, 8))
  print(string.sub(s, -11, -10), string.sub(s, -11), string.byte(s, -10))
  print(string.sub(s, -11, 15), string.sub(s, -11), string.byte(s, 15))
  print(string.sub(s, -3, 15), string.sub(s, -3), string.byte(s, 15))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 5))
  print(string.char(80))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- patterns
do
  print(pcall(string.find, "9[9 zc]x_.)", "^(%w+)", -4))
  print(pcall(string.find, "01%0- ", "[%.%-]", -4))
  print(pcall(string.find, "]y .1z(1[", "c-", 3))
  print(pcall(string.find, "(9-a(c99c_", "(%a)(%d)", -4))
  print(pcall(string.find, "%)b][", "%f[%w]%w+", 20))
  print(pcall(string.find, "-(__y0c1a.-,", "(a*)", 1))
  print(pcall(string.gsub, "b] b", "[]]", function() return 7 end))
  print(pcall(string.gsub, " _[y,)a y", "()", function() return 7 end, 2))
  print(pcall(string.gsub, "% ._z9y_z9.%-,", "%x+", function(c) return string.upper(c) end))
  do local out = {} for a, b in string.gfind("cb 1a- 0", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
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
