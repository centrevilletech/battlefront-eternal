-- Generated differential program 064 (generate.py, seed 503).
-- globals
do
  gx = 66
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, nil))
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
-- patterns
do
  print(pcall(string.find, ".]]a_11c(]", ".-b"))
  print(pcall(string.find, " .9(ayzc_01[cz-", "%x+", 1))
  print(pcall(string.find, "a00zc)xa1-().a9", "[%.%-]", 1))
  print(pcall(string.find, "]19x9x9zy1 ", "(a*)", 1))
  print(pcall(string.find, "baa-1[)xc.% b1-1", "z$", 1))
  print(pcall(string.find, "y.c,c[9 ", "c-"))
  print(pcall(string.find, " [ )%_y", "()", 1))
  print(pcall(string.gsub, "x", "(a*)", "%1-", 0))
  print(pcall(string.gsub, "", "%x+", function() return false end, 1))
  print(pcall(string.gsub, "_,[a19x]by", "(a*)", "%1-"))
  do local out = {} for a, b in string.gfind("91x[", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 6 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(9), counter(10)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
