-- Generated differential program 196 (generate.py, seed 503).
-- coerce
do
  print(1/0 .. "")
  print(2^24 + 1)
  print(-0 .. "")
  print(1e15 .. "")
  print(" 3e2 " * 1)
  print(1e100 .. "")
  print(16777217 .. "")
  print("0x10" + 0)
  print("12", tonumber("12"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1 2", tonumber("1 2"))
  print("1e", tonumber("1e"))
  print(".5e1", tonumber(".5e1"))
  print("inf", tonumber("inf"))
  print("5.", tonumber("5."))
  print(23, tonumber("j1", 23))
  print(26, tonumber("bnhp2", 26))
  print(9, tonumber("042203", 9))
  print(27, tonumber("0m0", 27))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- arith
do
  do local v = ((((5) - (117.56868)) * (-676)) + ((-531) * ((4294967295) - (-23.610))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (((1.17549435e-38) * ((2^-2) * (-45.515))) * (39e37))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (536)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (553)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(1.17549435e-38))
  print('ceil', math.ceil(602.89247))
  print('abs', math.abs(997.52028))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(285.2785, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(1.17549435e-38, 40e3, -3.662), math.max(4294967295, -6.770))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(0.75, -7))
  print('pow', 2 ^ 10)
end
-- compare
do
  print(237.91947 < 450.72202, 237.91947 <= 450.72202, 237.91947 == 450.72202)
  print(2^-2 < 381.88306, 2^-2 <= 381.88306, 2^-2 == 381.88306)
  print(8388609 < 8388609, 8388609 <= 8388609, 8388609 == 8388609)
  print(-29.661 < 0.76, -29.661 <= 0.76, -29.661 == 0.76)
  print(135 < 89e-39, 135 <= 89e-39, 135 == 89e-39)
  print(-373 < -926, -373 <= -926, -373 == -926)
  print("0b" < "bA_", "0b" <= "bA_", "0b" == "bA_")
  print("BB" < "", "BB" <= "", "BB" == "")
  print("bAB_" < "9cA", "bAB_" <= "9cA", "bAB_" == "9cA")
end
