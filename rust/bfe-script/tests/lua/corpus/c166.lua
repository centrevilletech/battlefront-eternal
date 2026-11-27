-- Generated differential program 166 (generate.py, seed 503).
-- arith
do
  do local v = ((((0.2780352105) + (20e34)) / ((8388609) * (16e21))) * (((3.4e38) * (-41.203)) * ((7) * (2^-18))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((1e38) + (0.3)) + ((76e2) * (42.94431))) + (((1e38) * (16777215)) + ((2147483647) + (2^16))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((-45.270) / (16777217)) * (0.16469174219)) / ((39e12) * ((2^13) * (0.1))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((15e-3) * (9e-35)) + ((1) - (70e15))) * ((41) / ((-26.224) * (2^1))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((-2.987) - (-23.357)) + ((-834) - (0.1))) * (((2^-2) + (0.1968089703)) * ((2/3) * (2^6))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(16e-33))
  print('ceil', math.ceil(7))
  print('abs', math.abs(-38.450))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(74e-22, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.3, 16777216, 4294967295), math.max(2^3, 2^-5))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(0.75, -11))
  print('pow', 1.5 ^ 3)
end
-- strings
do
  local s = "YZ000 ebZ"
  print(string.sub(s, -8, 12), string.sub(s, -8), string.byte(s, 12))
  print(string.sub(s, 6, 12), string.sub(s, 6), string.byte(s, 12))
  print(string.sub(s, 10, 13), string.sub(s, 10), string.byte(s, 13))
  print(string.sub(s, -14, 12), string.sub(s, -14), string.byte(s, 12))
  print(string.sub(s, 2, 15), string.sub(s, 2), string.byte(s, 15))
  print(string.sub(s, 3, -13), string.sub(s, 3), string.byte(s, -13))
  print(string.sub(s, 7, -9), string.sub(s, 7), string.byte(s, -9))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 1))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
