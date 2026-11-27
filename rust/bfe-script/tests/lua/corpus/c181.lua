-- Generated differential program 181 (generate.py, seed 503).
-- meta
do
  local mt = {}
  local function val(x) if type(x) == 'table' then return rawget(x, 'v') end return x end
  mt.__add = function(a, b) return 'add(' .. tostring(val(a)) .. ',' .. tostring(val(b)) .. ')' end
  mt.__index = function(t, k) return 'missing ' .. tostring(k) end
  mt.__newindex = function(t, k, v) rawset(t, k, v .. '!') end
  mt.__eq = function(a, b) return rawget(a, 'v') == rawget(b, 'v') end
  mt.__lt = function(a, b) return rawget(a, 'v') < rawget(b, 'v') end
  mt.__le = function(a, b) return rawget(a, 'v') <= rawget(b, 'v') end
  mt.__concat = function(a, b) return 'cat' end
  mt.__unm = function(a) return 'neg' end
  mt.__call = function(self, x) return 'called ' .. tostring(x) end
  local a = setmetatable({v = 4}, mt)
  local b = setmetatable({v = 3}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "ann"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- globals
do
  gx = 97
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- compare
do
  print(0.1880476735 < 0.2, 0.1880476735 <= 0.2, 0.1880476735 == 0.2)
  print(807.3755 < 0.07, 807.3755 <= 0.07, 807.3755 == 0.07)
  print(1e-45 < 33554433, 1e-45 <= 33554433, 1e-45 == 33554433)
  print(197.90666 < 2, 197.90666 <= 2, 197.90666 == 2)
  print("Bb" < "9A_", "Bb" <= "9A_", "Bb" == "9A_")
  print("cB" < "b_99", "cB" <= "b_99", "cB" == "b_99")
  print("" < "", "" <= "", "" == "")
end
