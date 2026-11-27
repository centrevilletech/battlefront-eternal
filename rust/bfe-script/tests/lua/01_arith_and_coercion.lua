-- Arithmetic, IEEE single-precision formatting, and string/number coercions.
print("basic", 1 + 2, 10 - 3, 4 * 5, 15 / 4, -7)
print("float", 1 / 3, 0.1, 1.5, 0.0, 0.0 * -1, -0.0 == 0.0)
print("scale", 1e13, 1e14, 1e-4, 1e-5, 31415926)
print("special", 1 / 0, -1 / 0)
print("pow", 2 ^ 10, 9 ^ 0.5, 2 ^ -3)
print("coerce", "12.5" + "3.5", "10" - 4, "3" * "7", "9" / "2", -"4.5", "2" ^ "6")
print("concat", "a" .. 1 .. "b" .. 2.5 .. "c")
print("tonum",
    tonumber("  -3.5e2  "),
    tonumber(".5"),
    tonumber("5."),
    tonumber("0x10"),
    tonumber("ff", 16),
    tonumber("10110", 2),
    tonumber("z", 36),
    tonumber("123a"),
    tonumber("")
)
local ok, err = pcall(function() return "abc" + 1 end)
print("bad_arith", ok, type(err))
