-- Generated differential program 128 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%0.4G]", 2^-19))
  print(pcall(string.format, "[%+ 5i]", 0))
  print(pcall(string.format, "[%0x]", 0))
  print(pcall(string.format, "[%0g]", 0.4254863497))
  print(pcall(string.format, "[%5.2s]", "tab\9here"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- strings
do
  local s = "Y9deb9X0f9"
  print(string.sub(s, 9, 1), string.sub(s, 9), string.byte(s, 1))
  print(string.sub(s, 11, -5), string.sub(s, 11), string.byte(s, -5))
  print(string.sub(s, 4, -10), string.sub(s, 4), string.byte(s, -10))
  print(string.sub(s, -7, 0), string.sub(s, -7), string.byte(s, 0))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 2))
  print(string.char(115, 55, 59, 54, 86))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- arith
do
  do local v = ((7) - (2147483647))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((3) / (-536)) * ((-20.604) + (5))) * (16777216))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((8388609) + (((723.95862) + (2)) * (-29.114)))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((-31.821) / (9)) * ((16777215) / (2^29))) - (((-22.691) - (45e12)) * ((-29.312) * (766.50167))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((196.84430) * ((-43.959) * ((7) * (62.16651))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((46e-13) + (8388609))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((16777216) + (((375.67156) + (0.76400)) + (2^30)))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = ((((261) * (-94)) - ((1/3) * (-6.660))) + (((795.48261) * (168)) * ((1.17549435e-38) + (1e-45))))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(55.34765))
  print('ceil', math.ceil(2^22))
  print('abs', math.abs(16777215))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(1e-45, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(549, 0.9321530, -786), math.max(-33.903, 33554433))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, 11))
  print('pow', 2 ^ 0.5)
end
