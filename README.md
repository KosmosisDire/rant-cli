# rant

The command line tool for Rant. It finds the packages and nodes in a workspace, builds
them in order, starts and stops nodes and groups of nodes, and reads and writes the mesh.

## Use

```
rant init                      # make this folder a workspace
rant build                     # build every package, dependencies first
rant start group nav speed=2   # start a group of nodes, rant start group nav --help lists its params
rant ls                        # the running nodes and what they talk through
rant sub odom                  # print a topic
rant stop group nav
```

`rant --help` lists every command.

## Build

Needs CMake 3.21, a C++17 compiler, Rust (stable, through rustup) and git.

```
cmake --preset windows        # or linux, macos
cmake --build --preset windows
ctest --preset windows
```

The binary lands in `bin/`. The `-local` presets build against `../rant` instead of the
pinned Rant release.
