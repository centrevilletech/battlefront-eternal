-- Generated differential program 184 (generate.py, seed 503).
-- compare
do
  print(102 < -6.270, 102 <= -6.270, 102 == -6.270)
  print(-43.410 < -49.570, -43.410 <= -49.570, -43.410 == -49.570)
  print(-49.432 < 468.2867, -49.432 <= 468.2867, -49.432 == 468.2867)
  print(26e-5 < 0.2218, 26e-5 <= 0.2218, 26e-5 == 0.2218)
  print("_c0_" < "Bab0", "_c0_" <= "Bab0", "_c0_" == "Bab0")
  print("0A9B" < "cbc", "0A9B" <= "cbc", "0A9B" == "cbc")
  print("a" < "ac", "a" <= "ac", "a" == "ac")
end
-- strings
do
  local s = "fde"
  print(string.sub(s, -6, 13), string.sub(s, -6), string.byte(s, 13))
  print(string.sub(s, -9, 14), string.sub(s, -9), string.byte(s, 14))
  print(string.sub(s, 1, 10), string.sub(s, 1), string.byte(s, 10))
  print(string.sub(s, -3, 9), string.sub(s, -3), string.byte(s, 9))
  print(string.sub(s, -5, 0), string.sub(s, -5), string.byte(s, 0))
  print(string.sub(s, -7, 1), string.sub(s, -7), string.byte(s, 1))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 5))
  print(string.char(125, 109, 34, 54))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
