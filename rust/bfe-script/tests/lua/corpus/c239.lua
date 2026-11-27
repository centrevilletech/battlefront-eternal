-- Generated differential program 239 (generate.py, seed 503).
-- trig
do
  print('cos', math.cos(10))
  print('exp', math.exp(2))
  print('tan', math.tan(1))
  print('deg', math.deg(0.001))
  print('atan2', math.atan2(1, -3))
  print('pi', math.pi, math.pi * 2)
end
-- compare
do
  print(65e8 < 2^-19, 65e8 <= 2^-19, 65e8 == 2^-19)
  print(2^21 < 677.68257, 2^21 <= 677.68257, 2^21 == 677.68257)
  print(2^14 < 562.65802, 2^14 <= 562.65802, 2^14 == 562.65802)
  print(774.98520 < 2^20, 774.98520 <= 2^20, 774.98520 == 2^20)
  print(-554 < 0.190593, -554 <= 0.190593, -554 == 0.190593)
  print(2^-22 < 355.44057, 2^-22 <= 355.44057, 2^-22 == 355.44057)
  print("" < "B0B9", "" <= "B0B9", "" == "B0B9")
  print("" < "A0a", "" <= "A0a", "" == "A0a")
  print("AbAA" < "", "AbAA" <= "", "AbAA" == "")
end
-- tables
do
  local t = {2, 40, -2}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 114)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 4)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
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
  local w = {"b", "a", "kiwi", "lime", "pear", "date"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- globals
do
  gx = 45
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
