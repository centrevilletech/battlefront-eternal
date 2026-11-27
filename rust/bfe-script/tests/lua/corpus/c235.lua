-- Generated differential program 235 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 1, 1, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 17 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 15 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- strings
do
  local s = "091 de0X "
  print(string.sub(s, 7, -9), string.sub(s, 7), string.byte(s, -9))
  print(string.sub(s, -4, -13), string.sub(s, -4), string.byte(s, -13))
  print(string.sub(s, 4, 0), string.sub(s, 4), string.byte(s, 0))
  print(string.sub(s, 1, -9), string.sub(s, 1), string.byte(s, -9))
  print(string.sub(s, -8, 4), string.sub(s, -8), string.byte(s, 4))
  print(string.sub(s, 7, 11), string.sub(s, 7), string.byte(s, 11))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 5))
  print(string.char(72, 46, 56, 36))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- coerce
do
  print(1e15 .. "")
  print(123456789 .. "")
  print("0x10" + 0)
  print("10" + 5)
  print("5" + "0.25")
  print(-0 .. "")
  print("1e3" / 4)
  print(".5" .. 1)
  print("1e400", tonumber("1e400"))
  print("-inf", tonumber("-inf"))
  print("0x", tonumber("0x"))
  print("5.", tonumber("5."))
  print(".", tonumber("."))
  print("  0x10  ", tonumber("  0x10  "))
  print(" 12 ", tonumber(" 12 "))
  print(28, tonumber("na7", 28))
  print(25, tonumber("b deob", 25))
  print(15, tonumber("89", 15))
  print(36, tonumber("uk", 36))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- arith
do
  do local v = (0.1)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((824.46928) + (((64e-28) + (2147483647)) / ((2) + (640.853))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (2)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((16777217) * (-732)) * (((8) - (35e11)) * ((1e-45) - (16777215))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((16777216) + (16777215))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((9) * ((0.4836) * (0.1687)))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(2^23))
  print('ceil', math.ceil(31e9))
  print('abs', math.abs(0.39622731604))
  print('sqrt', math.sqrt(3))
  do local m = math.mod(1/3, 3) if m == m then print('mod', m) end end
  print('minmax', math.min(2, 2/3, 22e-4), math.max(0.22986, 1/3))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(1, -19))
  print('pow', 2 ^ -2)
end
