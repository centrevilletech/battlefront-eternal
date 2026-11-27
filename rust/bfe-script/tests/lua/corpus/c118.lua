-- Generated differential program 118 (generate.py, seed 503).
-- globals
do
  gx = 54
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- patterns
do
  print(pcall(string.find, "[a1y_", "%x+"))
  print(pcall(string.find, "y c_ax01-x ", "()", 20))
  print(pcall(string.find, "zca0,)0]1. [", "^%s*(.-)%s*$", -4))
  print(pcall(string.find, ",,.z[](-.b])%(y", "(%d+)", 3))
  print(pcall(string.find, "1z-0,.](9.ccy", "(.)%1", 3))
  print(pcall(string.gsub, "z(9b-(a(.bx_", "%s", "#", 2))
  print(pcall(string.gsub, "[b_)bb001", "^(%w+)", "%%", 0))
  print(pcall(string.gsub, "ab%zc", "(.)%1", function(c) return nil end, 0))
  do local out = {} for a, b in string.gfind(")]]b ,,%]", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print(3 .. "x")
  print(1/0 .. "")
  print("  12  " + 0)
  print(-0 .. "")
  print(16777217 .. "")
  print("0x10" + 0)
  print("7" * "6")
  print(-"2")
  print("12", tonumber("12"))
  print(".5e1", tonumber(".5e1"))
  print("inf", tonumber("inf"))
  print("1e", tonumber("1e"))
  print(".", tonumber("."))
  print("0x", tonumber("0x"))
  print(" 12 ", tonumber(" 12 "))
  print(3, tonumber("2", 3))
  print(3, tonumber("0", 3))
  print(18, tonumber("d8911", 18))
  print(34, tonumber("4s", 34))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
