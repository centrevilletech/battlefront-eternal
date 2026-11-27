-- Generated differential program 009 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 1, 1, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 35 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 18 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- compare
do
  print(0.7464 < -39.923, 0.7464 <= -39.923, 0.7464 == -39.923)
  print(-24.548 < -216, -24.548 <= -216, -24.548 == -216)
  print(580.36363 < 20e-22, 580.36363 <= 20e-22, 580.36363 == 20e-22)
  print(5 < 9, 5 <= 9, 5 == 9)
  print(607 < 2^-26, 607 <= 2^-26, 607 == 2^-26)
  print(2/3 < 80e24, 2/3 <= 80e24, 2/3 == 80e24)
  print("BaAc" < "_", "BaAc" <= "_", "BaAc" == "_")
  print("0" < "b", "0" <= "b", "0" == "b")
  print("B00b" < "00", "B00b" <= "00", "B00b" == "00")
end
-- trig
do
  print('atan', math.atan(0.001))
  print('log', math.log(0.25))
  print('deg', math.deg(3))
  print('log10', math.log10(10))
  print('atan2', math.atan2(0.5, -3))
  print('pi', math.pi, math.pi * 2)
end
