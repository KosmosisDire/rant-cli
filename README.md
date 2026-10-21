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
folder onto PATH, then `rant setup` adds tab completion. Later, `rant update` moves it and
the installed packages to their newest releases, and `rant uninstall` removes all of it again,
leaving workspaces as they are.

## Use

```
rant new workspace demo --lang python  # a workspace with a talker, a listener and a group of both
rant init                      # or make this folder an empty workspace
rant new package cam --lang cpp  # a package with one node, Rant added (or python, csharp)
rant build                     # build every package, dependencies first
rant run Nav speed=2           # run a group of nodes, rant run Nav --help lists its params
rant run node talker           # or one node alone
rant ls nodes                  # the running nodes, and those that could start
rant sub Odom                  # print a topic
rant log Nav -f                # follow what its nodes print
rant stop Nav
rant enable Nav                # run it now and every time this computer starts
rant lib update                # move every package here to the newest Rant
rant setup                     # tab completion for your shells
rant install explorer          # a package, all of them at https://pack.rantlib.dev
rant explore                   # open the explorer
```

`rant --help` lists every command.

## AI coding agents

`rant setup skill` installs a skill that teaches Claude Code, Codex, Cursor, Gemini CLI,
GitHub Copilot and OpenCode to use Rant and the CLI: `~/.claude/skills/rant` for Claude Code,
`~/.agents/skills/rant` for the others. `rant update` keeps it current. Delete that folder
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
