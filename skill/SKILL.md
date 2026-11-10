---
name: rant-cli
description: The rant command line tool and its config files: workspaces, packages, groups, building, running, services, installing packages, adding libraries and reading the mesh. Use when a project has workspace.rant, package.rant or *.run files, or the user asks to make, build, run, stop, debug or inspect Rant nodes, topics, variables, functions or tasks, or to install Rant packages.
---

# Rant

Rant connects programs ("nodes") on one machine or a network ("the mesh"). Nodes find each
other on their own and share four kinds of entity: topics, variables, functions and tasks.
The `rant` command makes projects, builds them, starts and stops nodes, and reads and
writes the mesh. Full docs: https://docs.rantlib.dev/llms.txt (fetch the page you need).

## Skills

This skill is the CLI. Each part of Rant brings its own:

- `rant`: writing nodes in code. `rant add` and `rant new` offer it as they add Rant to a
  package.
- `bridge`, `explorer`, `manager` and `hmi`: each package's own, which `rant install <name>`
  offers.

## First

- Read the project before changing it. Keep its layout, names and style. The rules below
  are for new work and for what the project leaves open.
- `rant --help` and `rant <command> --help` are the truth for every command and flag.
- Nodes on another domain are invisible. Never start test nodes on the domain a real
  system uses. Use `--domain N` or a workspace `domain` for your own runs.
- Rant's ports: discovery on UDP 6730, the bridge on TCP 6731, the web explorer on TCP 6732.

## Layout

```
cell/                     workspace: the folder holding workspace.rant
  workspace.rant
  Cell.run                a group: which nodes run together, and how
  drivers/                a package: CMakeLists.txt, pyproject.toml, a .csproj or Cargo.toml that uses Rant
    CMakeLists.txt
    lidar.cpp             a node: one program
  logs/                   each started node's output, <node>.log, read with rant log
  .rant/                  rant's state, never edit, keep out of git
```

A build file that uses Rant makes a package by itself. Add a package.rant beside it only
to override something. A folder may be a workspace and a package, with both files.

Installed packages live in `~/.rant/packages`, built already, and every workspace sees them. A
package of the same name in the workspace hides one.

`rant install <name>` looks in `~/.rant/config.rant`, then the official package database.
`rant install someone/rant-tool` or the repo's link installs any GitHub repo. config.rant is
written by hand:

```hcl
paths        = ["C:/src/explorer-rant"]  # folders with package.rant, their run files and bin/
repos        = ["someone/rant-tool"]     # their GitHub releases
databases    = ["someone/rant-packages"]
agent_skills = "auto"                    # give a package's or library's skill unasked, or "ask"
```

## Commands

| goal | command |
|---|---|
| new project | `rant new workspace [folder] --lang cpp\|python\|csharp\|rust` (talker, listener, group) |
| new package or node | `rant new package [folder] --lang L`, `rant new node [name]`, `rant new group [name]` |
| build | `rant build [package...]`, `--dry-run` shows order and commands, `--clean` removes the build output first |
| run | `rant run <Group> key=value...`, one node alone with `rant run node <node>`, `--dry-run` first. It builds first (`--no-build` skips that), waits for the nodes to come up (`--wait S`), watches them a moment (`--watch S`) and fails with the log of one that exited |
| stop, restart | `rant stop [name]` and `rant restart [name]` (nothing named: all this workspace started), `rant stop --all` (every node on this machine). restart stops, builds, then starts. A name is the group, else the node, and `rant stop node <node>` takes the node alone. They find a name wherever rant started it on this machine, and fail without a terminal when several workspaces run it |
| services, at every boot | workspace.rant lists services as `service "Hmi" { project = "line-hmi" }` blocks, a group with its param values. `rant enable [Group key=value...]` runs them now and at every boot (`--login`: at login), asking for admin once. `rant disable [Group]` undoes it. `rant ls services` shows them |
| node output | `rant log [name...]`, `-f` to follow, `-n N` lines, a group shows all its nodes, `rant log node <node>` the node alone |
| see the mesh | `rant ls <nodes\|entities\|topics\|variables\|functions\|tasks\|types\|packages\|groups\|sources\|services\|recordings> [pattern]`, `rant ls` alone lists every entity, `rant ls -h` the kinds, `rant ls nodes --no-mesh` for only what this workspace started |
| explain one thing | `rant info [kind] <name>`, a type shows its fields and who uses it, a node its source file |
| read and write | `rant sub <topic\|var> -n 1`, `rant pub <topic\|var> <value>`, `rant call <fn> [value]`. `--lines` (and any pipe) prints a record a line instead of redrawing, `--value-only` only the values. `rant sub <fn\|task>` prints a record per step of every call any node makes to it |
| record and play | `rant record <name...>` writes the names (`*` and `?` match, `--ignore` leaves some out) to an MCAP file until Ctrl-C or `--duration S`, `-o file` names it. `rant ls recordings` lists them. `rant play <name>` sends one again: `--speed F`, `--start S`, `--loop`, `--only`, `--ignore`. It refuses names a live node sends, so play under `--prefix Replay` or on another `--domain`. Calls are recorded but not played |
| libraries in packages | `rant add [library [folder]]` adds a library to the package here or in folder, or moves it to the latest release, Rant when none is named (`rant add`, `rant add math cam`). `--version V` picks a release. In an Arduino sketch folder it unpacks the library into the Arduino sketchbook (`ARDUINO_DIRECTORIES_USER` names it when rant cannot find it), in a PlatformIO project it pins the release's zip in `lib_deps` and sets C++17. In a plcx workspace it puts the PLCnext Engineer library in `libs/` and references it in `pcwex.toml`, and beside a plain `.pcwex` it puts it in Engineer's user libraries. `rant ls libraries` shows what each package, the sketchbook and the user libraries use, `--available` what there is |
| packages | `rant install <package\|repo>...` (`--version V`, `--from PATH` for a local build folder or a .tar.gz). `rant install` alone is a checklist for a person, so an agent names the packages. `rant remove <package>...`, `rant ls packages --available` for what there is, `rant info package <name>` before installing. A package's skill goes to each AI coding agent found. Libraries such as Rant go in with `rant add`. Stop a package's nodes before install or remove |
| update | `rant update [package\|library...]`: the CLI, the installed packages and their skills, then every library the workspace's packages, the Arduino sketchbook and Engineer's user libraries use |
| setup | `rant setup` adds tab completion for the shells found (or the ones named), `rant setup skill` gives this skill to the AI coding agents found. `rant uninstall` removes rant and what it added, never workspaces |
| visual debugger | `rant explore`, after `rant install explorer` |
| VS Code | `rant install vscode` (`--editor cursor` for another editor, `--profile NAME` for a profile), `rant remove vscode`. Its Mesh view joins the mesh itself, or through the bridge the setting `rant.bridgeUrl` names |

