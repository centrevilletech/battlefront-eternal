-- Generated differential program 003 (generate.py, seed 503).
-- compare
do
  print(85e-8 < 2^14, 85e-8 <= 2^14, 85e-8 == 2^14)
  print(2^7 < 2^29, 2^7 <= 2^29, 2^7 == 2^29)
  print(16777217 < 71e14, 16777217 <= 71e14, 16777217 == 71e14)
  print(522.36482 < 339.22818, 522.36482 <= 339.22818, 522.36482 == 339.22818)
  print(966.89302 < 4, 966.89302 <= 4, 966.89302 == 4)
  print("ca" < "c", "ca" <= "c", "ca" == "c")
  print("" < "a", "" <= "a", "" == "a")
  print("B" < "9Ba", "B" <= "9Ba", "B" == "9Ba")
end
-- trig
do
  print('tan', math.tan(1))
  print('log10', math.log10(1))
  print('exp', math.exp(1))
  print('rad', math.rad(0.001))
  print('atan2', math.atan2(1, 2))
  print('pi', math.pi, math.pi * 2)
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
  local a, b, c = pass(7)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
