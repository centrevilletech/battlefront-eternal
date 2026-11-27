-- Generated differential program 120 (generate.py, seed 503).
-- tables
do
  local t = {22, 21, -28}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 189)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 145)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
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
  local w = {"lime", "kiwi", "b", "fig", "a", "plum"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- format
do
  print(pcall(string.format, "[%.4g]", -772))
  print(pcall(string.format, "[%-x]", 2147483647))
  print(pcall(string.format, "[%c]", 65))
  print(pcall(string.format, "[% #7u]", -1))
  print(pcall(string.format, "[%15f]", 3.4e38))
  print(pcall(string.format, "[%i]", -98765))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, "x]9)cx", "a?b"))
  print(pcall(string.find, "yb.,.9 _byby9y%[", "^%s*(.-)%s*$", 1))
  print(pcall(string.find, "0[(c(a% -9)", ".-b", 20))
  print(pcall(string.find, ")", "c-", -4))
  print(pcall(string.find, "z(a,%xz]%_( 19,b[", "(a*)"))
  print(pcall(string.find, " ", "^(%w+)", 1))
  print(pcall(string.find, "y-b[%a", "x*"))
  print(pcall(string.gsub, "].a-1a-.1bb-%[,z[", "%p", "%1-", 0))
  print(pcall(string.gsub, "0", "%p", "%%", 0))
  print(pcall(string.gsub, "b1a)a9)ay", "%b()", function() return false end, 2))
  do local out = {} for a, b in string.gfind("9xc0,0bb_", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- globals
do
  gx = 61
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
