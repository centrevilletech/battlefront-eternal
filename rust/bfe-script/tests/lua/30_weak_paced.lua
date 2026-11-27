-- Weak tables mutated between ticks while paced full collections clear them in slices.
-- Reads, copies, moves into the array part, deletions and mode changes all race the
-- clearing; verify() fails on any value that a sweep has freed.
W = setmetatable({}, { __mode = "v" })
K = setmetatable({}, { __mode = "k" })
S = {}
local seed = 1
local function rnd(n)
  -- Products stay below 2^24, exact in single precision.
  seed = math.mod(seed * 171, 30269)
  return math.mod(seed, n) + 1
end

function setup(n)
  for i = 1, n do
    local t = { i }
    W[i] = t
    W["s" .. i] = t
    K[t] = i
    if math.mod(i, 3) == 0 then S[i] = t end
  end
end

function poke(ops, n)
  for j = 1, ops do
    local r = rnd(12)
    local i = rnd(n)
    if r == 1 then S[i] = W[i]
    elseif r == 2 then W[i] = { i }
    elseif r == 3 then W[i] = nil
    elseif r == 4 then W["s" .. i] = W[i]
    elseif r == 5 then S[i] = nil
    elseif r == 6 then
      local t = { i }
      K[t] = i
      if rnd(2) == 1 then S[i] = t end
    elseif r == 7 then
      local skip = rnd(64)
      for k, v in pairs(W) do
        skip = skip - 1
        if skip == 0 then S[rnd(n)] = v break end
      end
    elseif r == 8 then W[n + rnd(n)] = { i }
    elseif r == 9 then
      local v = W["s" .. i]
      if v then S[i] = v end
    elseif r == 10 then
      if rnd(40) == 1 then setmetatable(W, { __mode = "k" }) setmetatable(W, { __mode = "v" }) end
    elseif r == 11 then
      for k in pairs(K) do if rnd(8) == 1 then S[rnd(n)] = k end break end
    else
      if rnd(20) == 1 then collectgarbage() end
    end
  end
end

local function check(v, where)
  if type(v) ~= "table" or type(v[1]) ~= "number" then error("bad value in " .. where) end
end

function verify()
  local strong = 0
  for k, v in pairs(S) do check(v, "S") strong = strong + 1 end
  local weak = 0
  for k, v in pairs(W) do check(v, "W") weak = weak + 1 end
  local keys = 0
  for k, v in pairs(K) do check(k, "K") keys = keys + 1 end
  return strong, weak, keys
end

-- After a complete full collection every value left in W and every key left in K
-- must be strongly reachable from S.
function settled()
  local held = {}
  for k, v in pairs(S) do held[v] = true end
  local stray = 0
  for k, v in pairs(W) do if not held[v] then stray = stray + 1 end end
  for k in pairs(K) do if not held[k] then stray = stray + 1 end end
  return stray
end
