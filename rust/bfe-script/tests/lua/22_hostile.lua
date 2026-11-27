-- Hostile workloads for the work budget and memory limit tests. Each global
-- function loops until the VM stops it.
function find_plain()
  local s = string.rep("a", 2^16)
  local p = string.rep("a", 2^15) .. "b"
  while true do local r = string.find(s, p, 1, true) end
end
function class_scan()
  local cls = "[" .. string.rep("a", 2^14) .. "]"
  local s = string.rep("b", 2^10)
  while true do local r = string.gsub(s, cls, "") end
end
function backtrack()
  local s = string.rep("a", 2^12)
  while true do local r = string.find(s, ".-.-.-b") end
end
function backref()
  local s = string.rep("ab", 2^12)
  while true do local a, b = string.find(s, "(a?.*)%1") end
end
function sort_comparator()
  local t = {}
  local seed = 7
  for i = 1, 2^12 do seed = math.mod(seed * 16807, 2147483647) t[i] = seed end
  while true do
    local c = {}
    for i = 1, table.getn(t) do c[i] = t[i] end
    table.sort(c, function(a, b) return a > b end)
  end
end
function index_chain()
  local t = {}
  for i = 1, 99 do t = setmetatable({}, {__index = t}) end
  while true do local x = t.missing end
end
function tombstones()
  local t = {}
  for i = 1, 8000 do t["k" .. i] = i end
  for i = 1, 7999 do t["k" .. i] = nil end
  while true do local k = next(t) end
end
function deep_recursion()
  local function r(n) if n == 0 then return 0 end return r(n - 1) + 1 end
  while true do r(4000) end
end
function garbage_churn()
  local n = 0
  while true do n = n + 1 local t = {n, "s" .. n, {}} end
end
function grow_tables()
  local keep = {}
  local i = 0
  while true do i = i + 1 keep[i] = {} end
end
function grow_strings()
  local keep = {}
  local i = 0
  while true do i = i + 1 keep[i] = "string number " .. i end
end
function grow_closures()
  local keep = {}
  local i = 0
  while true do i = i + 1 local j = i keep[i] = function() return j end end
end
function double_string()
  local s = "x"
  while true do s = s .. s end
end
function rep_parts()
  local parts = {}
  local i = 0
  while true do i = i + 1 parts[i] = string.rep("y", 2^14) end
end
function gsub_expand()
  local s = string.rep("a", 2^14)
  while true do s = string.gsub(s, "a", "aa") end
end
function print_flood()
  local line = string.rep("z", 1000)
  while true do print(line) end
end
function pointer_chase()
  local n = 12000
  local p = {}
  for i = 1, n do p[i] = i end
  local seed = 12345
  for i = n, 2, -1 do
    seed = math.mod(seed * 16807, 2147483647)
    local j = math.mod(seed, i) + 1
    p[i], p[j] = p[j], p[i]
  end
  local t = {}
  for i = 1, n - 1 do t[p[i] + 0.5] = p[i + 1] + 0.5 end
  t[p[n] + 0.5] = p[1] + 0.5
  p = nil
  local k = 1.5
  while true do k = t[k] end
end
function plain_find_long_pattern()
  local p = string.rep("a", 2^19) .. "b"
  while true do local r = string.find("short", p, 1, true) end
end
function find_long_pattern()
  local p = string.rep("a", 2^19) .. "b"
  while true do local r = string.find("short", p) end
end
function gfind_long_pattern()
  local p = string.rep("a", 2^19) .. "b"
  local s = string.rep("c", 64)
  while true do for w in string.gfind(s, p) do end end
end
function gsub_long_pattern()
  local p = string.rep("a", 2^19) .. "b"
  while true do local r = string.gsub("short", p, "") end
end
function format_precision()
  local big = string.rep("a", 2^19)
  while true do local r = string.format("%.1s|%5.2s", big, big) end
end
function concat_long_separator()
  local sep = string.rep(",", 2^19)
  while true do local r = table.concat({}, sep) .. table.concat({"x"}, sep) end
end
function error_long_message()
  local msg = string.rep("e", 2^19)
  while true do pcall(error, msg, 0) pcall(error, msg, 2) end
end
function deep_level_lookup()
  local function down(n) if n == 0 then while true do getfenv(3800) pcall(error, "x", 3800) end end return down(n - 1) + 0 end
  down(3900)
end
function weak_mode_scan()
  local mode = string.rep("x", 2^18) .. "k"
  local keep = {}
  for i = 1, 64 do keep[i] = setmetatable({}, {__mode = mode}) end
  while true do collectgarbage() end
end
