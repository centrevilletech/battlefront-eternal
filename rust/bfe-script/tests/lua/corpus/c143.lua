-- Generated differential program 143 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "[,b,", "%p", -4))
  print(pcall(string.find, "-a.%c9ax-", "(b+)(c*)", 20))
  print(pcall(string.find, " -a9y9z%,c", "^%s*(.-)%s*$"))
  print(pcall(string.find, "", "[%a_][%w_]*", 20))
  print(pcall(string.find, ",x. [", "(.)%1", -4))
  print(pcall(string.find, "z0y0%-](", "(.)%1", 1))
  print(pcall(string.gsub, "c.b", "^(%w+)", function() return 7 end, 0))
  print(pcall(string.gsub, "", "z$", function(c) return string.upper(c) end))
  print(pcall(string.gsub, ",)z1[1.,0([y9", "^(%w+)", "%1-", 0))
  do local out = {} for a, b in string.gfind("1.x%cc]", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print(1e100 .. "")
  print("  12  " + 0)
  print(0.1 .. "")
  print(123456789 .. "")
  print("10" + 5)
  print(2^24 + 1)
  print(1e15 .. "")
  print(".5" .. 1)
  print("+3", tonumber("+3"))
  print("-inf", tonumber("-inf"))
  print("0x1p4", tonumber("0x1p4"))
  print("1e", tonumber("1e"))
  print("  0x10  ", tonumber("  0x10  "))
  print("0x1F", tonumber("0x1F"))
  print(".5e1", tonumber(".5e1"))
  print(11, tonumber("810", 11))
  print(2, tonumber("00", 2))
  print(16, tonumber("558a", 16))
  print(2, tonumber("11", 2))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
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
