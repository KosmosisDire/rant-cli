---
name: rant-skill
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
- Rant's ports: discovery on UDP 6730 (each domain on its own multicast group), the bridge
  on TCP 6731, the web explorer on TCP 6732. A node's data port is chosen by the OS.

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
to override something. A folder may be a workspace and a package, with both files. A CMake folder inside a CMake package belongs to it.

Installed packages live in `~/.rant/packages`, built already. Every workspace sees them after
its own packages, and a package of the same name in the workspace hides one. Outside every
workspace `run`, `stop`, `log` and `ls` act on them alone, with logs in `~/.rant/logs`.

`rant install <name>` finds a package in `~/.rant/sources.rant` first, then the official
package database, then the databases sources.rant adds, in order. Each lists GitHub repos, and
the `name` in a repo's root package.rant names its package. `rant install someone/rant-tool`
or the repo's link installs any repo. sources.rant is written by hand and may also list local
build folders, linked so each build counts at once:

```hcl
paths     = ["C:/src/explorer-rant"]     # folders with package.rant, their run files and bin/
repos     = ["someone/rant-tool"]        # their GitHub releases
databases = ["someone/rant-packages"]
```

## Commands

| goal | command |
|---|---|
| new project | `rant new workspace [folder] --lang cpp\|python\|csharp\|rust` (talker, listener, group) |
| new package or node | `rant new package [folder] --lang L`, `rant new node [name]`, `rant new group [name]` |
| build | `rant build [package...]`, `--dry-run` shows order and commands, `--clean` removes the build output first. A CMake build folder made in another place, after the package moved or was renamed, is removed and configured again after a question |
| run | `rant run <Group> key=value...`, one node alone with `rant run node <node>`, `--dry-run` first. It builds what it starts (`--no-build` skips that), waits up to 10 s for the nodes to come up (`--wait S`), watches them 2 s more (`--watch S`) and fails with the log of one that exited or never came up |
| stop, restart | `rant stop [name]` (nothing named: all this workspace started), `rant stop --all` (every node on this machine), `rant restart [name]` (nothing named: all this workspace started), which stops, builds, then starts, the last build when the build fails. A stop or restart that names nothing leaves the services of an enabled workspace running, only naming one stops it. An included group runs inside the group that includes it and `rant ls nodes` lists it there: stopping or restarting it by name acts on just its own nodes. A name is the group of it that runs, else the node of it, and `rant stop node <node>` or `rant restart node <node>` takes the node alone. stop, restart and log find a name wherever rant started it on this machine: every run of it in one workspace, a pick when several workspaces run it, which fails without a terminal |
| services, at every boot | workspace.rant lists the workspace's services as `service "Hmi" { project = "line-hmi" }` blocks, a group with its param values, and `~/.rant/services.rant` lists them outside every workspace. `rant run` alone starts them. `rant enable` runs them now and every time the computer starts, as the user, and `rant enable <Group> key=value...` adds a block first. The first enable on a machine asks for admin once (sudo on Linux and macOS). `--login` starts them at login instead, for what needs the desktop. `rant disable` stops them and drops the start, keeping the blocks, and `rant disable <Group>` removes its block and stops it. `rant ls services` shows each service and whether it runs. The start never builds, and keeps `--domain` and the shell's `RANT_DOMAIN`, `RANT_PREFIX` and `RANT_NODE_NAME_PREFIX`. `rant stop <Group>` stops a service until the next start |
| node output | `rant log [name...]`, `-f` to follow, `-n N` lines, a group shows all its nodes, `rant log node <node>` the node alone |
| see the mesh | `rant ls <nodes\|entities\|topics\|variables\|functions\|tasks\|types\|packages\|groups\|sources\|services> [pattern]`, `rant ls` alone lists the kinds, `rant ls nodes --no-mesh` for only what this workspace started, without joining the mesh |
| explain one thing | `rant info [kind] <name>`, a type shows its fields and who uses it, a node its source file |
| read and write | `rant sub <topic\|var> -n 1`, `rant pub <topic\|var> <value>`, `rant call <fn> [value]`. On a terminal sub and a task's progress redraw in place, `--lines` (and any pipe) prints a record a line of `from`, `hz` and `value`, a call's result last. `--value-only` prints only the values. `--json` and `--csv` only change the format |
| libraries in packages | `rant add [library [folder]]` adds a library to the package here or in folder, or moves it to the latest release, Rant when none is named (`rant add`, `rant add math cam`). `--version V` picks a release, also an older one. `rant ls libraries` shows what each package uses, `rant ls libraries --available` what there is. A library's skill is offered as it is added |
| packages | `rant install <package\|repo>...` (`--version V` for one release, `--from PATH` for a local build folder, linked, or a .tar.gz). A package installed from a repo's link is never replaced by one of the same name from elsewhere, `rant remove` it first. `rant install` alone on a terminal is a checklist for a person, then one of their groups to run at every boot, so an agent names the packages. `rant remove <package>...`, `rant ls packages --available` for what there is, `rant ls sources` for where it looks, `rant info package <name>` before installing. A package that carries a skill gives it to each AI coding agent found, as this skill is the package `rant_skill`, and asks first when it carries more than the skill. `rant install` refuses a library, which `rant add` takes. `rant install` and `rant remove` need a package's nodes stopped, `rant update` stops them and starts them again |
| update | `rant update`: the CLI and the installed packages with their skills, then every library the workspace's packages use. `rant update <package\|library>...` updates only those. A package from `--from` stays until `rant install` replaces it |
| visual debugger | `rant explore`, after `rant install explorer` |
| VS Code | `rant install vscode` (`--editor cursor` for another editor, `--profile NAME` for a VS Code profile), `rant remove vscode`. Without `--profile` it takes the profile it went into before, else the one a VS Code terminal's folder opens in, else asks when several profiles have their own extensions. In a remote terminal it installs on that remote. Its Mesh view joins the mesh itself, or through the bridge the setting `rant.bridgeUrl` names |

