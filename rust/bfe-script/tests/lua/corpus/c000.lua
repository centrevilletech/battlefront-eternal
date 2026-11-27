-- Generated differential program 000 (generate.py, seed 503).
-- compare
do
  print(-19.140 < -916, -19.140 <= -916, -19.140 == -916)
  print(-44.126 < 727, -44.126 <= 727, -44.126 == 727)
  print(525 < 8, 525 <= 8, 525 == 8)
  print(-46.39 < 776.84260, -46.39 <= 776.84260, -46.39 == 776.84260)
  print(6 < 1e38, 6 <= 1e38, 6 == 1e38)
  print("B0B" < "cB_", "B0B" <= "cB_", "B0B" == "cB_")
  print("" < "A0c", "" <= "A0c", "" == "A0c")
  print("" < "", "" <= "", "" == "")
end
-- strings
do
  local s = "d9fYec"
  print(string.sub(s, 13, 13), string.sub(s, 13), string.byte(s, 13))
  print(string.sub(s, 8, -13), string.sub(s, 8), string.byte(s, -13))
  print(string.sub(s, 0, -15), string.sub(s, 0), string.byte(s, -15))
  print(string.sub(s, -11, -13), string.sub(s, -11), string.byte(s, -13))
  print(string.sub(s, -5, -9), string.sub(s, -5), string.byte(s, -9))
  print(string.sub(s, -1, 9), string.sub(s, -1), string.byte(s, 9))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 4))
  print(string.char(38))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
