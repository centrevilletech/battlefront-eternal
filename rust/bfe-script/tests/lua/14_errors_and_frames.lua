-- Protected-error semantics, raw byte error objects, call frames, getobjname, and xpcall pre-unwind.
local P = pcall

print("gsub_pcall", P(string.gsub, "a", "a", {}))
print("err_pcall", P(error, "E"))

local function tail_err() return error("E") end
print("tail_err", P(tail_err))

local _, e_neg = P(error, 5, -1)
print("err_neg", type(e_neg), e_neg)

local _, e_raw = P(function() error(string.char(255)) end)
print("err_raw_byte", string.byte(e_raw, -1))

print("assert_msg", P(function() assert(false, "e") end))
print("assert_def", P(function() assert(false) end))

local function add_local(x) return x + 1 end
print("arith_local", P(add_local, {}))

print("cmp_same", P(function() return {} < {} end))
print("cmp_diff", P(function() return {} < 1 end))
print("arg_len", P(function() string.len({}) end))
print("for_bad", P(function() for i = {}, 2 do end end))

local env = { error = error }
local f_env = function() error(false) end
setfenv(f_env, env)
local _, got_env = xpcall(f_env, function() return getfenv(3) == env end)
print("xpcall_env", got_env)

print("call_field", P(function() local t = {}; return t.foo() end))
print("call_method", P(function() local t = {}; return t:bar() end))
print("call_global", P(function() return nope() end))
print("index_local", P(function() local a = nil; return a.x end))
print("concat_local", P(function() local a = {}; return "hi" .. a end))
print("xpcall_bad_h", xpcall(function() error("x") end, 123))
print("xpcall_err_h", xpcall(function() error("x") end, function() error("y") end))
