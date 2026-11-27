-- Generated differential program 208 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 0.5, 1, 0.1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 36 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 9 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- strings
do
  local s = "eY9ZZae1 "
  print(string.sub(s, 13, 3), string.sub(s, 13), string.byte(s, 3))
  print(string.sub(s, 6, -9), string.sub(s, 6), string.byte(s, -9))
  print(string.sub(s, -5, -12), string.sub(s, -5), string.byte(s, -12))
  print(string.sub(s, 0, 7), string.sub(s, 0), string.byte(s, 7))
  print(string.sub(s, -3, -15), string.sub(s, -3), string.byte(s, -15))
  print(string.sub(s, 15, 0), string.sub(s, 15), string.byte(s, 0))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 4))
  print(string.char(101, 55, 51))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
