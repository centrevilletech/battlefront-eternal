-- Generated differential program 158 (generate.py, seed 503).
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(5))
  local a, b, c = pass(1, 8, 6, 1)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
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
  local obj = setmetatable({name = "bo"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- strings
do
  local s = "b9fXad1"
  print(string.sub(s, 0, 10), string.sub(s, 0), string.byte(s, 10))
  print(string.sub(s, 7, -8), string.sub(s, 7), string.byte(s, -8))
  print(string.sub(s, 8, 15), string.sub(s, 8), string.byte(s, 15))
  print(string.sub(s, -12, -1), string.sub(s, -12), string.byte(s, -1))
  print(string.sub(s, 13, -13), string.sub(s, 13), string.byte(s, -13))
  print(string.sub(s, -7, 0), string.sub(s, -7), string.byte(s, 0))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 4))
  print(string.char(105))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- tables
do
  local t = {45, -1, 42, 47, -48, 22, -17, 41}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 161)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 0)
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
  local w = {"date", "lime", "plum", "a", "fig", "Apple"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
