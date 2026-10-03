-- hello.lua — minimal VBLua script (Phases 1-3).
-- Run: paste into the VBLua console, or VbRuntime::execute("hello.lua", code).
print("Hello from VBLua")
print(vblua.version())
print(vblua.api_version())
vblua.log("platform: " .. vblua.platform())
return vblua.math.lerp(0, 10, 0.5)
