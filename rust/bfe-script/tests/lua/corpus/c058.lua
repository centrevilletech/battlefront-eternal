-- Generated differential program 058 (generate.py, seed 503).
-- arith
do
  do local v = ((((7) * (859)) * (0.5556)) * (((-45.484) * (4294967295)) + ((-27.302) + (0.75580692))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (903.50214)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((-1.640) - ((4294967295) - (0.1))) * (287))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (-7.924)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(-23.631))
  print('ceil', math.ceil(79e-20))
  print('abs', math.abs(-736))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(-18.105, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(1e38, 2^-5, 0.167105828), math.max(2^16, 1e-45))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(-3, -20))
  print('pow', 2 ^ 10)
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(6))
  local a, b, c = pass(1, 7)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
