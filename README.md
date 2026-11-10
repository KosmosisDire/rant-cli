# rant-cli

The command line tool for Rant, run as `rant`. It finds the packages and nodes in a
workspace, builds them in order, starts and stops nodes and groups of nodes, and reads and
writes the mesh.

## Install

```
curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh                 # Linux, macOS
irm https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.ps1 | iex                     # Windows
```

Each lists what it will do, then a checklist of the CLI's skill, the packages and the Arduino libraries to install,
which is the only question: the `rant` command goes to `~/.rant/bin`, that folder onto PATH, and
`rant setup` adds tab completion. Later, `rant update` moves it and
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
rant record 'Cam/*' Odom       # record them until Ctrl-C
rant play 2026-11-05_09-30-00Z --speed 2  # send a recording again at twice the speed
rant log Nav -f                # follow what its nodes print
rant stop Nav
rant enable Nav                # run it now and every time this computer starts
rant add                       # add Rant to the package here, or rant add math cam
rant setup                     # tab completion for your shells
rant install explorer          # a package, all of them at https://pack.rantlib.dev
rant explore                   # open the explorer
```

`rant --help` lists every command.

## AI coding agents

rant-cli carries a skill that teaches Claude Code, Codex, Cursor, Gemini CLI, GitHub Copilot and
OpenCode to use it. `rant setup skill` links it as `~/.claude/skills/rant-cli` for Claude Code and
`~/.agents/skills/rant-cli` for the others, and `rant update` keeps it current. `rant add` offers
the skill for writing nodes with Rant.

## Build

Needs Rust (stable, through rustup) and a C compiler, which builds Rant in.

```
cargo build --release
cargo test
```

The binary lands in `target/release/`. It builds against the Rant checked out beside it, in
`../rant`.
