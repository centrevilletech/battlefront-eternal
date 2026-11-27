-- Generated differential program 038 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "c)0 -1%,[x", "[^%s]+", 3))
  print(pcall(string.find, "]()", "[^%s]+"))
  print(pcall(string.find, "9[", "%p", 1))
  print(pcall(string.find, "%[b1-", "x*", 3))
  print(pcall(string.find, " aby_)z%z1z(09)", "[^,]+", -4))
  print(pcall(string.find, "]z xz%_[%", "(%d+)", -4))
  print(pcall(string.find, "))19a_] xbz(b", "%p"))
  print(pcall(string.gsub, "(", "[%a_][%w_]*", "<%0>", 0))
  print(pcall(string.gsub, ").1[x0_1-0x", "a?b", function() return false end))
  print(pcall(string.gsub, "1[_0", "^(%w+)", "#", 2))
  do local out = {} for a, b in string.gfind("c-]z1", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print(3 .. "x")
  print(2^24 + 1)
  print(-1/0 .. "")
  print(1e15 .. "")
  print(-0 .. "")
  print(10 .. 20)
  print("0x10" + 0)
  print(-"2")
  print(".5e1", tonumber(".5e1"))
  print("-inf", tonumber("-inf"))
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print("+3", tonumber("+3"))
  print("0x", tonumber("0x"))
  print("5.", tonumber("5."))
  print(21, tonumber("c6", 21))
  print(27, tonumber("2", 27))
  print(15, tonumber("933", 15))
  print(33, tonumber("pabi", 33))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- loops
do
  do local n, last = 0, nil for i = 10, 1, 0.1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 5 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 3 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
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
