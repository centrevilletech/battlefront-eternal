-- Generated differential program 023 (generate.py, seed 503).
-- tables
do
  local t = {28, 48, -6}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
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
  local w = {"lime", "b", "fig", "kiwi", "a", "Apple"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- globals
do
  gx = 98
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- arith
do
  do local v = (28e19)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2^13)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((2/3) + (((0.520635174) - (2^21)) * ((474.58910) - (0.3))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (2/3)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((-14.229) + (747.91671)) + ((165.58072) + (-46.774))) / (((-6.213) + (61e-25)) * (0.2)))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(-538))
  print('ceil', math.ceil(66e30))
  print('abs', math.abs(2^-24))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(-37.647, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(0.686292177, 1e-45, -40.387), math.max(89e-10, 48e-6))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, 10))
  print('pow', 1.5 ^ 0.5)
end
-- compare
do
  print(-16.346 < 22e29, -16.346 <= 22e29, -16.346 == 22e29)
  print(0.577298 < 1e38, 0.577298 <= 1e38, 0.577298 == 1e38)
  print(0.7341181764 < 56e-31, 0.7341181764 <= 56e-31, 0.7341181764 == 56e-31)
  print(8388609 < -878, 8388609 <= -878, 8388609 == -878)
  print(780.84235 < 2, 780.84235 <= 2, 780.84235 == 2)
  print(33554433 < -14, 33554433 <= -14, 33554433 == -14)
  print("99" < "", "99" <= "", "99" == "")
  print("cB_9" < "ba_0", "cB_9" <= "ba_0", "cB_9" == "ba_0")
  print("c" < "aBc", "c" <= "aBc", "c" == "aBc")
end
