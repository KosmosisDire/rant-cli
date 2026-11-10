# rant-cli

The command line tool for Rant, run as `rant`: workspaces, builds, nodes, packages and the mesh.

## Install

```
curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh                 # Linux, macOS
irm https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.ps1 | iex                     # Windows
```

`rant --help` lists every command, `rant update` moves rant and its packages to the newest
releases, and `rant uninstall` removes them again.

## Build

```
cargo build --release
cargo test
```
