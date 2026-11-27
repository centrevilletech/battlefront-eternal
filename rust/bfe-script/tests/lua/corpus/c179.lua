-- Generated differential program 179 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 1, 2.6, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 29 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 1 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- coerce
do
  print(" 3e2 " * 1)
  print(0.1 .. "")
  print(1e100 .. "")
  print(1e15 .. "")
  print("  12  " + 0)
  print(-"2")
  print(16777217 .. "")
  print(3 .. "x")
  print("-inf", tonumber("-inf"))
  print("1e", tonumber("1e"))
  print("12", tonumber("12"))
  print("1e400", tonumber("1e400"))
  print(".5e1", tonumber(".5e1"))
  print("3x", tonumber("3x"))
  print("0x", tonumber("0x"))
  print(2, tonumber("011", 2))
  print(3, tonumber("2 1", 3))
  print(3, tonumber("10", 3))
  print(3, tonumber("10", 3))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
