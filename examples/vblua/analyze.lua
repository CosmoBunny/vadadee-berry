-- analyze.lua — GPU kernel image runtime, milestone 1 (Phases 2-6).
-- Handles are opaque; analysis returns scalars (never whole images).
-- average_brightness runs GPU luminance + reduction when a device exists
-- (see vblua.image.backend()), else the bit-identical CPU oracle.
local img = vblua.image.solid(64, 64, { 1, 0, 0, 1 })
vblua.log("backend: " .. vblua.image.backend())

local b = vblua.image.average_brightness(img)
vblua.log(string.format("brightness: %.4f (Rec.709 red = 0.2126)", b))

local c = vblua.image.region_average(img, 0.25, 0.25, 0.5, 0.5)
vblua.log(string.format("center: %.4f", c))

-- Custom kernels validate now, execute in milestone 2.
local k = vblua.kernel.create({
  name = "sobel",
  language = "wgsl",
  source = "@compute @workgroup_size(8, 8)\nfn main() {}",
  parameters = { strength = 1.0 },
})
vblua.log("kernel: " .. vblua.kernel.info(k).name)
return b
