-- Generated differential program 154 (generate.py, seed 503).
-- strings
do
  local s = "XY0Y09b11af"
  print(string.sub(s, 5, 8), string.sub(s, 5), string.byte(s, 8))
  print(string.sub(s, 7, 0), string.sub(s, 7), string.byte(s, 0))
  print(string.sub(s, -1, -11), string.sub(s, -1), string.byte(s, -11))
  print(string.sub(s, 11, -9), string.sub(s, 11), string.byte(s, -9))
  print(string.sub(s, -3, -7), string.sub(s, -3), string.byte(s, -7))
  print(string.sub(s, -3, 15), string.sub(s, -3), string.byte(s, 15))
  print(string.sub(s, 5, -4), string.sub(s, 5), string.byte(s, -4))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 0))
  print(string.char(77, 38, 109, 42, 51))
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
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 7 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(9), counter(15)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- patterns
do
  print(pcall(string.find, "xc)]-y(%x %", "%f[%w]%w+", 20))
  print(pcall(string.find, ".y_9", "c-", 20))
  print(pcall(string.find, "1_,zz]x", "[]]", 1))
  print(pcall(string.find, "a,_%-y(,_", "[]]", 3))
  print(pcall(string.gsub, "(bb([%)[01c))%-0", "[%.%-]", function() return false end, 0))
  print(pcall(string.gsub, "( .%),1a([x1", "%p", "#", 2))
  print(pcall(string.gsub, ",[%zay( 0%y1.c[.", ".-b", function() return false end, 1))
  do local out = {} for a, b in string.gfind("1-11a].bx", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
