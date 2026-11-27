-- Generated differential program 171 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 4 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(10)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- compare
do
  print(985.28451 < 0.2, 985.28451 <= 0.2, 985.28451 == 0.2)
  print(8388609 < 37e-37, 8388609 <= 37e-37, 8388609 == 37e-37)
  print(2 < 0.6043536, 2 <= 0.6043536, 2 == 0.6043536)
  print(4294967295 < 0.2364574, 4294967295 <= 0.2364574, 4294967295 == 0.2364574)
  print("Ab9A" < "cbcB", "Ab9A" <= "cbcB", "Ab9A" == "cbcB")
  print("9" < "Bb9c", "9" <= "Bb9c", "9" == "Bb9c")
  print("0a" < "B_b", "0a" <= "B_b", "0a" == "B_b")
end
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
