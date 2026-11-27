-- Generated differential program 027 (generate.py, seed 503).
-- tables
do
  local t = {-13, 49, 27}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 120)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 180)
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
  local w = {"a", "date", "plum", "b", "kiwi", "pear"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(7))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- compare
do
  print(2.49683 < -177, 2.49683 <= -177, 2.49683 == -177)
  print(369.36336 < 0.85112274152, 369.36336 <= 0.85112274152, 369.36336 == 0.85112274152)
  print(348.69468 < 58e-13, 348.69468 <= 58e-13, 348.69468 == 58e-13)
  print(2147483647 < 1/3, 2147483647 <= 1/3, 2147483647 == 1/3)
  print(-35.532 < 2^7, -35.532 <= 2^7, -35.532 == 2^7)
  print(0.36582474 < 12e33, 0.36582474 <= 12e33, 0.36582474 == 12e33)
  print("A" < "_Bb", "A" <= "_Bb", "A" == "_Bb")
  print("A" < "", "A" <= "", "A" == "")
  print("9bca" < "b9a", "9bca" <= "b9a", "9bca" == "b9a")
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
