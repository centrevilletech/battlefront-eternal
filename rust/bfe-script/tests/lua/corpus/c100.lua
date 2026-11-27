-- Generated differential program 100 (generate.py, seed 503).
-- strings
do
  local s = "ebcY ZX1X1"
  print(string.sub(s, 7, 9), string.sub(s, 7), string.byte(s, 9))
  print(string.sub(s, -12, -14), string.sub(s, -12), string.byte(s, -14))
  print(string.sub(s, 4, -13), string.sub(s, 4), string.byte(s, -13))
  print(string.sub(s, -12, 12), string.sub(s, -12), string.byte(s, 12))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 4))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 6 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(4), counter(16)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- coerce
do
  print("1e3" / 4)
  print(16777217 .. "")
  print(-"2")
  print(" 3e2 " * 1)
  print("  12  " + 0)
  print(-1/0 .. "")
  print(1/0 .. "")
  print(-0 .. "")
  print("+3", tonumber("+3"))
  print("-7.5", tonumber("-7.5"))
  print("0x", tonumber("0x"))
  print("1 2", tonumber("1 2"))
  print(".5e1", tonumber(".5e1"))
  print("12", tonumber("12"))
  print("0x1p4", tonumber("0x1p4"))
  print(4, tonumber("1", 4))
  print(32, tonumber("bn", 32))
  print(10, tonumber("903463", 10))
  print(35, tonumber("x10", 35))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 's'))
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
