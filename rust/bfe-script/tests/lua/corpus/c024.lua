-- Generated differential program 024 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 1, 1, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 33 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 7 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- strings
do
  local s = "e0bcd"
  print(string.sub(s, -1, 4), string.sub(s, -1), string.byte(s, 4))
  print(string.sub(s, -7, -1), string.sub(s, -7), string.byte(s, -1))
  print(string.sub(s, 1, -2), string.sub(s, 1), string.byte(s, -2))
  print(string.sub(s, -12, -12), string.sub(s, -12), string.byte(s, -12))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 3))
  print(string.char(111, 76, 64, 36))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
