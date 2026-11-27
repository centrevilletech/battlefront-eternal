-- Generated differential program 006 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%+3g]", 1e-45))
  print(pcall(string.format, "[%E]", 0.3))
  print(pcall(string.format, "[%-14u]", 2147483647))
  print(pcall(string.format, "[%- 2d]", 7))
  print(pcall(string.format, "[%c]", 97))
  print(pcall(string.format, "[%-12o]", 65535))
  print(pcall(string.format, "[%-x]", 1))
  print(pcall(string.format, "[%5.2s]", ""))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 2.6, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 5 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 9 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- trig
do
  print('exp', math.exp(2))
  print('log', math.log(1))
  print('atan', math.atan(2))
  print('sin', math.sin(10))
  print('atan2', math.atan2(1, -3))
  print('pi', math.pi, math.pi * 2)
end
