---
name: rant
description: Rant robotics middleware and the rant CLI. Use when a project uses Rant (rant.hcl, *.group.hcl, rant.hpp, import rant, using Rant) or the user asks to make, build, run, debug or inspect Rant nodes, topics, variables, functions or tasks.
---

# Rant

Rant connects programs ("nodes") on one machine or a network ("the mesh"). Nodes find each
other on their own and share four kinds of entity: topics, variables, functions and tasks.
The `rant` command makes projects, builds them, starts and stops nodes, and reads and
writes the mesh. Full docs: https://docs.rantlib.dev/ (fetch the page you need).

## First

- Read the project before changing it. Keep its layout, names and style. The rules below
  are for new work and for what the project leaves open.
- `rant --help` and `rant <command> --help` are the truth for every command and flag.
- Nodes on another domain are invisible. Never start test nodes on the domain a real
  system uses. Use `--domain N` or a workspace `domain` for your own runs.

## Layout

```
cell/                     workspace: rant.hcl with workspace {}
  rant.hcl
  cell.group.hcl          a group: which nodes run together, and how
  drivers/                a package: CMakeLists.txt, pyproject.toml or a .csproj that uses Rant
    CMakeLists.txt
    lidar.cpp             a node: one program
  logs/                   each started node's output, <node>.log
  .rant/                  rant's state, never edit, keep out of git
```

A build file that uses Rant makes a package by itself. Add `package {}` to its rant.hcl
only to override something. A CMake folder inside a CMake package belongs to it.

## Commands

| goal | command |
|---|---|
| new project | `rant new workspace [folder] --lang cpp\|python\|csharp` (talker, listener, group) |
| new package or node | `rant new package [folder] --lang L`, `rant new node [name]`, `rant new group [name]` |
| build | `rant build [package...]`, `--dry-run` shows order and commands |
| run | `rant start node <node>`, `rant start group <group> key=value...`, `--dry-run` first |
| stop, restart | `rant stop [node X \| group X]` (nothing named: all this workspace started), `rant stop --all` (every node on this machine), `rant restart ...` |
| see the mesh | `rant ls [nodes\|entities\|topics\|variables\|functions\|tasks\|packages\|groups]`, `rant ls nodes -a` |
| explain one thing | `rant info [kind] <name>` |
| read and write | `rant sub <topic> -n 1`, `rant pub <topic> <value>`, `rant get <var>`, `rant set <var> <value>`, `rant call <fn> [value]` |
| Rant version in packages | `rant lib [folder]`, `rant lib install [folder] [--version V]` |
| visual debugger | `rant explore` |

Values on the command line are forgiving JSON: `{x: 1, y: 2}`, `[1, 2]`, or field words
like `position.x=1`. Add `--json` to any command for output a program can read, `-y` to
answer its questions.

## Config files

rant.hcl at the workspace root:

```hcl
workspace {
  logs   = "logs"              # where node output goes
  ignore = ["third_party/**"]  # folders discovery skips
  domain = 7                   # placement, see below
}
```

`package {}` in a package's rant.hcl: `name`, `build` (commands replacing the default),
`depend` (package names built first), `ignore`, `python` (the interpreter), and
`node "name" { run = "./tool --flag" }` for programs rant cannot find itself.

A group is `<name>.group.hcl`, everything at the top level:

```hcl
description = "Both arms"
param "speed" {
  type    = float              # string, int, float or bool
  default = 1
}
include "arm" {                # another group, with its param values
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

Placement: `domain`, `prefix` and `node_prefix` may go in `workspace {}`, `package {}`, a
group's top level, an `include` and a group `node`. Prefixes join from the outside in, the
innermost domain wins. `prefix` goes in front of every entity name a node makes,
`node_prefix` in front of its node name. Place nodes this way, never with names or domains
hard coded in the program.

## Writing nodes

```cpp
#include "rant.hpp"   // CMake: target_link_libraries(x PRIVATE rant::rant_host)
rant::Node node("talker");
auto pose = node.publisher<rant::types::Pose2D>("pose");
pose.send({ { 1.0, 2.0 }, 0.5 });
auto sub = node.subscriber<rant::types::Pose2D>("pose", [](const rant::types::Pose2D& p) { /* ... */ });
```

```python
import rant
from rant.types import Double2, Pose2D
node = rant.Node("talker")
pose = node.publisher("pose", Pose2D)
pose.send(Pose2D(position=Double2(x=1.0, y=2.0), angle=0.5))
node.subscriber("pose", Pose2D, lambda p: print(p.angle))
```

```csharp
using Rant; using Rant.Types;
using var node = new RantNode("talker");
var pose = node.Publisher<Pose2D>("pose");
node.Subscriber<Pose2D>("pose", p => Console.WriteLine(p.Angle));
```

Every handle comes from the node, named after what it is: publisher, subscriber,
variable_definition, remote_variable, function_definition, remote_function,
task_definition, remote_task (PascalCase in C#). A subscriber with no handler is pulled
with `take`.

## Conventions

- Use a standard type whenever one fits: `Pose`, `Pose2D`, `Transform`, `Twist`, `Wrench`,
  `Double2/3`, `Quaternion`, `JointState`, `Image`, `Color`, `Timestamp`, `Duration`,
  `Empty` and the rest at https://docs.rantlib.dev/. Never define your own pose or vector.
- Pick the entity by shape: a topic for a stream, a variable for state or a setting with
  one owner, a function for a quick question, a task for a long job with progress or cancel.
- Names are lower snake_case, `/` between levels: `arm/joint_state`, `line1/conveyor`.
  A node is named for what it is (`lidar`, not `lidar_node`). Types are PascalCase. Fields
  follow the language (`frame_id` in Python, `FrameId` in C#), the wire matches them.
- SI units, angles in radians, time as `Timestamp` microseconds since the Unix epoch.
- One program per node. Keep domain, prefixes and node names out of the code: rant.hcl and
  groups place them, and the name in the code is only a default.
- Add Rant with `rant lib install`, never by hand, and keep packages current with it.

## Debugging

1. `rant ls` and `rant ls nodes -a`: what runs, and what could.
2. Not seeing a node: check the domain on both sides (`--domain`, `domain`, `RANT_DOMAIN`).
3. A node that died: read `logs/<node>.log`.
4. Wrong config: `rant start group X --dry-run`, `rant build --dry-run`, `rant info group X`.
5. Wrong data: `rant info <topic>` shows its type and who talks, `rant sub <topic> -n 1`
   shows a message. A type mismatch is refused, not silently read.
6. "not built yet": run `rant build`.
