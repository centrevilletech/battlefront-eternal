-- Generated differential program 086 (generate.py, seed 503).
-- compare
do
  print(-48.222 < 3.4e38, -48.222 <= 3.4e38, -48.222 == 3.4e38)
  print(528.67708 < 6, 528.67708 <= 6, 528.67708 == 6)
  print(519.82038 < -28.607, 519.82038 <= -28.607, 519.82038 == -28.607)
  print(0.3 < -42.510, 0.3 <= -42.510, 0.3 == -42.510)
  print(0.093 < -21.901, 0.093 <= -21.901, 0.093 == -21.901)
  print(182.10138 < 68e-2, 182.10138 <= 68e-2, 182.10138 == 68e-2)
  print("" < "", "" <= "", "" == "")
  print("Bb" < "_B90", "Bb" <= "_B90", "Bb" == "_B90")
  print("" < "B9", "" <= "B9", "" == "B9")
end
-- strings
do
  local s = "ba0bf9e"
  print(string.sub(s, 8, 7), string.sub(s, 8), string.byte(s, 7))
  print(string.sub(s, -13, 7), string.sub(s, -13), string.byte(s, 7))
  print(string.sub(s, 8, -10), string.sub(s, 8), string.byte(s, -10))
  print(string.sub(s, -2, 14), string.sub(s, -2), string.byte(s, 14))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 5))
  print(string.char(113))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- loops
do
  do local n, last = 0, nil for i = 10, 3, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 20 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 12 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
