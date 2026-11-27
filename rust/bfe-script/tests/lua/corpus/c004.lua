-- Generated differential program 004 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "[c]", "x*", 3))
  print(pcall(string.find, "%(]x%_b0__(", "()", 1))
  print(pcall(string.find, "z ab(((),", "(%d+)", 3))
  print(pcall(string.find, "", "z$"))
  print(pcall(string.find, "[.-y1_9](x0y([-0", "(.)%1", 3))
  print(pcall(string.gsub, "( -a9],0b)_],", "%p", function(c) return nil end))
  print(pcall(string.gsub, "y( yx9]", "[]]", function(c) return nil end, 1))
  print(pcall(string.gsub, "[-a%[b_x,0", "[%a_][%w_]*", "<%0>", 1))
  do local out = {} for a, b in string.gfind("y1[.[1)", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
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
-- arith
do
  do local v = ((8) / ((-4.521) * ((0.252677) - (0.1))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2147483647)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((2^-27) + (((2^-26) * (794.19720)) + ((504.73281) - (1))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((16777217) * (-219)) * (-945)) * (((50.90244) + (2^-14)) * ((900.21044) + (-159))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (0.755931)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(571.67755))
  print('ceil', math.ceil(16777216))
  print('abs', math.abs(213.22774))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(2^27, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(17e-40, 0.0, 70e-11), math.max(-29.580, 0.149505595))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(1, -13))
  print('pow', 1.5 ^ 0.5)
end
-- compare
do
  print(-27.503 < 0.3, -27.503 <= 0.3, -27.503 == 0.3)
  print(710 < 0.7973568, 710 <= 0.7973568, 710 == 0.7973568)
  print(6 < 60.71279, 6 <= 60.71279, 6 == 60.71279)
  print(0.98433 < -1.368, 0.98433 <= -1.368, 0.98433 == -1.368)
  print(16777216 < -4.135, 16777216 <= -4.135, 16777216 == -4.135)
  print(455.32450 < -944, 455.32450 <= -944, 455.32450 == -944)
  print("_" < "90ca", "_" <= "90ca", "_" == "90ca")
  print("c0BA" < "", "c0BA" <= "", "c0BA" == "")
  print("0bb0" < "", "0bb0" <= "", "0bb0" == "")
end
