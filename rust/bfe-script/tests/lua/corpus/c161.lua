-- Generated differential program 161 (generate.py, seed 503).
-- coerce
do
  print(1e100 .. "")
  print(0.1 .. "")
  print(-0 .. "")
  print(3 .. "x")
  print("1e3" / 4)
  print("10" + 5)
  print(123456789 .. "")
  print("  12  " + 0)
  print("1 2", tonumber("1 2"))
  print(".5e1", tonumber(".5e1"))
  print("3x", tonumber("3x"))
  print("1e400", tonumber("1e400"))
  print("-7.5", tonumber("-7.5"))
  print("0x", tonumber("0x"))
  print("1e", tonumber("1e"))
  print(14, tonumber("515c4", 14))
  print(16, tonumber("4", 16))
  print(35, tonumber("6nu", 35))
  print(7, tonumber("346125", 7))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- tables
do
  local t = {-23, 11, -50, -18, 17, 29, 1, -46}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, 2)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 0)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -3)
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
  local w = {"Apple", "date", "kiwi", "b", "a", "lime"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 42))
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
