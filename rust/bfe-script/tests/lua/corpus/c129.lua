-- Generated differential program 129 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 1 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(1), counter(18)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- format
do
  print(pcall(string.format, "[%+E]", 0.89773431790))
  print(pcall(string.format, "[% 5g]", 2/3))
  print(pcall(string.format, "[%-3u]", 123456))
  print(pcall(string.format, "[%-15o]", -98765))
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%17c]", 48))
  print(pcall(string.format, "[%0s]", "xxxxxxxxxxxx"))
  print(pcall(string.format, "[%+ 19i]", 123456))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
