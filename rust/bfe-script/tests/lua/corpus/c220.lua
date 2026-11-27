-- Generated differential program 220 (generate.py, seed 503).
-- coerce
do
  print(3 .. "x")
  print(1/0 .. "")
  print(1e15 .. "")
  print("5" + "0.25")
  print("10" + 5)
  print("0x10" + 0)
  print("7" * "6")
  print(0.1 .. "")
  print("-inf", tonumber("-inf"))
  print("+3", tonumber("+3"))
  print(".5e1", tonumber(".5e1"))
  print("1 2", tonumber("1 2"))
  print(".", tonumber("."))
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print(9, tonumber("8384", 9))
  print(25, tonumber("25na", 25))
  print(32, tonumber("e0", 32))
  print(26, tonumber("p", 26))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- patterns
do
  print(pcall(string.find, "cya-1[y_-_b1", "%b()", 3))
  print(pcall(string.find, "[% .[x(]y %  ", "%a+"))
  print(pcall(string.find, "()_.09yyy)", "[%.%-]"))
  print(pcall(string.find, ",c0by]9]]a", "%a+", 1))
  print(pcall(string.find, "b(0 ", "%b()"))
  print(pcall(string.find, "9,.]9_1c_", "%s", 1))
  print(pcall(string.gsub, "b,[(.0b90.9x-a_9)", "%f[%w]%w+", "<%0>", 1))
  print(pcall(string.gsub, "", "%f[%w]%w+", "<%0>", 1))
  print(pcall(string.gsub, "", "x*", function() return 7 end, 0))
  do local out = {} for a, b in string.gfind("(.1,x )c-", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(7))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
