-- Generated differential program 011 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%q]", "plain"))
  print(pcall(string.format, "[%+19.5G]", 2^18))
  print(pcall(string.format, "[%18s]", "tab\9here"))
  print(pcall(string.format, "[%x]", 7))
  print(pcall(string.format, "[%#3f]", 8388609))
  print(pcall(string.format, "[%9e]", 0.2))
  print(pcall(string.format, "[%5.2s]", "xxxxxxxxxxxx"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- globals
do
  gx = 7
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
  print(33554433 < 11e-33, 33554433 <= 11e-33, 33554433 == 11e-33)
  print(-375 < 8388609, -375 <= 8388609, -375 == 8388609)
  print(-24.571 < 16777217, -24.571 <= 16777217, -24.571 == 16777217)
  print(60e-26 < 895.18707, 60e-26 <= 895.18707, 60e-26 == 895.18707)
  print("90Ab" < "bccA", "90Ab" <= "bccA", "90Ab" == "bccA")
  print("" < "bc_a", "" <= "bc_a", "" == "bc_a")
  print("9abA" < "", "9abA" <= "", "9abA" == "")
end
