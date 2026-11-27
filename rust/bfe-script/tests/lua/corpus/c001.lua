-- Generated differential program 001 (generate.py, seed 503).
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
-- compare
do
  print(2^4 < 714.75892, 2^4 <= 714.75892, 2^4 == 714.75892)
  print(97e-19 < 2147483647, 97e-19 <= 2147483647, 97e-19 == 2147483647)
  print(681.20172 < -32.302, 681.20172 <= -32.302, 681.20172 == -32.302)
  print(2^21 < 93e-21, 2^21 <= 93e-21, 2^21 == 93e-21)
  print(949.33555 < 403.74218, 949.33555 <= 403.74218, 949.33555 == 403.74218)
  print(36 < 2^25, 36 <= 2^25, 36 == 2^25)
  print(-366 < 70.36481, -366 <= 70.36481, -366 == 70.36481)
  print("BB0c" < "", "BB0c" <= "", "BB0c" == "")
  print("90AB" < "0cbA", "90AB" <= "0cbA", "90AB" == "0cbA")
  print("cB0_" < "Acb", "cB0_" <= "Acb", "cB0_" == "Acb")
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(1))
  local a, b, c = pass(7, 5)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
