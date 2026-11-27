-- Generated differential program 104 (generate.py, seed 503).
-- arith
do
  do local v = (1/3)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((3.4e38) * (((6) - (1.17549435e-38)) + ((63e6) * (8388609))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((2) * (6)) - ((2^11) * (-5.733))) / (((4294967295) * (-12.331)) * ((0.751) / (398.23366))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((7) + (2^-6)) + ((257.31870) * (1.17549435e-38))) + (((-37.123) + (36e-1)) / ((2^-19) * (-35.87))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((9) * ((2^-2) - (16777216))) - (1/3))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((0.2) + (74e1)) + ((-3.903) + (-559))) * (0.59283101))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((5) + (28e1)) / ((1e-45) * (23e-39))) * (((190) * (33554433)) / (4294967295)))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(35e-36))
  print('ceil', math.ceil(-35.173))
  print('abs', math.abs(-19.6))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(16777217, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(3, 2/3, 677), math.max(-46.155, 2^13))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(1, -11))
  print('pow', 1.5 ^ -2)
end
-- strings
do
  local s = "YXdf9dZ"
  print(string.sub(s, -9, -8), string.sub(s, -9), string.byte(s, -8))
  print(string.sub(s, 14, 4), string.sub(s, 14), string.byte(s, 4))
  print(string.sub(s, -5, -12), string.sub(s, -5), string.byte(s, -12))
  print(string.sub(s, 3, -1), string.sub(s, 3), string.byte(s, -1))
  print(string.sub(s, 4, -1), string.sub(s, 4), string.byte(s, -1))
  print(string.sub(s, -1, -14), string.sub(s, -1), string.byte(s, -14))
  print(string.sub(s, -9, 4), string.sub(s, -9), string.byte(s, 4))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 2))
  print(string.char(69))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
