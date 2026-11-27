-- Generated differential program 044 (generate.py, seed 503).
-- tables
do
  local t = {18, 7, -43, 6, -3, 39, -12, -49}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, math.max(1, math.min(3, table.getn(t) + 1)), 182)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 190)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 101)
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
  local w = {"date", "lime", "fig", "plum", "apple", "pear"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(11))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- loops
do
  do local n, last = 0, nil for i = 10, 1, 1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 34 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 7 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
