-- Generated differential program 022 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%07e]", 982.1765))
  print(pcall(string.format, "[%#+7x]", -98765))
  print(pcall(string.format, "[%-13.0G]", 16777215))
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%-.3e]", -554))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, " [() )_", ".-b"))
  print(pcall(string.find, "_ba[_9-9", "[%.%-]", 20))
  print(pcall(string.find, "]( z9 z([)[y%%c", "%p"))
  print(pcall(string.find, ")%..]0)_(", "(%a)(%d)", -4))
  print(pcall(string.gsub, "x(a,( a %[z191,c,", "%s", "#", 0))
  print(pcall(string.gsub, "0 x,91])--]%]", "(a*)", function() return false end, 2))
  print(pcall(string.gsub, ")[00[._% ", "(.)%1", "%1-", 1))
  do local out = {} for a, b in string.gfind("", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 1 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(0), counter(13)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(6))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
