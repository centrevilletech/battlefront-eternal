-- Generated differential program 030 (generate.py, seed 503).
-- coerce
do
  print("  12  " + 0)
  print(".5" .. 1)
  print(1/0 .. "")
  print(1e100 .. "")
  print("0x10" + 0)
  print(-1/0 .. "")
  print("1e3" / 4)
  print(3 .. "x")
  print("1 2", tonumber("1 2"))
  print(".", tonumber("."))
  print(".5e1", tonumber(".5e1"))
  print("-7.5", tonumber("-7.5"))
  print("12", tonumber("12"))
  print("  0x10  ", tonumber("  0x10  "))
  print("0x", tonumber("0x"))
  print(20, tonumber("870", 20))
  print(17, tonumber("a7", 17))
  print(9, tonumber("024", 9))
  print(17, tonumber("d3c", 17))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(14))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 6 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(4), counter(16)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- patterns
do
  print(pcall(string.find, "y(%0", "z$", 1))
  print(pcall(string.find, "0(a.0.(1b0(0_", "%s", 20))
  print(pcall(string.find, "1,,[_", "z$", -4))
  print(pcall(string.find, "[)_- ,0b", "c-", 1))
  print(pcall(string.find, "(x9-x1%0%)9", "^%s*(.-)%s*$", 1))
  print(pcall(string.gsub, "y", "%s", function(c) return nil end, 2))
  print(pcall(string.gsub, " y z(]y,", "[^,]+", "%1-", 1))
  print(pcall(string.gsub, "z-b_%", "x*", "<%0>", 0))
  do local out = {} for a, b in string.gfind("]b,%z90]%", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
