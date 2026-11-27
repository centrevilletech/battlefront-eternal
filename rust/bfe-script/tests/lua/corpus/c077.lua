-- Generated differential program 077 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(10))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
-- compare
do
  print(599 < 2147483647, 599 <= 2147483647, 599 == 2147483647)
  print(2^13 < 16777216, 2^13 <= 16777216, 2^13 == 16777216)
  print(25e-34 < 8388609, 25e-34 <= 8388609, 25e-34 == 8388609)
  print(2/3 < 75e-16, 2/3 <= 75e-16, 2/3 == 75e-16)
  print(16777217 < 0.87861744, 16777217 <= 0.87861744, 16777217 == 0.87861744)
  print(4 < 33554433, 4 <= 33554433, 4 == 33554433)
  print("a" < "b", "a" <= "b", "a" == "b")
  print("_aBb" < "90", "_aBb" <= "90", "_aBb" == "90")
  print("c" < "", "c" <= "", "c" == "")
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, -0.5))
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
