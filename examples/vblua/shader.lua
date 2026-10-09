-- shader.lua — safe WGSL pass management (Phase 9). Needs document.write.
-- New passes start DISABLED (inert until enabled). Sources pass the host
-- static validator at set time; GPU compile still happens host-side.
local ok = vblua.shader.validate([[
  @fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(uv.x, uv.y, 1.0, 1.0);
  }
]])
vblua.log("template valid: " .. tostring(ok))

local p = vblua.shader.create(nil, "My Effect")
vblua.shader.set_uniforms(p, { 0.0, 0.5, 0.0, 1.0 })
vblua.log("passes: " .. #vblua.shader.passes())
-- vblua.shader.set_enabled(p, true)  -- enable when the source is ready
return p
