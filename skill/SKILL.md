---
name: rant-skill
description: Rant robotics middleware and the rant CLI. Use when a project uses Rant (workspace.rant, package.rant, *.run, rant.hpp, import rant, using Rant), or the user asks to make, build, run, debug or inspect Rant nodes, topics, variables, functions or tasks.
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
| Rant version in packages | `rant lib [folder]` shows it, `rant lib install [folder]` adds it, `rant lib update [folder]` moves it, both take `--version V` |
| packages | `rant install <package\|repo>...` (`--version V` for one release, `--from PATH` for a local build folder, linked, or a .tar.gz). A package installed from a repo's link is never replaced by one of the same name from elsewhere, `rant remove` it first. `rant install` alone on a terminal is a checklist for a person, then one of their groups to run at every boot, so an agent names the packages. `rant remove <package>...`, `rant ls packages --available` for what there is, `rant ls sources` for where it looks, `rant info package <name>` before installing. A package that carries a skill gives it to each AI coding agent found, as this skill is the package `rant_skill`. `rant install` and `rant remove` need a package's nodes stopped, `rant update` stops them and starts them again |
| update | `rant update`: the CLI and the installed packages with their skills, then the workspace's packages. `rant update <package>...` updates only those. A package from `--from` stays until `rant install` replaces it |
| visual debugger | `rant explore`, after `rant install explorer` |
| web frontends | `rant install bridge`, then `rant run Bridge`, see Bridge |
| explorer in a browser | `rant install explorer`, then `rant run WebExplorer`, see Web explorer |
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
unpacked into `~/.rant/packages/<name>`, and `<name>-<version>.vsix` goes into editors.
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
`path.workspace` anywhere but `archive` and `vsix`.

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

## Bridge

The bridge is a background server that lends this host's UDP sockets to Rant nodes elsewhere:
pages, which have none, and programs on another network, so they join this host's mesh. It is
the package `bridge`, and its group `Bridge` runs it like any group, no admin needed.

| goal | command |
|---|---|
| install | `rant install bridge`, `--from PATH` for a bridge you built |
| run it | `rant run Bridge`, `rant stop Bridge`, `rant restart Bridge` |
| at every boot | `rant enable Bridge`, `rant disable Bridge` |
| output | `rant log Bridge`, `-f` to follow |

- Its config is `~/.rant/bridge.yaml`, written at its first start with a random passphrase.
  It listens on every interface on 6731 unless the config says otherwise. Edit it by hand,
  then run `rant restart Bridge`.
- Bridges find each other over mDNS: each answers `rant-bridges.local` and lists the others,
  which `findBridges()` from `rant-middleware/tunnel` reads with no passphrase. `advertise:
  false` in the config turns it off.
- Security: the passphrase never crosses the wire, a client proves it on a fresh nonce.
  Anyone who can load a frontend that connects to the bridge has the bridge's access to the
  mesh. Restrict who can reach the frontend (Cloudflare Access or similar) and
  set `allowed_origins` in the config.
- A page, a script or an editor extension runs Rant itself through the TypeScript binding,
  the npm package `rant-middleware` each Rant release attaches. It has every pattern and
  reflection, with the same names as the other bindings. In Node.js a node joins the mesh
  directly, and a page passes the bridge's sockets:

```ts
import { openNode, types } from "rant-middleware";
import { TunnelNet } from "rant-middleware/tunnel";
const net = await TunnelNet.connect("ws://192.168.1.20:6731", { passphrase });
const node = await openNode("page", { net });
node.subscriber("Pose", types.Pose2D, (p) => draw(p));
```

- A node in any other binding runs on a bridge's sockets with the bridge option (`net.bridge`
  in C, `bridge` in C++, Python and Rust, `Bridge` in C#, Bridge Url on the Unity component)
  and its passphrase, or with `RANT_BRIDGE` and `RANT_BRIDGE_PASSPHRASE` in its environment
  when the code sets none. `ws://` only. An unreachable bridge or a wrong passphrase fails
  the open with the `bridge` error and its reason.
- A machine with no multicast, or behind a NAT (a container, a cloud host), joins a mesh this
  way, through a bridge on that mesh's network.

## Web explorer

The explorer package's group `WebExplorer` serves the explorer as a web page, so any browser
on the network opens it. The page reaches the mesh through the bridge on the same host, so run
both.

| goal | command |
|---|---|
| install | `rant install explorer` |
| run it | `rant run WebExplorer`, `port=N` and `bind=IP` set where it listens, 6732 on every address by default |
| stop it | `rant stop WebExplorer` |
| at every boot | `rant enable WebExplorer`, with the same params, `rant disable WebExplorer` |
| output | `rant log WebExplorer`, `-f` to follow |

