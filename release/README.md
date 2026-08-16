# Release downloads

This folder is the **download drop** for packaged desktop builds.

Produce a host release with:

```bash
./packaging/make_desktop_release.sh
```

That will:

1. `cargo build --release` for `vadadee-berry` and `vadadee-mcp-stdio`
2. Bundle binaries + logos under `dist/desktop/`
3. Write a versioned archive here, e.g.  
   `vadadee-berry-0.1.0-linux-x86_64.tar.gz`  
   plus `.sha256` and a `vadadee-berry-latest-linux-x86_64.tar.gz` symlink

Host or attach these files for users to download.

## Logos

`assets/logo.svg` is 200×100:

| Tile | Region | File | Product |
|------|--------|------|---------|
| Studio | left 100×100 | `assets/icon_studio.png` | Desktop app window / launcher icon |
| MCP | right 100×100 | `assets/icon_mcp.png` | MCP product mark |
