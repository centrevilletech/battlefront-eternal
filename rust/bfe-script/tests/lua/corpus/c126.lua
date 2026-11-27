-- Generated differential program 126 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%x]", 1))
  print(pcall(string.format, "[%+14x]", 7))
  print(pcall(string.format, "[%0.6E]", 2))
  print(pcall(string.format, "[%17c]", 97))
  print(pcall(string.format, "[%i]", 7))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- coerce
do
  print(-0 .. "")
  print(3 .. "x")
  print(10 .. 20)
  print(123456789 .. "")
  print(1e100 .. "")
  print(16777217 .. "")
  print(1/0 .. "")
  print("10" + 5)
  print("-7.5", tonumber("-7.5"))
  print("-inf", tonumber("-inf"))
  print("0x1F", tonumber("0x1F"))
  print(" 12 ", tonumber(" 12 "))
  print("+3", tonumber("+3"))
  print("1e", tonumber("1e"))
  print("12", tonumber("12"))
  print(30, tonumber("8sl9o9", 30))
  print(13, tonumber("1094", 13))
  print(15, tonumber("095854", 15))
  print(30, tonumber("jbbr", 30))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