## Manager

The package `manager` offers a workspace's groups and nodes on the mesh, so a program or an
HMI can see which run and run, stop or restart them.

| goal | command |
|---|---|
| install | `rant install manager` |
| run it | `rant run Manager` in the workspace, `rant stop Manager` |
| output | `rant log Manager`, `-f` to follow |

- Names start with the workspace's prefix, else its folder name in PascalCase (`cell` is
  `Cell`). `Cell/Rant/Groups` lists every group as `GroupState[]`, `Cell/Rant/Nodes` every
  node as `NodeState[]`, each row with its `name`, as the CLI takes it, and its `status`
  (Stopped, Running).
- A group's row lists the nodes its own file declares and the groups it includes, a node's
  the groups whose files declare it, up to 32 names each in fixed `Name[32]` lists, unused
  slots empty.
- `Cell/Rant/Groups/Manage` and `Cell/Rant/Nodes/Manage` take a `ManageRequest { name,
  action }`, the name from the list beside it and the action Run, Stop or Restart.
- Run and Restart wait up to 12 s for the nodes, so give the remote function handle a timeout
  over that, such as 30 s. Its 5 s default ends the call first.
- It never builds and passes no params. Stopping the manager leaves running what it started.

## HMI

The package `hmi` makes screens for machines: pages designed in a browser editor and run by a
native backend that is the Rant node. `rant install hmi` installs it with its own skill, which
says how to make them.

## Writing nodes

```cpp
#include "rant.hpp"   // CMake: target_link_libraries(x PRIVATE rant::rant_host)
rant::Node node("talker");
auto pose = node.publisher<rant::types::Pose2D>("Pose");
pose.send({ { 1.0, 2.0 }, 0.5 });
auto sub = node.subscriber<rant::types::Pose2D>("Pose", [](const rant::types::Pose2D& p) { /* ... */ });
```

```python
import rant
from rant.types import Double2, Pose2D
node = rant.Node("talker")
pose = node.publisher("Pose", Pose2D)
pose.send(Pose2D(position=Double2(x=1.0, y=2.0), angle=0.5))
node.subscriber("Pose", Pose2D, lambda p: print(p.angle))
```

```csharp
using Rant; using Rant.Types;
using var node = new RantNode("talker");
var pose = node.Publisher<Pose2D>("Pose");
node.Subscriber<Pose2D>("Pose", p => Console.WriteLine(p.Angle));
```

```rust
use rant::types::{Double2, Pose2D};   // a program in src/bin, Rant added with rant lib install
let node = rant::Node::new("talker", rant::NodeOptions::new())?;
let pose = node.publisher::<Pose2D>("Pose", rant::Qos::new())?;
pose.send(&Pose2D { position: Double2 { x: 1.0, y: 2.0 }, angle: 0.5 })?;
let _sub = node.subscriber::<Pose2D>("Pose", rant::Qos::new(), |p| println!("{}", p.angle))?;
```

```c
#define RANT_IMPLEMENTATION   /* in exactly one .c file */
#include "rant.h"
static void on_message(const RantMsg *m) {    /* every topic of the node lands here */
    RantPose2D p;                              /* standard types have C mirrors, wire layout */
    memcpy(&p, m->data.data, sizeof p);
}
RantAllocator mem = rant_allocator_heap(0);
RantNode *n = rant_node_open(&mem, "talker", on_message, NULL, NULL);
RantTopic *pose = rant_node_create_topic(n, "Pose", RANT_PUBSUB, rant_node_schema(n, "Pose2D"), NULL);
RantPose2D p = { { 1.0, 2.0 }, 0.5 };
rant_topic_send(pose, rant_bytes(&p, sizeof p));
```

