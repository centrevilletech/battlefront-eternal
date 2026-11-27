-- Generated differential program 083 (generate.py, seed 503).
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, true))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- coerce
do
  print("5" + "0.25")
  print(10 .. 20)
  print(".5" .. 1)
  print(1/0 .. "")
  print(0.1 .. "")
  print(3 .. "x")
  print("0x10" + 0)
  print("  12  " + 0)
  print("5.", tonumber("5."))
  print("3x", tonumber("3x"))
  print("inf", tonumber("inf"))
  print("  0x10  ", tonumber("  0x10  "))
  print(".", tonumber("."))
  print("+3", tonumber("+3"))
  print("12", tonumber("12"))
  print(36, tonumber("ti", 36))
  print(6, tonumber("302", 6))
  print(6, tonumber("3", 6))
  print(8, tonumber("43", 8))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- loops
do
  do local n, last = 0, nil for i = 1, 2.6, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 18 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 15 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
