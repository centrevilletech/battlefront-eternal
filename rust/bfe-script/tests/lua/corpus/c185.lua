-- Generated differential program 185 (generate.py, seed 503).
-- compare
do
  print(2^-2 < -13.142, 2^-2 <= -13.142, 2^-2 == -13.142)
  print(46e-25 < -30.839, 46e-25 <= -30.839, 46e-25 == -30.839)
  print(4 < 2^-25, 4 <= 2^-25, 4 == 2^-25)
  print(-613 < -3.270, -613 <= -3.270, -613 == -3.270)
  print("b" < "", "b" <= "", "b" == "")
  print("0bca" < "", "0bca" <= "", "0bca" == "")
  print("0B99" < "a9", "0B99" <= "a9", "0B99" == "a9")
end
-- format
do
  print(pcall(string.format, "[%0 11E]", -3.409))
  print(pcall(string.format, "[%#19.5e]", 686.95512))
  print(pcall(string.format, "[%#3d]", -98765))
  print(pcall(string.format, "[%13i]", 7))
  print(pcall(string.format, "[%-+5X]", 7))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- coerce
do
  print(2^24 + 1)
  print(-1/0 .. "")
  print(123456789 .. "")
  print("  12  " + 0)
  print("1e3" / 4)
  print(".5" .. 1)
  print(16777217 .. "")
  print("10" + 5)
  print("-7.5", tonumber("-7.5"))
  print("5.", tonumber("5."))
  print("+3", tonumber("+3"))
  print("-inf", tonumber("-inf"))
  print("1e400", tonumber("1e400"))
  print("12", tonumber("12"))
  print("3x", tonumber("3x"))
  print(14, tonumber("0514", 14))
  print(3, tonumber("0", 3))
  print(35, tonumber("mac", 35))
  print(25, tonumber("79la1k", 25))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
