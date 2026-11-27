-- Generated differential program 222 (generate.py, seed 503).
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(7))
  local a, b, c = pass(4, 5)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- trig
do
  print('sin', math.sin(10))
  print('log10', math.log10(10))
  print('tan', math.tan(0.5))
  print('cos', math.cos(0.001))
  print('atan2', math.atan2(-1, -3))
  print('pi', math.pi, math.pi * 2)
end
