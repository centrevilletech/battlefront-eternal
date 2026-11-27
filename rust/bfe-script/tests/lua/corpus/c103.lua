-- Generated differential program 103 (generate.py, seed 503).
-- globals
do
  gx = 1
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(2))
  local a, b, c = pass(6, 8)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- patterns
do
  print(pcall(string.find, "yab", "[%a_][%w_]*", 20))
  print(pcall(string.find, ")a,.b.c0]]c]-,", "(%a)(%d)", -4))
  print(pcall(string.find, ",", "[%.%-]"))
  print(pcall(string.find, "cc", "c-", 1))
  print(pcall(string.gsub, "- ) ", "(.)%1", "%1-", 1))
  print(pcall(string.gsub, "-.11_]z)y(]b_", "[]]", function(c) return nil end, 1))
  print(pcall(string.gsub, "._b1yb 1b %cx", ".-b", "%1-"))
  do local out = {} for a, b in string.gfind("b9z_(%._", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(8), counter(12)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
