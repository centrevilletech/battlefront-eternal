-- Native functions that call native functions: each level recurses on the
-- native stack, so each one counts toward the re-entry limit, as a Lua
-- callback does.
function pcall_chain()
  local fs = {}
  for i = 1, 200 do fs[i] = pcall end
  return pcall(unpack(fs))
end
function tostring_loop()
  local t = setmetatable({}, {})
  getmetatable(t).__tostring = tostring
  return tostring(t)
end
function xpcall_chain()
  local function again() return xpcall(again, again) end
  return again()
end
function shallow_chain()
  local fs = {}
  for i = 1, 20 do fs[i] = pcall end
  return pcall(unpack(fs))
end
