-- Generated differential program 211 (generate.py, seed 503).
-- tables
do
  local t = {-24, 30, -2, -13, 42, -30}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
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
  local w = {"date", "pear", "plum", "fig", "lime", "b"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- arith
do
  do local v = ((((0.6358452986) * (16777216)) - ((169.54636) + (0.0372))) + (-22.265))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (54e-38)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((-10) + ((2^9) + (713))) * (41e35))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (-553)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(2))
  print('ceil', math.ceil(502.32930))
  print('abs', math.abs(3.4e38))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(-37.218, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(81e-36, 0.8787, 49e-7), math.max(943, 369.49776))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(-3, 15))
  print('pow', 0.5 ^ 0.5)
end
