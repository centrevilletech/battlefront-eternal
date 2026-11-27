-- Generated differential program 219 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 2 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(2), counter(12)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- compare
do
  print(0.5728713644 < 8, 0.5728713644 <= 8, 0.5728713644 == 8)
  print(-39.311 < -49.896, -39.311 <= -49.896, -39.311 == -49.896)
  print(375.80072 < 9, 375.80072 <= 9, 375.80072 == 9)
  print(0.2433072162 < 0.1918, 0.2433072162 <= 0.1918, 0.2433072162 == 0.1918)
  print(2^-5 < -33.705, 2^-5 <= -33.705, 2^-5 == -33.705)
  print("00a" < "b", "00a" <= "b", "00a" == "b")
  print("__" < "bcBc", "__" <= "bcBc", "__" == "bcBc")
  print("AB" < "9BcA", "AB" <= "9BcA", "AB" == "9BcA")
end
-- globals
do
  gx = 38
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- format
do
  print(pcall(string.format, "[%0i]", 255))
  print(pcall(string.format, "[%17c]", 48))
  print(pcall(string.format, "[%0o]", 0))
  print(pcall(string.format, "[%11.5g]", 49e-19))
  print(pcall(string.format, "[%-#12g]", 0.93036076))
  print(pcall(string.format, "[%-#11f]", 3.4e38))
  print(pcall(string.format, "[%-G]", 436.43080))
  print(pcall(string.format, "[%.14f]", 2))
  print(pcall(string.format, "[%5.2s]", "xxxxxxxxxxxx"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