Values on the command line are forgiving JSON: `{x: 1, y: 2}`, `[1, 2]`, or field words
like `position.x=1`. Add `--json` to any command for output a program can read, `-y` to
answer its questions.

## Config files

All three are HCL with every setting at the top level. workspace.rant at the workspace root,
empty for the defaults:

```hcl
logs   = "logs"              # where node output goes
ignore = ["third_party/**"]  # folders discovery skips
domain = 7                   # placement, see below
```

package.rant in a package: `name`, `description`, `build` (commands replacing the default),
`depend` (package names built first), `ignore`, `python` (the interpreter), and
`node "name" { run = "./tool --flag" }` for programs rant cannot find itself. Add
`expect_mesh = false` to one that never joins the mesh, so `rant run` waits only for it to
start.

A package others install gives its `name` in package.rant and names its release files by
rule: `<name>-<version>-<platform>.tar.gz` holds bin/, package.rant and the run files,
unpacked into `~/.rant/packages/<name>`, `<name>-<version>.vsix` goes into editors, and
`<name>-<version>-skill.zip` holds its agent skill, the `skill/` folder of its repo.
`version` is the release tag without its v, such as 1.2.0 or 1.2.0-beta.1, `platform` one of
win-x64, win-arm64, linux-x64, linux-arm64, osx-x64 and osx-arm64. A plain install takes the
newest release that is no pre-release, `--version` any one.

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

A `path` param's value reaches the node as a full path. One given on the command line counts
from the folder `rant` runs in, one a group file writes from that file's folder, so
`rant run Viewer project=my/project` means the same as typing the full path.

Placement: `domain`, `prefix` and `node_prefix` may go in workspace.rant, package.rant, a
group's top level, an `include` and a group `node`. Prefixes join from the outside in, the
innermost domain wins. `prefix` goes in front of every entity name a node makes,
`node_prefix` in front of its node name. Place nodes this way, never with names or domains
hard coded in the program.

`restart` says whether a node that exits starts again: `"never"` (the default),
`"on-failure"` (unless it exited with 0) or `"always"`. It goes on a group's top level, an
`include` or a group `node`, and the innermost wins. rant's service process starts such a
node and starts it again after a pause that grows while it keeps failing. `rant stop` stops
it for good.

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
