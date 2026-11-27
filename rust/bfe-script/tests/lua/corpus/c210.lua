-- Generated differential program 210 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 4 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(6), counter(17)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- arith
do
  do local v = ((((-272) + (2^-14)) * ((16e7) + (-41.172))) * (4))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((587.83131) * (2^-26)) / ((2^21) * (16777216))) / (((821) / (6)) + ((437) + (-21.473))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((2^0) * (16e2)) + (3.4e38)) - (16777215))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((0.689001) / (0.972544035))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((8) / (-857)) - ((2/3) * (551))) * ((1e-45) - ((2) + (2^9))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(2^8))
  print('ceil', math.ceil(261.63022))
  print('abs', math.abs(2^-28))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(1, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(2^2, 16777215, 475.55952), math.max(119.93030, -807))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(0.75, -10))
  print('pow', 10 ^ 0.5)
end
-- tables
do
  local t = {19, 37, -20, 22, -44, 6, -26, 17, -17}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, 1)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 117)
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
  local w = {"pear", "kiwi", "plum", "b", "date", "fig"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- globals
do
  gx = 46
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
