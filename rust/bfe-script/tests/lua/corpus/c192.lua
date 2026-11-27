-- Generated differential program 192 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(0), counter(16)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- format
do
  print(pcall(string.format, "[%g]", 0.1))
  print(pcall(string.format, "[%#7i]", 0))
  print(pcall(string.format, "[% -18.7f]", 393.35874))
  print(pcall(string.format, "[%0X]", 7))
  print(pcall(string.format, "[%+ 2u]", 65535))
  print(pcall(string.format, "[% u]", 255))
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(pcall(string.format, "[%6.2f]", 2^-5))
  print(pcall(string.format, "[% #6f]", 461))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- compare
do
  print(-0.32 < 3.4e38, -0.32 <= 3.4e38, -0.32 == 3.4e38)
  print(192.18207 < 9, 192.18207 <= 9, 192.18207 == 9)
  print(2/3 < 259, 2/3 <= 259, 2/3 == 259)
  print(7 < -954, 7 <= -954, 7 == -954)
  print(-1.811 < 9, -1.811 <= 9, -1.811 == 9)
  print("" < "_9", "" <= "_9", "" == "_9")
  print("b" < "", "b" <= "", "b" == "")
  print("bb_a" < "90B", "bb_a" <= "90B", "bb_a" == "90B")
end
