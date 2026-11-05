//! `rant build`: the packages asked for and all they depend on, in dependency order. Each
//! build tool's output is progress, so it goes to stderr.

use std::path::Path;

use rant_config::build_plan::{BuildPlan, Step};
use rant_config::model::NodeType;
use rant_config::plan::Plan;
use rant_config::{Workspace, paths};
use rant_process::{Command, shown};
use serde::Serialize;

use crate::context::Context;
use crate::failure::{Failure, Result, failed};
use crate::names::renames;
use crate::output::{Data, Style, Table, Text};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the packages to build with all they depend on, every package when none is given
    #[arg(value_name = "package")]
    pub packages: Vec<String>,
    /// show the packages, dependencies, order and commands without building
    #[arg(long)]
    pub dry_run: bool,
    /// remove each package's build output first, then configure and build from nothing
    #[arg(long)]
    pub clean: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Report {
    DryRun(DryRun),
    Built(Built),
}

impl Data for Report {
    fn text(&self, t: &Text) -> String {
        match self {
            Report::DryRun(d) => d.text(t),
            Report::Built(b) => b.text(t),
        }
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<Report> {
    let ws = ctx.require_workspace()?;
    let plan = ws.build_plan(&args.packages)?;
    if args.dry_run {
        return Ok(Report::DryRun(DryRun::new(&ws, &plan, args.clean)));
    }
    let (groups, nodes) = own_names(&ws);
    let built = build_steps(ctx, &plan, args.clean)?;
    for note in renames(&groups, &nodes) {
        ctx.out.note(&note);
    }
    Ok(Report::Built(Built { built }))
}

/// Builds the packages the plans run and what they depend on, so a start runs the code as it
/// is now. A failed build is reported and false, and the last build that worked starts.
pub fn build_for(ctx: &mut Context, plans: &[&Plan]) -> Result<bool> {
    let ws = ctx.require_scope()?;
    let mut packages: Vec<String> = Vec::new();
    for i in plans.iter().flat_map(|p| &p.instances) {
        let own = ws.model.package(&i.node.package).is_some_and(|p| !p.installed);
        if own && !packages.contains(&i.node.package) {
            packages.push(i.node.package.clone());
        }
    }
    if packages.is_empty() {
        return Ok(true);
    }
    let built = ws.build_plan(&packages).map_err(Failure::from).and_then(|plan| build_steps(ctx, &plan, false));
    match built {
        Ok(_) => Ok(true),
        Err(f) => {
            for m in f.messages() {
                ctx.out.error(&format!("{m}, starting the last build"));
            }
            Ok(false)
        }
    }
}

/// The names of the workspace's own groups and node types, never an installed package's.
fn own_names(ws: &Workspace) -> (Vec<String>, Vec<String>) {
    let model = &ws.model;
    let installed = |package: &Option<String>| package.as_deref().and_then(|p| model.package(p)).is_some_and(|p| p.installed);
    let groups = model.groups.iter().filter(|g| !installed(&g.package)).map(|g| g.name.clone()).collect();
    let own = model.packages.iter().filter(|p| !p.installed).flat_map(|p| &p.nodes);
    let nodes = own.chain(&model.loose).map(|n| n.name.clone()).collect();
    (groups, nodes)
}

/// Runs a build's steps in order and gives the packages built. Configuring CMake, or removing
/// a build folder made elsewhere, asks first, and the first step that fails stops it.
pub fn build_steps(ctx: &Context, plan: &BuildPlan, clean: bool) -> Result<Vec<String>> {
    let mut built = Vec::new();
    for s in plan.steps.iter().filter(|s| s.configure.is_some() || !s.commands.is_empty()) {
        ctx.out.progress(&format!("building {}", s.package));
        // a build folder just removed configures again without a question
        let mut fresh = false;
        if clean {
            if s.clean.is_empty() {
                ctx.out.note(&format!("{} builds with its own commands, so rant removes nothing of it", s.package));
            }
            for c in &s.clean {
                remove_output(s, c)?;
            }
            fresh = true;
        } else if let (Some(made), Some(build_dir)) = (&s.moved_from, &s.build_dir) {
            let question = format!(
                "The build folder of {} was made in {}, which CMake cannot use here. Remove it and configure again?",
                s.package,
                paths::slash(made)
            );
            if !ctx.confirm(&question) {
                return Err(failed(format!("{0} was not built, `rant build {0} --clean` makes its build folder again", s.package)));
            }
            remove_output(s, build_dir)?;
            fresh = true;
        }
        if let Some(configure) = s.configure.as_ref().filter(|_| fresh || s.unconfigured) {
            if !fresh && !ctx.confirm(&format!("{} is not configured yet. Configure it with `{}`?", s.package, shown(configure))) {
                return Err(failed(format!("{} was not configured, so not built", s.package)));
            }
            run_command(s, configure)?;
        }
        for c in &s.commands {
            run_command(s, c)?;
        }
        built.push(s.package.clone());
    }
    Ok(built)
}

/// One command in a package's folder. A failure names both.
fn run_command(s: &Step, argv: &[String]) -> Result<()> {
    let code = rant_process::run(&Command::new(argv.iter().cloned(), &s.dir)).map_err(|e| failed(format!("building {}: {e}", s.package)))?;
    if code != 0 {
        return Err(failed(format!("building {} failed: `{}` exited {code}", s.package, shown(argv))));
    }
    Ok(())
}

/// Removes a folder a build made, so the next build starts from nothing.
fn remove_output(s: &Step, p: &Path) -> Result<()> {
    match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(failed(format!("building {}: cannot remove {}, {e}", s.package, paths::slash(p))))
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Serialize)]
pub struct Built {
    built: Vec<String>,
}

impl Data for Built {
    fn text(&self, _: &Text) -> String {
        match self.built.len() {
            0 => "nothing to build: no package here has a build step".into(),
            1 => "built 1 package".into(),
            n => format!("built {n} packages"),
        }
    }
}

/// Everything a build would do, and why: the packages and their node types, every
/// dependency with where it was found, the order and the exact commands.
#[derive(Debug, Serialize)]
pub struct DryRun {
    workspace: String,
    packages: Vec<PackageRow>,
    loose: Vec<NodeRow>,
    dependencies: Vec<EdgeRow>,
    build: Vec<StepRow>,
    #[serde(skip)]
    clean: bool,
}

#[derive(Debug, Serialize)]
struct PackageRow {
    name: String,
    dir: String,
    nodes: Vec<NodeRow>,
}

#[derive(Debug, Serialize)]
struct NodeRow {
    name: String,
    kind: &'static str,
    path: Option<String>,
    run: Vec<String>,
    cwd: String,
    #[serde(skip)]
    reference: String,
}

#[derive(Debug, Serialize)]
struct EdgeRow {
    from: String,
    to: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct StepRow {
    package: String,
    dir: String,
    /// The CMake configure the build runs, none when the folder is configured already.
    configure: Option<Vec<String>>,
    moved_from: Option<String>,
    clean: Vec<String>,
    commands: Vec<Vec<String>>,
    #[serde(skip)]
    step: Step,
}

impl NodeRow {
    fn of(n: &NodeType) -> NodeRow {
        NodeRow {
            name: n.name.clone(),
            kind: n.kind.name(),
            path: n.path.as_deref().map(paths::slash),
            run: n.run.clone(),
            cwd: paths::slash(&n.cwd),
            reference: n.reference(),
        }
    }
}

impl DryRun {
    fn new(ws: &Workspace, plan: &BuildPlan, clean: bool) -> DryRun {
        let model = &ws.model;
        let own = model.packages.iter().filter(|p| !p.installed);
        let packages = own.map(|p| PackageRow { name: p.name.clone(), dir: paths::slash(&p.dir), nodes: p.nodes.iter().map(NodeRow::of).collect() });
        let edge = |e: &rant_config::build_plan::Edge| EdgeRow { from: e.from.clone(), to: e.to.clone(), source: e.source.clone() };
        let step = |s: &Step| StepRow {
            package: s.package.clone(),
            dir: paths::slash(&s.dir),
            configure: s.configure.clone().filter(|_| s.unconfigured || s.moved_from.is_some()),
            moved_from: s.moved_from.as_deref().map(paths::slash),
            clean: s.clean.iter().map(|c| paths::slash(c)).collect(),
            commands: s.commands.clone(),
            step: s.clone(),
        };
        DryRun {
            workspace: paths::slash(&model.settings.root),
            packages: packages.collect(),
            loose: model.loose.iter().map(NodeRow::of).collect(),
            dependencies: plan.edges.iter().map(edge).collect(),
            build: plan.steps.iter().map(step).collect(),
            clean,
        }
    }
}

impl Data for DryRun {
    fn text(&self, t: &Text) -> String {
        let root = Path::new(&self.workspace);
        let near = |p: &str| paths::relative(Path::new(p), root);
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut sorted: Vec<&PackageRow> = self.packages.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let (mut packages, mut nodes, mut edges) = (Table::default(), Table::default(), Table::default());
        let mut node_row = |n: &NodeRow| nodes.row(vec![n.reference.clone(), dim(n.kind), n.path.as_deref().map_or_else(|| shown(&n.run), near)]);
        for p in sorted {
            packages.row(vec![p.name.clone(), dim(&near(&p.dir))]);
            p.nodes.iter().for_each(&mut node_row);
        }
        self.loose.iter().for_each(&mut node_row);
        for e in &self.dependencies {
            edges.row(vec![format!("{} -> {}", e.from, e.to), dim(&e.source)]);
        }

        let mut lines = Vec::new();
        for (title, table) in [("PACKAGES", &packages), ("NODES", &nodes), ("DEPENDENCIES", &edges)] {
            lines.push(t.paint(Style::Bold, title));
            lines.extend(table.lines("  "));
            if table.is_empty() {
                lines.push(dim("  (none)"));
            }
            lines.push(String::new());
        }
        lines.push(t.paint(Style::Bold, "BUILD ORDER"));
        for (n, row) in self.build.iter().enumerate() {
            let s = &row.step;
            lines.push(format!("  {}. {}", n + 1, s.package));
            let moved = !self.clean && s.moved_from.is_some();
            if self.clean {
                lines.extend(s.clean.iter().map(|c| format!("     remove {}", near(&paths::slash(c)))));
            }
            if let (true, Some(build_dir), Some(made)) = (moved, &s.build_dir, &s.moved_from) {
                let why = format!("  (made in {}, asks first)", paths::slash(made));
                lines.push(format!("     remove {}{}", near(&paths::slash(build_dir)), dim(&why)));
            }
            if let Some(configure) = s.configure.as_ref().filter(|_| self.clean || moved || s.unconfigured) {
                let asks = if self.clean || moved { String::new() } else { dim("  (asks first)") };
                lines.push(format!("     {}{asks}", shown(configure)));
            }
            lines.extend(s.commands.iter().map(|c| format!("     {}", shown(c))));
            if s.configure.is_none() && s.commands.is_empty() {
                lines.push(dim("     nothing to build"));
            }
        }
        if self.build.is_empty() {
            lines.push(dim("  (none)"));
        }
        lines.join("\n")
    }
}
