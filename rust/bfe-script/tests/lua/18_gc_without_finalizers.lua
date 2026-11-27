-- NewUD(id) is a host function returning a userdata with a fresh metatable.
-- `__gc` metamethods are never called; weak entries clear in full collections.
count = 0
for i = 1, 120 do
  local u = NewUD(i)
  getmetatable(u).__gc = function(x) count = count + 1; error("not reached") end
end
collectgarbage()
collectgarbage()
print("finalizers run", count)
local wv = setmetatable({}, {__mode = "v"})
local wk = setmetatable({}, {__mode = "k"})
wv[1] = {}
wv[2] = "strings stay"
wk[{}] = 1
local kept = {}
wk[kept] = 2
collectgarbage()
print("weak value", wv[1] == nil, wv[2])
local k, v = next(wk)
print("weak key", k == kept, v, next(wk, k) == nil)