Values on the command line are forgiving JSON: `{x: 1, y: 2}`, `[1, 2]`, or field words
like `position.x=1`. Add `--json` or `--csv` to any command for output a program can read,
`--lines` for a record a line instead of a view that redraws (run, stop and restart print a
row per node as it changes), `-y` to answer its questions. stdout carries only that data:
notes, questions, progress and build output go to stderr.

## Config files

All three are HCL with every setting at the top level. workspace.rant at the workspace root,
empty for the defaults:

```hcl
logs   = "logs"              # where node output goes
recordings = "recordings"    # where rant record writes
ignore = ["third_party/**"]  # folders discovery skips
domain = 7                   # placement, see below
```

package.rant in a package: `name`, `description`, `build` (commands replacing the default),
`depend` (package names built first), `ignore`, `python` (the interpreter), and
`node "name" { run = "./tool --flag" }` for programs rant cannot find itself. Add
`expect_mesh = false` to one that never joins the mesh, so `rant run` waits only for it to
start.

A package others install names its release files `<name>-<version>-<platform>.tar.gz` (bin/,
package.rant and the run files), `<name>-<version>.vsix` and `<name>-<version>-skill.zip`.
`version` is the release tag without its v, `platform` one of win-x64, win-arm64, linux-x64,
linux-arm64, osx-x64 and osx-arm64.

A group is `<Name>.run`:

```hcl
description = "Both arms"
param "speed" {
  type    = float              # string, int, float, bool or path
  default = 1
}
include "Arm" {                # another group, package/Arm when unclear, with its param values
  side        = "left"
  node_prefix = "left"
}
node "planner" {               # the label names the node to run, package/name when unclear
  name = "planner"             # the name on the mesh, the node's own by default
  args = "--speed ${param.speed}"
  env  = { LEVEL = "debug" }
}
```

Expressions may use `param.<name>` in groups and `path.file`, `path.package` and
`path.workspace` anywhere.

A `path` param reaches the node as a full path, counted from the folder `rant` runs in on the
command line and from the group file's folder in a group.

Placement: `domain`, `prefix` and `node_prefix` may go in workspace.rant, package.rant, a
group's top level, an `include` and a group `node`, the innermost winning. `prefix` goes in
front of every entity name a node makes, `node_prefix` in front of its node name. Place nodes
this way, never with names or domains hard coded in the program.

`restart` says whether a node that exits starts again: `"never"` (the default),
`"on-failure"` or `"always"`. It goes where placement does.

## Names

- Nodes are snake_case and named like their file: `arm_controller.py`, a C++ target
  `arm_controller`, a C# `ArmController.csproj` with `<AssemblyName>arm_controller`, a Rust
  `src/bin/arm_controller.rs`. Name a node for what it is (`lidar`, not `lidar_node`). One
  program per node.
- Groups are PascalCase: `Nav.run`, run as `rant run Nav`. Names match without case or
  underscores, so the group `Nav` and the node `nav` are one name and the group wins:
  `rant stop nav` stops the group, `rant stop node nav` the node alone.
  A group is how a system starts, so give every system one, even of one node.
  `rant new group` names them so.
- `rant run` and `rant build` note each group or node name that breaks this. Entities, types
  and fields are named in code, as the skill `rant` says.
- Add Rant and other libraries with `rant add`, never by hand, and keep packages current with `rant update`.

## Debugging

1. `rant ls nodes` shows what runs and what could, `rant ls entities` what they talk through.
2. Not seeing a node: check the domain on both sides (`--domain`, `domain`, `RANT_DOMAIN`).
3. A node that died: read `logs/<node>.log`.
4. Wrong config: `rant run X --dry-run`, `rant build --dry-run`, `rant info group X`.
5. Wrong data: `rant info <topic>` shows its type and who talks, `rant sub <topic> -n 1`
   shows a message. A type mismatch is refused, not silently read.
6. "not built yet": run `rant build`.
