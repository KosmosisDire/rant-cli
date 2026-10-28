---
name: rant
description: Rant robotics middleware, the rant CLI and the Rant HMI. Use when a project uses Rant (workspace.rant, package.rant, *.run, rant.hpp, import rant, using Rant) or is an HMI project (hmi.toml, pages/*.html, components/*.html), or the user asks to make, build, run, debug or inspect Rant nodes, topics, variables, functions or tasks, or to make or change an HMI.
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
package database, then the databases sources.rant adds, in order. sources.rant is written by
hand and points a package at a local build folder, which is linked so each build counts at
once, or at another repo:

```hcl
package "explorer" {
  path = "C:/src/explorer-rant"   # a folder with package.rant, its run files and bin/
}
package "tool" {
  repo = "someone/rant-tool"      # its GitHub releases
}
database "someone/rant-packages" {}
```

## Commands

| goal | command |
|---|---|
| new project | `rant new workspace [folder] --lang cpp\|python\|csharp\|rust` (talker, listener, group) |
| new package or node | `rant new package [folder] --lang L`, `rant new node [name]`, `rant new group [name]` |
| build | `rant build [package...]`, `--dry-run` shows order and commands, `--clean` removes the build output first. A CMake build folder made in another place, after the package moved or was renamed, is removed and configured again after a question |
| run | `rant run <Group> key=value...`, one node alone with `rant run node <node>`, `--dry-run` first. It builds what it starts (`--no-build` skips that), waits up to 10 s for the nodes to come up (`--wait S`), watches them 2 s more (`--watch S`) and fails with the log of one that exited or never came up |
| stop, restart | `rant stop [name]` (nothing named: all this workspace started), `rant stop --all` (every node on this machine), `rant restart [name]` (nothing named: all this workspace started), which stops, builds, then starts, the last build when the build fails. A stop or restart that names nothing leaves enabled groups and nodes running, only naming one stops it. An included group runs inside the group that includes it and `rant ls nodes` lists it there: stopping or restarting it by name acts on just its own nodes |
| run at every boot | `rant enable <Group> key=value...` or `rant enable node <node>` runs it now and every time the computer starts, as the user. The first enable on a machine asks for admin once (sudo on Linux and macOS). `--login` starts it at login instead, for what needs the desktop. `rant disable` stops it and drops it, `rant ls services` shows what is enabled and whether it runs. The choices live in `~/.rant/services.json`. The start never builds, and keeps `--domain` and the shell's `RANT_DOMAIN`, `RANT_PREFIX` and `RANT_NODE_NAME_PREFIX`. `rant stop <Group>` stops an enabled group until the next start |
| node output | `rant log [name...]`, `-f` to follow, `-n N` lines, a group shows all its nodes |
| see the mesh | `rant ls <nodes\|entities\|topics\|variables\|functions\|tasks\|types\|packages\|groups\|sources\|services> [pattern]`, `rant ls` alone lists the kinds, `rant ls nodes --no-mesh` for only what this workspace started, without joining the mesh |
| explain one thing | `rant info [kind] <name>`, a type shows its fields and who uses it, a node its source file |
| read and write | `rant sub <topic\|var> -n 1`, `rant pub <topic\|var> <value>`, `rant call <fn> [value]`. On a terminal sub redraws in place, `--lines` (and any pipe) prints a record a line. `--json` and `--csv` only change the format |
| Rant version in packages | `rant lib [folder]` shows it, `rant lib install [folder]` adds it, `rant lib update [folder]` moves it, both take `--version V` |
| packages | `rant install <package>...` (`--version V` for one release, `--from PATH` for a local build folder, linked, or a .tar.gz), `rant remove <package>...`, `rant ls packages --available` for what there is, `rant ls sources` for where it looks, `rant info package <name>` before installing. `rant install` and `rant remove` need a package's nodes stopped, `rant update` stops them and starts them again |
| update | `rant update`: the CLI, the installed packages and the skill, then the workspace's packages. `rant update <package>...` updates only those. A package from `--from` stays until `rant install` replaces it |
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

A package others install says what each release carries, by kind: `archive =
"rant-x-${version}-${platform}.tar.gz"` holds bin/, package.rant and the run files, unpacked into
`~/.rant/packages/<name>`, and `vsix = "rant-x-${version}.vsix"` goes into editors. `version`
is the release tag without its v, `platform` one of win-x64, win-arm64, linux-x64,
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

The package `hmi` makes HMIs and apps: pages operators use in a browser, designed in a
browser editor, while their logic runs in a native backend that is the Rant node. A project
is a folder of plain text files, and you may write them directly.

| goal | command |
|---|---|
| install | `rant install hmi` |
| run a project | `rant run Hmi project=line-hmi`, a folder counted from where you run it. An empty folder becomes a new project. `port=N` and `bind=IP` set where it listens, 6739 on every address by default. `rtc_ports=6740-6759` pins its WebRTC ports for a firewall |
| open it | `http://localhost:6739` designs it, `http://localhost:6739/run` is an operator's screen |
| get it as an app | the editor's download button, or `curl -o line.exe "http://localhost:6739/app/win-x64?runs=device"` for `win-x64`, `linux-x64` or `linux-arm64`. `runs=screen&address=http://host:6739` makes one that shows that screen |
| check an edit | `rant log Hmi -n 20` names a refused file and the reason |
| see what is wrong | `curl http://localhost:6739/problems` gives every screen's problems as JSON: an expression or drawing that fails, a handler or logic that stopped, a fault in the HMI itself with its stack. The editor's status bar counts them, and `Hmi/Problems` on the mesh holds them too |
| stop it | `rant stop Hmi` |
| at every boot | `rant enable Hmi project=...`, `rant disable Hmi` |

- The full docs are plain text at https://docs.rantlib.dev/hmi-reference.md (every element,
  property, event and name), https://docs.rantlib.dev/hmi-files.md (the files and how each
  part behaves) and https://docs.rantlib.dev/hmi-math.md. Fetch them for anything this section
  leaves out.
- It serves https beside http on the same port, with its own certificate kept in
  `~/.rant/hmi`, which a browser warns about until trusted. A screen away from the HMI's host
  needs https for gamepads and for video it decodes itself.
- An app is one file that runs the HMI and its logic on the device with no server, or shows a
  server's screen. The server fetches each platform's runtime from the HMI's GitHub release the
  first time, about 170 MB. With no way to GitHub, put
  `rant-hmi-runtime-<version>-<platform>.tar.gz` from that release in
  `~/.rant/hmi/runtimes/<version>/`. On Linux, `chmod +x` the app first.
- Work against a simulator on its own domain (`--domain N`), never on the domain of a
  machine in use. `rant ls entities` and `rant info <name>` give the names, types and fields
  to import.
- There are no logins yet: whoever reaches the port can change the project and press its
  buttons. Use `bind=127.0.0.1` on a shared network, or put access control in front.

### What the HMI does, what you write

The HMI does these by itself. Never build them:

- Live values: every binding redraws as its values change, on every screen. Write no
  polling, timers or refresh code.
- Missing data: an element showing a value it cannot know yet, as an import missing from the
  mesh, draws hatched, and a handler that reads one stops, never the rest. The HMI's own
  variable `Hmi/Status`, imported like any entity, lists such imports in `missing`. Add no
  fallback such as `Speed ?? 0`, which would show a made-up value as real.
- Where code runs: drawing in each screen, handlers and logic on the backend. Write no client
  and server split.
- Connections: a screen reconnects by itself, its data over WebRTC with the WebSocket as the
  fallback. Video plays as H264 when rant-transcoder runs on the camera's host, else as the
  pictures come. Write no player or reconnect code.
- A screen that drops or goes silent for half a second: the HMI lets go of all it held, so its
  `release` handlers run. Write no watchdog.
- Writing back: a bindable input bound to a writable name writes it, and a refused write says
  why.
- Theme: each screen follows its own light or dark setting through the tokens. Write no theme
  switch.
- Phones: a screen held upright lays a page wider than tall out again for its width, band by
  band from the top, never zoomed. Build no phone layout.
- Files: a saved change reaches every screen. A file with a mistake is refused and the last
  good one stays.
- Records sample on the backend and logic handlers run there, whether a screen is open or not.

You write the rest: the imports and their intents, variables, computed values, records and
tokens, the pages and components, handlers for what operators do, logic for what must happen
on its own, and the safety choices under Pages.

### Project

```
hmi.toml                 name, start = "overview" the page screens open on, and [[ice]] WebRTC's
                         STUN and TURN servers (urls, username, credential), ice = [] for none
data/imports.toml        the mesh imports: each entity it uses, under an alias
data/variables.toml      its own state, per = "hmi" (one value) or "screen" (each screen's own)
data/computed.toml       values worked out from others, written back through a setter
data/records.toml        a value's history, sampled every_ms and kept keep_s
data/theme.toml          tokens, read as $name
pages/<name>.html        a page, named in small letters and dashes: Robot cell in robot-cell.html
components/<name>.html   a component, the built in ones copied in when the project starts
logic/handlers.html      handlers that run on the backend for no screen
files/                   models, pictures and fonts a page loads, as src="files/arm.glb"
```

Screens load nothing of the project but `files/`. Never put a model or a picture in a variable.

```toml
[[import]]
alias  = "Speed"          # PascalCase. The HMI's own names are camelCase, so the two never look alike
entity = "Line1/Speed"
kind   = "variable"       # topic, variable, function or task
intent = "read"           # write only what the HMI must change. A function or task is write
# rate   = 5              # a topic it reads may come at most this many a second, best effort
```

A page is the HTML it draws as. Lengths are units of 10 px, and a page is 128 by 72 by default.

```html
<page name="Overview" size="128x72">
  <div id="e1" name="Line panel" x="2" y="2" w="32" h="hug" style='background: $panel; border-radius: $radiusLarge; gap: $space; layout: "down"; padding: $spaceLarge'>
    <span id="e2" name="Title" x="0" y="0" w="hug" h="hug" style="color: $dim">"Line 1"</span>
    <Readout id="e3" name="Speed" x="0" y="0" w="fill" h="8" label='"Speed"' value="Speed" unit='"m/s"' digits="2" fill="$panel2"/>
    <Stepper id="e4" name="Target" x="0" y="0" w="fill" h="5" value="target" max="20"/>
    <Button id="e5" name="Start" x="0" y="0" w="fill" h="6" text='"Start"' fill="$green" holdMs="600" style='opacity: Ready ? 1 : 0.4; pointer-events: Ready ? "auto" : "none"'>
      <on event="confirm">
        await Start(target);
      </on>
    </Button>
  </div>
</page>
```

- Every element has an `id` of its own in its file. Keep the ids of what exists and give
  what you add the next free one.
- Every attribute but `id` and `name` is an expression: `Speed * 10`, text in quotes,
  `#2a6fd6` a colour, `$accent` a token. Put an attribute holding double quotes in single
  quotes.
- `x`, `y`, `w` and `h` take units, a percent of what holds the element, `hug` or `fill`.
  In a frame with `layout`, its parts follow one another and their `x` and `y` wait.
- A handler is `<on event="...">` first inside its element. `p` holds what the event brings:
  `x` and `y` from 0 to 1 across the element, `angle`, `on` the part pressed, or the value a
  component sends. The events:
  - pointer: `tap`, `press`, `release`, `hold` (`after` ms), `repeat` (`every` ms),
    `outside`, and `drag`, whose `p.dx` and `p.dy` count as the element's own `x` does, so
    `x = x + p.dx` follows the pointer. They take the left button, a finger or a pen, unless
    `button="middle"`, `"right"` or `"any"`, and bring `p.button`, `p.shift`, `p.ctrl`,
    `p.alt` and the element's size `p.w` and `p.h`
  - `scroll` (`p.dx`, `p.dy` as a finger would move what it shows) and `zoom` (`p.factor`), from
    a wheel, a trackpad, a pinch or two fingers, `from="wheel"` or `from="fingers"` for one
  - keys: `keydown`, `keyup` and `keyrepeat` (`every` ms), with `key="a"` for what it types or
    `code="KeyA"` for where it sits
  - gamepads: `buttondown` and `buttonup` with `button="south"` and `pad="1"`, and `axes`
    (`p.lx`, `ly`, `rx`, `ry`, `lt`, `rt`) past a `deadband`, all 0 once at rest
  - an event its component sends with `emit`
- `while="condition"` on any handler but an end takes the event only while it holds. An
  element holds one handler per event and filter. An end (`release`, `keyup`, `buttonup`)
  always comes, with `p.lost` true when the HMI let go for the operator, as a screen that
  dropped.
- Code sets a variable, or one imported to write, by assignment (`target = 5`,
  `Cmd.speed = 0.5`), calls a function with `await Start(target)` and publishes with
  `Jog.send({ x: 1, y: 0 })`. It also has `await sleep(ms)`, `notify(text)`, `log(...)` and
  `await open(Pages.Confirm)`, which gives what that page passes to `close(answer)`.
- A task is called as a function is, with a last function for each progress update:
  `await Fill(5, (p) => { filled = p })`, and a bar bound to `filled` shows it. What the call
  gives has `.cancel()` for that call, `Fill.cancel()` stops every call of it, and a stopped
  call fails with `e.status` `"cancelled"`.

### Components

```html
<component name="Stepper" size="18x5" sizing="fill">
  <prop name="value" default="0" bindable="true"/>
  <prop name="step" default="1"/>
  <prop name="min" default="0"/>
  <prop name="max" default="10"/>
  <prop name="fill" default="$panel3"/>
  <prop name="textColour" default="$text"/>
  <prop name="radius" default="$radius"/>
  <event name="change"/>
  <div id="e1" name="row" x="0" y="0" w="100%" h="100%" style='layout: "right"; gap: $spaceSmall'>
    <div id="e2" name="less" x="0" y="0" w="hug" h="100%" style='align: "center"; aspect-ratio: 1; background: less.pressed ? fill.darken(0.15) : fill; border-radius: radius; layout: "right"'>
      <on event="repeat" every="150">
        value = clamp(value - step, min, max);
        emit("change", value);
      </on>
      <span id="e3" name="lessSign" x="0" y="0" w="hug" h="hug" style="color: textColour; font-size: $textLarge">"-"</span>
    </div>
    <span id="e4" name="reading" x="0" y="0" w="fill" h="100%" style='align-content: "center"; color: textColour; font-size: $textLarge; font-weight: 600; text-align: "center"'>value</span>
    <div id="e5" name="more" x="0" y="0" w="hug" h="100%" style='align: "center"; aspect-ratio: 1; background: more.pressed ? fill.darken(0.15) : fill; border-radius: radius; layout: "right"'>
      <on event="repeat" every="150">
        value = clamp(value + step, min, max);
        emit("change", value);
      </on>
      <span id="e6" name="moreSign" x="0" y="0" w="hug" h="hug" style="color: textColour; font-size: $textLarge">"+"</span>
    </div>
  </div>
</component>
```

- Use a built in before writing one. Button, Toggle, Slider, Knob, Dropdown, ModeSwitch,
  Joystick and Joystick1D take input, and Readout, Gauge, Plot, Lamp, Bar, Bars, Joints, Pose,
  Position, Swatch and Table show what the machine reports. They sit in every new project's
  components folder. Changing a project's copy changes every instance of it.
- Give a view's main input a `type`, as `type="number"`, `type="Pose"` or `type="number[]"`.
  The editor's Show an import then offers the component for each import of that type.
- One job and the fewest inputs that do it: what an instance must say (`value`, `text`,
  `min`, `max`, `unit`, `options`) and a few look inputs. Add more only when asked.
- Every look input defaults to a token (`fill` `$panel3`, `colour` `$accent`, `textColour`
  `$text`, `radius` `$radius`), so the component follows the theme, light and dark, until
  an instance sets its own. Work states out from those rather than adding inputs:
  `fill.darken(0.15)` pressed, `colour.alpha(0.15)` a tint.
- Name inputs and events as the built ins do, so every instance reads alike: `value`,
  `min`, `max`, `step`, `unit`, `digits`, `text`, `label`, `options`, `fill`, `colour`,
  `textColour`, `textSize`, `radius`, and `change` with the new value, `confirm`, `move`.
- A control that holds a value has one `value` input with `bindable="true"`. Its code sets
  `value`, then sends `change` with it. An instance bound to a writable name
  (`value="target"`) writes back to it. One bound to the machine's state
  (`value="Robot.servo_power"`) shows it, and the instance's `change` handler asks the
  machine (`await SetServo(p)`), so the screen shows what the machine reports, not what was
  pressed.
- A component never names a mesh import. Data comes in through inputs and actions go out
  as events, so one component serves any machine and the page decides what a press does.
- `sizing="fill"` lays it out in each instance's box. Use it for what stretches (buttons,
  cards, bars, plots), with parts in percents, `hug`, `fill` and layouts. Leave it out for a
  drawing that must keep its shape, as a dial, which then scales whole.
- Name each part for its role in camelCase (`track`, `thumb`, `label`). Code reads a part's
  `pressed`, `hovered` and `heldFor` by that name, `root` for the whole. Give every press a
  visible response.
- Its own state is a `<var>`, each instance's own on each screen, as a dropdown's `open`.
- Made for touch: targets at least 4 units high, nothing shown only on hover, `outside`
  closes what a press opened.
- An input may share a math function's name, as a knob's `min` and `max`: the plain name is
  the input, and a call such as `max(a, b)` is always the function.
- Draw with frames and SVG shapes, ease movement with `transition`, and move with
  `time.now` only what must move on its own. A screen also reads `link` (`link.open`,
  `link.pingMs`), `keys` (what is held, as `keys.a`) and `gamepads`.

### Pages

- One top level frame per panel, holding its title and its controls, so each panel stays
  whole when a phone lays the page out again.
- Lay out with frames: `layout`, `gap`, `padding`, `w="fill"`, `h="hug"`, and a repeated part
  for a list. Keep `x` and `y` for what sits on its own, as a marker over a picture, on whole
  units.
- Depth comes from tokens: `$ground` the page, `$surface` and `$panel` cards, `$panel2` and
  `$panel3` for what sits on a card. Text is `$text`, labels `$dim`, hints `$faint`, in
  `$textSmall`, `$textSize` and `$textLarge`. Spacing is `$spaceSmall`, `$space` and
  `$spaceLarge`, corners `$radiusSmall` up to `$radiusPill`. Change the look in theme.toml,
  never element by element, and add a token for any colour that means something new.
- Colour means state, the same on every page: `$green` running or good, `$amber` waiting or
  a warning, `$red` a fault or stop, `$accent` selected, with their `Bg` tokens behind them.
  Always with words beside it, never colour alone.
- Show state first: the machine's mode and faults where the eye lands, values large with
  their units, actions below them.
- Safety:
  - An action that moves a machine takes a hold to confirm, as `holdMs="600"`.
  - Jogging runs only while pressed: `press` starts it and `release` stops it, which a
    screen that drops runs too. Never a toggle for motion.
  - A control that cannot act now is dimmed and takes no presses (`opacity` and
    `pointer-events` on the instance), with the reason in words nearby.
- Keep logic in data where it fits: a computed value for a derived number, a record for a
  plot's history, a handler in logic/handlers.html (`every`, `when` with `for`, `change`)
  for what must happen with no screen open. Page handlers are for what an operator does.
- A choice for the machine is `per = "hmi"`, an operator's own (a jog mode, a picked tab) is
  `per = "screen"`.
- `repeat="6"` or `repeat="Jobs"` draws an element per item, with `index` and `item`,
  instead of copies.
- A long list is a frame with `overflow-y: "auto"`, which a wheel and a finger scroll, a
  finger on a button in it too, as the Dropdown's `rows` does.
- `shows="over"` on a page opens it as a dialog over the one beneath.
- `<canvas>` draws with code in 2D or WebGL, and `<scene>` shows 3D parts (`box`, `points`,
  `model` and more) in metres with z up, each placed by its `pose`, as an import's. Fetch the
  reference before writing either.
- A scene's camera reads a screen variable, its view, which the scene's drag, scroll and zoom
  handlers move, as the editor makes a new one. `points` takes `color` and `size` as one or
  a list one per point, as a cloud's colours beside its points: `color="Cloud.colors"`.

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
  `Double2/3`, `Quaternion`, `JointState`, `Image`, `Color`, `Timestamp`, `Duration`,
  `Empty` and the rest at https://docs.rantlib.dev/llms.txt. Never define your own pose or vector.
- Pick the entity by shape: a topic for a stream, a variable for state or a setting with
  one owner, a function for a quick question, a task for a long job with progress or cancel.
- Names are snake_case where they meet code and PascalCase everywhere else:
  - Nodes are snake_case and named like their file: `arm_controller.py`, a C++ target
    `arm_controller`, a C# `ArmController.csproj` with `<AssemblyName>arm_controller`, a Rust
    `src/bin/arm_controller.rs`. Name a node for what it is (`lidar`, not `lidar_node`).
  - Entities and namespaces are PascalCase, `/` between levels: `FrontLidar/JointState`,
    `Line1/Conveyor`. So are types and enum options.
  - Groups are PascalCase: `Nav.run`, run as `rant run Nav`. Names match case exactly, so
    `rant stop Nav` stops the group and `rant stop nav` the node.
    A group is how a system starts, so give every system one, even of one node.
    `rant new group` names them so.
  - Fields are snake_case on the wire. Write them in the language's style (`frame_id` in
    Python and Rust, `FrameId` in C#) and Rant converts, so both are `frame_id`.
  - `rant run` and `rant build` note each group or node name that breaks this.
- Units are SI: meters, seconds, m/s, N, radians. Time is a `Timestamp`, microseconds since
  the Unix epoch.
- Frames are right handed, and an angle turns from +x toward +y. Two kinds of 2D differ:
  - Physical 3D: z up.
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
