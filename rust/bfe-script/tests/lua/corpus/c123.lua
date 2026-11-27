-- Generated differential program 123 (generate.py, seed 503).
-- compare
do
  print(-43.479 < 132.46651, -43.479 <= 132.46651, -43.479 == 132.46651)
  print(4294967295 < 2^24, 4294967295 <= 2^24, 4294967295 == 2^24)
  print(66e0 < 2^7, 66e0 <= 2^7, 66e0 == 2^7)
  print(512 < 2147483647, 512 <= 2147483647, 512 == 2147483647)
  print("0_90" < "_bbb", "0_90" <= "_bbb", "0_90" == "_bbb")
  print("99a" < "A", "99a" <= "A", "99a" == "A")
  print("bB" < "", "bB" <= "", "bB" == "")
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, -5, 0.1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 31 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 10 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- strings
do
  local s = "0 9"
  print(string.sub(s, -8, -15), string.sub(s, -8), string.byte(s, -15))
  print(string.sub(s, -13, 8), string.sub(s, -13), string.byte(s, 8))
  print(string.sub(s, -15, 15), string.sub(s, -15), string.byte(s, 15))
  print(string.sub(s, -12, 11), string.sub(s, -12), string.byte(s, 11))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 1))
  print(string.char(115))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
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
  local a = setmetatable({v = 3}, mt)
  local b = setmetatable({v = 0}, mt)
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
