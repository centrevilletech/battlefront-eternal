-- Generated differential program 223 (generate.py, seed 503).
-- coerce
do
  print(16777217 .. "")
  print(".5" .. 1)
  print(-1/0 .. "")
  print("  12  " + 0)
  print(10 .. 20)
  print(3 .. "x")
  print(1e15 .. "")
  print("1e3" / 4)
  print("5.", tonumber("5."))
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print("+3", tonumber("+3"))
  print("1e400", tonumber("1e400"))
  print(".", tonumber("."))
  print("  0x10  ", tonumber("  0x10  "))
  print(28, tonumber("qn", 28))
  print(8, tonumber("6642", 8))
  print(32, tonumber("j", 32))
  print(25, tonumber("i", 25))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- trig
do
  print('tan', math.tan(1))
  print('atan', math.atan(0.5))
  print('log', math.log(0.001))
  print('log10', math.log10(0.001))
  print('atan2', math.atan2(0.5, 2))
  print('pi', math.pi, math.pi * 2)
end
-- strings
do
  local s = "9Yef1YZZeZ"
  print(string.sub(s, -2, -10), string.sub(s, -2), string.byte(s, -10))
  print(string.sub(s, -14, 5), string.sub(s, -14), string.byte(s, 5))
  print(string.sub(s, -12, 7), string.sub(s, -12), string.byte(s, 7))
  print(string.sub(s, -3, 10), string.sub(s, -3), string.byte(s, 10))
  print(string.sub(s, 2, -12), string.sub(s, 2), string.byte(s, -12))
  print(string.sub(s, 11, 10), string.sub(s, 11), string.byte(s, 10))
  print(string.sub(s, -7, 14), string.sub(s, -7), string.byte(s, 14))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 5))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
