-- Generated differential program 029 (generate.py, seed 503).
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(1))
  local a, b, c = pass(4, 2, 2)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(8), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- tables
do
  local t = {16, 43, -32}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 104)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 101)
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
  local w = {"kiwi", "b", "fig", "date", "Apple", "a"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- format
do
  print(pcall(string.format, "[%c]", 65))
  print(pcall(string.format, "[%.0G]", 33554433))
  print(pcall(string.format, "[%0+i]", 65535))
  print(pcall(string.format, "[%0x]", 255))
  print(pcall(string.format, "[%+ o]", 1))
  print(pcall(string.format, "[%5.2s]", "tab\9here"))
  print(pcall(string.format, "[%#g]", 2^0))
  print(pcall(string.format, "[%- .0e]", 33554433))
  print(pcall(string.format, "[%016.6f]", 1/3))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
