-- Generated differential program 203 (generate.py, seed 503).
-- strings
do
  local s = "f9a"
  print(string.sub(s, 11, -4), string.sub(s, 11), string.byte(s, -4))
  print(string.sub(s, 2, -6), string.sub(s, 2), string.byte(s, -6))
  print(string.sub(s, 1, -4), string.sub(s, 1), string.byte(s, -4))
  print(string.sub(s, 3, -3), string.sub(s, 3), string.byte(s, -3))
  print(string.sub(s, -3, -9), string.sub(s, -3), string.byte(s, -9))
  print(string.sub(s, -6, 15), string.sub(s, -6), string.byte(s, 15))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 5))
  print(string.char(122, 65))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- coerce
do
  print(-"2")
  print(0.1 .. "")
  print(-1/0 .. "")
  print(" 3e2 " * 1)
  print(1/0 .. "")
  print(3 .. "x")
  print("7" * "6")
  print(123456789 .. "")
  print("+3", tonumber("+3"))
  print("1 2", tonumber("1 2"))
  print("0x", tonumber("0x"))
  print("inf", tonumber("inf"))
  print(".", tonumber("."))
  print("1e400", tonumber("1e400"))
  print("3x", tonumber("3x"))
  print(2, tonumber("0", 2))
  print(24, tonumber("f2g", 24))
  print(26, tonumber("9j05g", 26))
  print(5, tonumber("43", 5))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
