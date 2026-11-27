-- Generated differential program 098 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 10, 3, 0.1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 24 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 8 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
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
  local obj = setmetatable({name = "cy"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- tables
do
  local t = {5, -16, 32, 12, -13, 48, 1, -47, 31}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, 0)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 128)
  print(table.getn(t), table.concat(t, ' '))
  table.sort(t)
  print(table.concat(t, ' '))
  table.sort(t, function(a, b) return a > b end)
  print(table.concat(t, ' '))
  print(pcall(table.concat, t, '-', 2, 3))
  print(table.concat({}, 'x'), table.concat(t, '', 3, 2))
  local u = {n = 2, 'a', 'b', 'c'} print(table.getn(u))
  table.setn(u, 1) print(table.getn(u))
  print(unpack({1, 2, nil, 4}))
  table.foreachi(t, function(i, v) if i <= 3 then print(i, v) end end)
  local w = {"b", "date", "a", "lime", "apple", "plum"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- arith
do
  do local v = ((-682) + (((0.59646960624) * (33554433)) * ((0.5963273) * (0.2023551))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((8e-13) + (((2^-29) * (-32.684)) * (6)))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((499.78022) + (56e-34)) * ((97e0) + (-26.742))) * (((-8.555) + (2^14)) - ((982) / (24e19))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((23e30) - (((0.78) / (2^4)) * ((8388609) + (1.17549435e-38))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((7e-11) / (((160.7620) * (-45.969)) + (2147483647)))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (((-391) / (2^13)) / (((2^-1) * (2)) + (2/3)))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((-8.605) + (0.3)) * ((2) * (1/3))) * (426.72736))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = ((((16777216) / (33554433)) + ((0.2) + (2^25))) * (((2^13) * (-285)) + ((2^-18) - (-32.325))))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(2^0))
  print('ceil', math.ceil(0.418699819))
  print('abs', math.abs(0.3))
  print('sqrt', math.sqrt(3))
  do local m = math.mod(-214, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(3.4e38, 6, 2/3), math.max(2^-14, 755.82842))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, -19))
  print('pow', 10 ^ -2)
end
