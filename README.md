# rant-cli

The command line tool for Rant, run as `rant`. It finds the packages and nodes in a
workspace, builds them in order, starts and stops nodes and groups of nodes, and reads and
writes the mesh.

## Install

```
curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh                 # Linux, macOS
irm https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.ps1 | iex                     # Windows
```

Each lists what it will do and asks first: the `rant` command goes to `~/.rant/bin`, that
folder onto PATH, then `rant setup` adds tab completion. Later, `rant --update` moves to the
newest release.

## Use

```
rant new workspace demo --lang python  # a workspace with a talker, a listener and a group of both
rant init                      # or make this folder an empty workspace
rant new package cam --lang cpp  # a package with one node, Rant added (or python, csharp)
rant build                     # build every package, dependencies first
rant start group nav speed=2   # start a group of nodes, rant start group nav --help lists its params
rant ls                        # the running nodes and what they talk through
rant sub odom                  # print a topic
rant stop group nav
rant lib install               # move every package here to the newest Rant
rant setup                     # tab completion for your shells
```

`rant --help` lists every command.

## AI coding agents

`rant setup skill` installs a skill that teaches Claude Code, Codex, Cursor, Gemini CLI,
GitHub Copilot and OpenCode to use Rant and the CLI: `~/.claude/skills/rant` for Claude Code,
`~/.agents/skills/rant` for the others. `rant --update` keeps it current. Delete that folder
to remove it.

## Build

Needs CMake 3.21, a C++17 compiler, Rust (stable, through rustup) and git.

```
cmake --preset windows        # or linux, macos
cmake --build --preset windows
ctest --preset windows
```

The binary lands in `bin/`. The `-local` presets build against `../rant` instead of the
pinned Rant release.
