-- Generated differential program 057 (generate.py, seed 503).
-- strings
do
  local s = "a  X1Xd0d"
  print(string.sub(s, 12, 5), string.sub(s, 12), string.byte(s, 5))
  print(string.sub(s, 9, 0), string.sub(s, 9), string.byte(s, 0))
  print(string.sub(s, 1, -8), string.sub(s, 1), string.byte(s, -8))
  print(string.sub(s, -13, 8), string.sub(s, -13), string.byte(s, 8))
  print(string.sub(s, 15, 13), string.sub(s, 15), string.byte(s, 13))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 2))
  print(string.char(72, 53, 99, 125, 104))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- trig
do
  print('log', math.log(3))
  print('exp', math.exp(10))
  print('sin', math.sin(10))
  print('tan', math.tan(2))
  print('atan2', math.atan2(1, 1))
  print('pi', math.pi, math.pi * 2)
end
-- compare
do
  print(-40.516 < 2^-1, -40.516 <= 2^-1, -40.516 == 2^-1)
  print(0.2 < 225, 0.2 <= 225, 0.2 == 225)
  print(-19.393 < 4, -19.393 <= 4, -19.393 == 4)
  print(79e25 < 0.809446447, 79e25 <= 0.809446447, 79e25 == 0.809446447)
  print(914.30924 < 8388609, 914.30924 <= 8388609, 914.30924 == 8388609)
  print(70e-18 < 2^25, 70e-18 <= 2^25, 70e-18 == 2^25)
  print("0a" < "", "0a" <= "", "0a" == "")
  print("" < "cB", "" <= "cB", "" == "cB")
  print("0BA" < "", "0BA" <= "", "0BA" == "")
end
-- format
do
  print(pcall(string.format, "[%12c]", 48))
  print(pcall(string.format, "[%0-x]", 123456))
  print(pcall(string.format, "[% 9x]", 65535))
  print(pcall(string.format, "[%4.12s]", "a b"))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(pcall(string.format, "[%#2o]", 0))
  print(pcall(string.format, "[%-x]", 0))
  print(pcall(string.format, "[%9o]", 65535))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
