-- Generated differential program 091 (generate.py, seed 503).
-- strings
do
  local s = "cZa9cZ"
  print(string.sub(s, 7, 15), string.sub(s, 7), string.byte(s, 15))
  print(string.sub(s, -9, -6), string.sub(s, -9), string.byte(s, -6))
  print(string.sub(s, 10, -11), string.sub(s, 10), string.byte(s, -11))
  print(string.sub(s, 8, 5), string.sub(s, 8), string.byte(s, 5))
  print(string.sub(s, -8, 12), string.sub(s, -8), string.byte(s, 12))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 2))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- arith
do
  do local v = ((((-46.110) + (6e-8)) + ((33554433) - (314.87785))) / (((21e7) * (49e-2)) - ((0.4979614590) / (957))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (-21.500)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (16777216)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (41e-16)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((0.2) * (((0.893379) - (-395)) + ((46e-29) * (0.4))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((9) * (7)) * ((60e-23) - (4))) / (((2^28) * (0.2)) + ((0.76) - (634.10654))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (0.12246708)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(1e38))
  print('ceil', math.ceil(1e-45))
  print('abs', math.abs(1.17549435e-38))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(0.8306, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.60212, -18, 4294967295), math.max(0.42438536227, 16777215))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(1, -20))
  print('pow', 2 ^ 3)
end
-- format
do
  print(pcall(string.format, "[%10c]", 65))
  print(pcall(string.format, "[%16E]", -29.834))
  print(pcall(string.format, "[%12u]", 255))
  print(pcall(string.format, "[%15s]", "hello"))
  print(pcall(string.format, "[%07o]", 7))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(pcall(string.format, "[%#04X]", 123456))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
