-- Generated differential program 119 (generate.py, seed 503).
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
-- patterns
do
  print(pcall(string.find, "[a-", "%a+", 20))
  print(pcall(string.find, ".(_. (a_19x.x,1b.", "%s"))
  print(pcall(string.find, "xa zx9- b%_a", "a?b", 1))
  print(pcall(string.find, "x) -a-%-_,)_ yz)_", "%s", 20))
  print(pcall(string.find, "_,", "[^%s]+", -4))
  print(pcall(string.gsub, "bz1a[z ]-% ])", "(b+)(c*)", function(c) return string.upper(c) end, 2))
  print(pcall(string.gsub, "1z9x0[,yz]acz)", "%f[%w]%w+", function(c) return string.upper(c) end, 2))
  print(pcall(string.gsub, ".cb_z-yyc", "%x+", "<%0>"))
  do local out = {} for a, b in string.gfind("-z-.])([yx", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
