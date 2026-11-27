-- Generated differential program 134 (generate.py, seed 503).
-- tables
do
  local t = {45, -23, 18}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
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
  local w = {"kiwi", "a", "date", "lime", "b", "fig"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 3, 1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 18 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 15 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- arith
do
  do local v = ((3e-1) - (((-544) * (2)) + ((0.3) / (-22.336))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((-16.478) + (214)) + ((6) * (759))) * (((2^4) / (2)) * ((2147483647) + (3.4e38))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((18e22) * (85e11)) - (((40e21) * (3.4e38)) / ((0.350) / (0.02530))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (1)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((-29.894) * (70e1))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (33554433)
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(2^30))
  print('ceil', math.ceil(0.3))
  print('abs', math.abs(0.2))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(-11.483, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(2^18, 7e-20, 2^-20), math.max(1/3, 2/3))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(0.75, -3))
  print('pow', 2 ^ 10)
end
