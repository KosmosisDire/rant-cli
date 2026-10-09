---
name: rant
description: Rant robotics middleware and the rant CLI. Use when a project uses Rant (rant.hcl, *.group.hcl, rant.hpp, import rant, using Rant) or the user asks to make, build, run, debug or inspect Rant nodes, topics, variables, functions or tasks.
---

# Rant

Rant connects programs ("nodes") on one machine or a network ("the mesh"). Nodes find each
other on their own and share four kinds of entity: topics, variables, functions and tasks.
The `rant` command makes projects, builds them, starts and stops nodes, and reads and
writes the mesh. Full docs: https://docs.rantlib.dev/llms.txt (fetch the page you need).

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
rant::Node node("Talker");
auto pose = node.publisher<rant::types::Pose2D>("pose");
pose.send({ { 1.0, 2.0 }, 0.5 });
auto sub = node.subscriber<rant::types::Pose2D>("pose", [](const rant::types::Pose2D& p) { /* ... */ });
```

```python
import rant
from rant.types import Double2, Pose2D
node = rant.Node("Talker")
pose = node.publisher("pose", Pose2D)
pose.send(Pose2D(position=Double2(x=1.0, y=2.0), angle=0.5))
node.subscriber("pose", Pose2D, lambda p: print(p.angle))
```

```csharp
using Rant; using Rant.Types;
using var node = new RantNode("Talker");
var pose = node.Publisher<Pose2D>("pose");
node.Subscriber<Pose2D>("pose", p => Console.WriteLine(p.Angle));
```

```c
#define RANT_IMPLEMENTATION   /* in exactly one .c file */
#include "rant.h"
static void on_message(const RantMsg *m) {    /* every topic of the node lands here */
    RantPose2D p;                              /* standard types have C mirrors, wire layout */
    memcpy(&p, m->data.data, sizeof p);
}
RantAllocator mem = rant_allocator_heap(0);
RantNode *n = rant_node_open(&mem, "Talker", on_message, NULL, NULL);
RantTopic *pose = rant_node_create_topic(n, "pose", RANT_PUBSUB, rant_node_schema(n, "Pose2D"), NULL);
RantPose2D p = { { 1.0, 2.0 }, 0.5 };
rant_topic_send(pose, rant_bytes(&p, sizeof p));
```

Every handle comes from the node, named after what it is: publisher, subscriber,
variable_definition, remote_variable, function_definition, remote_function,
task_definition, remote_task (PascalCase in C#). A subscriber with no handler is pulled
with `take`.

## Threading

Every call is thread safe. The node's threading mode decides where callbacks run:

- Service thread, the default: a thread of the node's own runs the loop and every
  callback, one at a time. Use it unless something below fits better.
- Manual: your thread calls poll and callbacks run inside it. For a node with its own loop.
- Dispatch: the service thread runs the loop, callbacks wait until your thread calls
  dispatch. For a game or UI loop, once per frame.
- A queue: one handle's callbacks wait on a queue that a thread of your choice dispatches.

Inside a callback only sends and read only queries work. Creating or closing handles is
refused there, except from a dispatch, which has the whole API.

```cpp
rant::NodeOptions o;
o.threading = rant::Threading::Manual;      // while (running) node.poll(10ms);
o.threading = rant::Threading::Dispatch;    // each frame: node.dispatch();
rant::Node node("Talker", o);
rant::Queue q = node.create_queue();
rant::Qos qos; qos.queue = &q;
auto sub = node.subscriber<rant::types::Pose2D>("pose", handler, qos);
std::thread worker([&] { while (running) q.dispatch(0, 100ms); });
```

```python
node = rant.Node("Talker", threading=rant.Threading.MANUAL)     # while True: node.poll(0.01)
node = rant.Node("Talker", threading=rant.Threading.DISPATCH)   # each frame: node.dispatch()
q = node.create_queue()
node.subscriber("pose", Pose2D, handler, queue=q)               # a thread runs q.dispatch(timeout=0.1)
```

```csharp
var node = new RantNode("Talker", new NodeOptions { Threading = Threading.Manual });    // node.Poll(10)
var node = new RantNode("Talker", new NodeOptions { Threading = Threading.Dispatch });  // node.Dispatch()
var q = node.CreateQueue();
node.Subscriber<Pose2D>("pose", handler, new Qos { Queue = q });                        // q.Dispatch(0, 100)
```

```c
rant_node_start(n);                          /* service thread, stop with rant_node_stop */
while (running) rant_node_poll(n, 10);       /* or manual, without start */
RantQueue *q = rant_node_create_queue(n);    /* a queue, given at creation: */
rant_node_create_topic(n, "pose", RANT_SUB_ONLY, schema, &(RantTopicOpts){ .queue = q });
rant_queue_dispatch(q, 0, 100);              /* on the thread that runs them */
```

## Conventions

- Use a standard type whenever one fits: `Pose`, `Pose2D`, `Transform`, `Twist`, `Wrench`,
  `Double2/3`, `Quaternion`, `JointState`, `Image`, `Color`, `Timestamp`, `Duration`,
  `Empty` and the rest at https://docs.rantlib.dev/llms.txt. Never define your own pose or vector.
- Pick the entity by shape: a topic for a stream, a variable for state or a setting with
  one owner, a function for a quick question, a task for a long job with progress or cancel.
- Names:
  - Entities are camelCase at every level, `/` between levels: `arm/jointState`,
    `line1/conveyorSpeed`.
  - Nodes are PascalCase, named for what they are: `Lidar`, `ArmController`, never
    `LidarNode`.
  - Types are PascalCase, fields camelCase: `Waypoint { at: Transform, holdTime: Duration }`.
  - A type written in the language gets these on the wire by itself (`frame_id` in Python
    and `FrameId` in C# both go as `frameId`). Schema text, as in C's `rant_node_schema`,
    must spell them so.
- Units are SI: meters, seconds, m/s, N, radians. Time is a `Timestamp`, microseconds since
  the Unix epoch.
- Frames are right handed, and an angle turns from +x toward +y. Two kinds of 2D differ:
  - Physical 3D: z up.
  - Physical 2D, such as a robot on the floor: the top view of 3D, so x right, y up, angles
    counterclockwise seen from above.
  - Image or screen: origin at the top left, x right, y down, angles clockwise on screen.
    Raw image rows start at the top.
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
