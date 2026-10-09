-- params.lua — generic node parameters (Phase 6). Needs document.write.
-- Values cross the VbValue boundary: numbers/strings in, clear errors otherwise.
local v = vblua.graph.create("Value")
vblua.graph.set_param(v, "value", 42)

local e = vblua.graph.create("ExprX")
vblua.graph.set_param(e, "expr", "x*2+1")

vblua.log("value=" .. vblua.graph.get_param(v, "value"))
vblua.log("expr=" .. vblua.graph.get_param(e, "expr"))
-- Discover keys: vblua.graph.params(v) -> {value = "number"}
return vblua.graph.get(e).params.expr
