-- Generated differential program 207 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%u]", 0))
  print(pcall(string.format, "[%#-14g]", 1e38))
  print(pcall(string.format, "[%4d]", 7))
  print(pcall(string.format, "[%+-x]", 1))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(pcall(string.format, "[%#014g]", 0.2))
  print(pcall(string.format, "[% u]", 0))
  print(pcall(string.format, "[%07E]", -378))
  print(pcall(string.format, "[%1d]", 7))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, " 1 a.]]c", "(a*)", 1))
  print(pcall(string.find, "y]_)1zc[0ax0c", "z$", 1))
  print(pcall(string.find, "%xz(", "()", 20))
  print(pcall(string.find, ".xxy)b-", "(%d+)", -4))
  print(pcall(string.find, "[b0z,a", "()", 20))
  print(pcall(string.gsub, "%z0%-1", "c-", "#"))
  print(pcall(string.gsub, ",b_0%]9 ", "z$", "<%0>", 0))
  print(pcall(string.gsub, "ax(y", "%f[%w]%w+", "#"))
  do local out = {} for a, b in string.gfind("%_c[1][1._ -", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- tables
do
  local t = {23, 27, 1, 20, -39, 13, -19, -12, 16}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 107)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 2)
  print(table.getn(t), table.concat(t, ' '))
  table.sort(t)
  print(table.concat(t, ' '))
  table.sort(t, function(a, b) return a > b end)
  print(table.concat(t, ' '))
  print(pcall(table.concat, t, '-', 2, 3))
  print(table.concat({}, 'x'), table.concat(t, '', 3, 2))
  local u = {n = 2, 'a', 'b', 'c'} print(table.getn(u))
  table.setn(u, 1) print(table.getn(u))
  print(unpack({1, 2, nil, 4}))
  table.foreachi(t, function(i, v) if i <= 3 then print(i, v) end end)
  local w = {"fig", "date", "b", "pear", "lime", "a"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
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
  local a = setmetatable({v = 3}, mt)
  local b = setmetatable({v = 2}, mt)
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
