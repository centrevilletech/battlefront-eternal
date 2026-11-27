-- Generated differential program 231 (generate.py, seed 503).
-- arith
do
  do local v = ((((2^2) + (2^-11)) * ((97e20) * (16777216))) + (((2^23) + (-446)) / ((-23.982) * (23e5))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (((76e12) - (85e33)) + (0.53))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (2^20)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (807.82648)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((233.77735) - (-291)) * ((826.45405) + (8))) / (((16777217) + (937)) / ((59e31) * (16777215))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(-38.913))
  print('ceil', math.ceil(16777216))
  print('abs', math.abs(169.91390))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(0.52690949, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(-34.406, 2^14, -19.271), math.max(9, 52e12))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(1, 1))
  print('pow', 10 ^ 0.5)
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(14))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(2500))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 3))
end
-- tables
do
  local t = {-46, -47, -11}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 110)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 151)
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
  local w = {"a", "apple", "kiwi", "b", "lime", "plum"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- compare
do
  print(1/3 < 16777217, 1/3 <= 16777217, 1/3 == 16777217)
  print(2/3 < 528, 2/3 <= 528, 2/3 == 528)
  print(2^6 < -525, 2^6 <= -525, 2^6 == -525)
  print(2 < 953, 2 <= 953, 2 == 953)
  print(2^15 < 0.1, 2^15 <= 0.1, 2^15 == 0.1)
  print(-34.787 < 0.125705, -34.787 <= 0.125705, -34.787 == 0.125705)
  print(461 < 286, 461 <= 286, 461 == 286)
  print("0A" < "9_0", "0A" <= "9_0", "0A" == "9_0")
  print("aacb" < "B0A", "aacb" <= "B0A", "aacb" == "B0A")
  print("9" < "_9", "9" <= "_9", "9" == "_9")
end
