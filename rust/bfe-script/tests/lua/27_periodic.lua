-- A routine callback beside a large live table: it formats a marker and reads one
-- cached entry, so collection is the only work that grows with the table.
local cache = {}

function setup(n)
  for i = 1, n do
    cache[i] = i
  end
end

function step(i)
  local label = "marker" .. i
  return cache[1]
end

-- A table with a large key index to drop, for destruction in pieces.
function build_index(n)
  local t = {}
  for i = 1, n do
    t["key" .. i] = i
  end
  return t
end

function churn(i)
  local t = { "a" .. i, "b" .. i }
  return t[1]
end
