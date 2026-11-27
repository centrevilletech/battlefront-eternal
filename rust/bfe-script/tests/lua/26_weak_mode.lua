-- The weak mode of a table is read from its metatable's __mode field as the field
-- holds it at each collection (up to its first NUL), however the field was set.
local function count(t)
  local n = 0
  for _ in pairs(t) do n = n + 1 end
  return n
end

local mt = {}
local w = setmetatable({}, mt)
w[{}] = 1
w[{}] = 2
collectgarbage()
print("strong", count(w))

rawset(mt, "__mode", "k")
collectgarbage()
print("weak keys", count(w))

w[{}] = 3
mt.__mode = "\0k"
collectgarbage()
print("after NUL", count(w))

mt.__mode = "kv"
local v = setmetatable({}, mt)
v[1] = {}
v.x = "kept"
v[2] = 5
v.y = {}
collectgarbage()
print("weak both", v[1], v.x, v[2], v.y)

mt.__mode = nil
w[{}] = 4
collectgarbage()
print("mode removed", count(w))

mt.__mode = 7
w[{}] = 5
collectgarbage()
print("number mode", count(w))

local other = {}
setmetatable(w, other)
other.__mode = "k"
collectgarbage()
print("new metatable", count(w))