Every handle comes from the node, named after what it is: publisher, subscriber,
variable_definition, remote_variable, function_definition, remote_function,
task_definition, remote_task (PascalCase in C#). A subscriber with no handler is pulled
with `take`, in Rust one from `pulled_subscriber`. A function or task handler returns its
answer, and in Rust a value answers Ok and an `Err` fails the call with its text.

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
rant::Node node("talker", o);
rant::Queue q = node.create_queue();
rant::Qos qos; qos.queue = &q;
auto sub = node.subscriber<rant::types::Pose2D>("Pose", handler, qos);
std::thread worker([&] { while (running) q.dispatch(0, 100ms); });
```

```python
node = rant.Node("talker", threading=rant.Threading.MANUAL)     # while True: node.poll(0.01)
node = rant.Node("talker", threading=rant.Threading.DISPATCH)   # each frame: node.dispatch()
q = node.create_queue()
node.subscriber("Pose", Pose2D, handler, queue=q)               # a thread runs q.dispatch(timeout=0.1)
```

```csharp
var node = new RantNode("talker", new NodeOptions { Threading = Threading.Manual });    // node.Poll(10)
var node = new RantNode("talker", new NodeOptions { Threading = Threading.Dispatch });  // node.Dispatch()
var q = node.CreateQueue();
node.Subscriber<Pose2D>("Pose", handler, new Qos { Queue = q });                        // q.Dispatch(0, 100)
```

```rust
let node = Node::new("talker", NodeOptions::new().threading(Threading::Manual))?;    // node.poll(Duration::from_millis(10))
let node = Node::new("talker", NodeOptions::new().threading(Threading::Dispatch))?;  // node.dispatch(0, Duration::ZERO)
let q = node.create_queue()?;
let sub = node.subscriber::<Pose2D>("Pose", Qos::new().queue(&q), handler)?;        // q.dispatch(0, Duration::from_millis(100))
```

```c
rant_node_start(n);                          /* service thread, stop with rant_node_stop */
while (running) rant_node_poll(n, 10);       /* or manual, without start */
RantQueue *q = rant_node_create_queue(n);    /* a queue, given at creation: */
rant_node_create_topic(n, "Pose", RANT_SUB_ONLY, schema, &(RantTopicOpts){ .queue = q });
rant_queue_dispatch(q, 0, 100);              /* on the thread that runs them */
```

## Conventions

- Use a standard type whenever one fits: `Pose`, `Pose2D`, `Transform`, `Twist`, `Wrench`,
  `Double2/3`, `Quaternion`, `JointState`, `Odometry`, `Imu`, `LaserScan`, `PointCloud`,
  `Mesh`, `Grid`, `Image`, `File`, `Color`, `Timestamp`, `Duration`, `Empty` and the rest at
  https://docs.rantlib.dev/llms.txt. Never define your own pose, vector, cloud or map.
- Pick the entity by shape: a topic for a stream, a variable for state or a setting with
  one owner, a function for a quick question, a task for a long job with progress or cancel.
- Names are snake_case where they meet code and PascalCase everywhere else:
  - Nodes are snake_case and named like their file: `arm_controller.py`, a C++ target
    `arm_controller`, a C# `ArmController.csproj` with `<AssemblyName>arm_controller`, a Rust
    `src/bin/arm_controller.rs`. Name a node for what it is (`lidar`, not `lidar_node`).
  - Entities and namespaces are PascalCase, `/` between levels: `FrontLidar/JointState`,
    `Line1/Conveyor`. So are types and enum options.
  - Every struct and enum has a name, the same in every language that sends or reads it.
    In schema text define it once and use the name: `Mode = enum<u8> { Idle, Run }` then
    `Arm { mode: Mode }`. An inline `{ }` or `enum` in a field is refused.
  - Groups are PascalCase: `Nav.run`, run as `rant run Nav`. Names match without case or
    underscores, so the group `Nav` and the node `nav` are one name and the group wins:
    `rant stop nav` stops the group, `rant stop node nav` the node alone.
    A group is how a system starts, so give every system one, even of one node.
    `rant new group` names them so.
  - Fields are snake_case on the wire. Write them in the language's style (`frame_id` in
    Python and Rust, `FrameId` in C#) and Rant converts, so both are `frame_id`.
  - `rant run` and `rant build` note each group or node name that breaks this.
- Units are SI: meters, seconds, m/s, N, radians. Time is a `Timestamp`, microseconds since
  the Unix epoch.
- Frames are right handed, and an angle turns from +x toward +y. Two kinds of 2D differ:
  - Physical 3D: z up. A body's own frame is x forward, y left, z up, as in ROS.
  - Physical 2D, such as a robot on the floor: the top view of 3D, so x right, y up, angles
    counterclockwise seen from above.
  - Image or screen: origin at the top left, x right, y down, angles clockwise on screen.
    Raw image rows start at the top.
- One program per node. Keep domain, prefixes and node names out of the code: the config
  files and groups place them, and the name in the code is only a default.
- Add Rant with `rant lib install`, never by hand, and keep packages current with `rant lib update`.

## Debugging

1. `rant ls nodes` shows what runs and what could, `rant ls entities` what they talk through.
2. Not seeing a node: check the domain on both sides (`--domain`, `domain`, `RANT_DOMAIN`).
3. A node that died: read `logs/<node>.log`.
4. Wrong config: `rant run X --dry-run`, `rant build --dry-run`, `rant info group X`.
5. Wrong data: `rant info <topic>` shows its type and who talks, `rant sub <topic> -n 1`
   shows a message. A type mismatch is refused, not silently read.
6. "not built yet": run `rant build`.
