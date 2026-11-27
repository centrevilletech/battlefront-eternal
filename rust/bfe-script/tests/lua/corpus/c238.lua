-- Generated differential program 238 (generate.py, seed 503).
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
  local a = setmetatable({v = 0}, mt)
  local b = setmetatable({v = 4}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "cy"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- coerce
do
  print(-0 .. "")
  print(-1/0 .. "")
  print(2^24 + 1)
  print(3 .. "x")
  print(-"2")
  print(" 3e2 " * 1)
  print("7" * "6")
  print("1e3" / 4)
  print("inf", tonumber("inf"))
  print("1e400", tonumber("1e400"))
  print("1 2", tonumber("1 2"))
  print("12", tonumber("12"))
  print(".5e1", tonumber(".5e1"))
  print(" 12 ", tonumber(" 12 "))
  print("3x", tonumber("3x"))
  print(6, tonumber("23 4", 6))
  print(8, tonumber("05", 8))
  print(28, tonumber("bp1a", 28))
  print(6, tonumber("2", 6))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- trig
do
  print('deg', math.deg(10))
  print('log10', math.log10(0.5))
  print('exp', math.exp(10))
  print('sin', math.sin(0.001))
  print('atan2', math.atan2(0.5, 1))
  print('pi', math.pi, math.pi * 2)
end
-- compare
do
  print(724.68460 < 2^-6, 724.68460 <= 2^-6, 724.68460 == 2^-6)
  print(16777216 < -14.32, 16777216 <= -14.32, 16777216 == -14.32)
  print(100.26646 < 2/3, 100.26646 <= 2/3, 100.26646 == 2/3)
  print(880 < 0.447913952, 880 <= 0.447913952, 880 == 0.447913952)
  print(81e-39 < 90e-2, 81e-39 <= 90e-2, 81e-39 == 90e-2)
  print("" < "c", "" <= "c", "" == "c")
  print("0a99" < "B", "0a99" <= "B", "0a99" == "B")
  print("9aBa" < "", "9aBa" <= "", "9aBa" == "")
end
