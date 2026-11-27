-- Generated differential program 025 (generate.py, seed 503).
-- arith
do
  do local v = (635.87810)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2^8)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((6) / (1e-45)) + ((2^-18) + (2^15))) / (((369.57710) - (89e7)) - ((2147483647) + (13e-23))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((172.3350) * (0.1788852))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((-10.350) * ((9) - (2^-8))) + ((-655) / ((0.84872) / (71e-26))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (4)
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((2^27) - (-357))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(0.32447316915))
  print('ceil', math.ceil(647.85441))
  print('abs', math.abs(0.825))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(-7.964, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(1.17549435e-38, 1e-45, 16777215), math.max(0.856653, 2e24))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(-3, -13))
  print('pow', 10 ^ 10)
end
-- tables
do
  local t = {47, 49, 9, 23}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -5)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 186)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 102)
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
  local w = {"Apple", "kiwi", "plum", "date", "pear", "fig"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(10))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 4 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(15)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
