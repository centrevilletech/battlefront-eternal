-- Generated differential program 089 (generate.py, seed 503).
-- tables
do
  local t = {36, 39, -37, -10, -7, -34, 37, -39}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 190)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -3)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 187)
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
  local w = {"date", "kiwi", "b", "fig", "a", "pear"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, true))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- compare
do
  print(58e-5 < 33e-31, 58e-5 <= 33e-31, 58e-5 == 33e-31)
  print(-664 < -27.380, -664 <= -27.380, -664 == -27.380)
  print(-717 < 623.450, -717 <= 623.450, -717 == 623.450)
  print(707.13393 < 0.12052210586, 707.13393 <= 0.12052210586, 707.13393 == 0.12052210586)
  print(16777215 < 9, 16777215 <= 9, 16777215 == 9)
  print(1.17549435e-38 < 457, 1.17549435e-38 <= 457, 1.17549435e-38 == 457)
  print(5 < 4, 5 <= 4, 5 == 4)
  print("9" < "bAb", "9" <= "bAb", "9" == "bAb")
  print("" < "bb", "" <= "bb", "" == "bb")
  print("" < "bB", "" <= "bB", "" == "bB")
end
