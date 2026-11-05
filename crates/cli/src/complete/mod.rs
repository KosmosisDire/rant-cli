//! Tab completion, answered by the binary for every shell. A shell hook runs `rant __complete
//! --cur=<word at the cursor> <words before it...>` and offers each line printed.

pub mod shells;

use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

use rant::EntityKind;
use rant_config::param::{self, ParamType};
use rant_config::refs::same_name;
use rant_config::{GroupInfo, Settings, Workspace};

use crate::commands::kinds;
use crate::commands::run::states_anywhere;
use crate::context::Context;
use crate::files::same_path;
use crate::mesh::{Client, Snapshot, kind_name, standard_types};
use crate::output::{Format, Output};
use crate::packages;
use crate::state::RootKind;

/// The mesh query one completion makes: whoever answers within this joins the snapshot.
const MESH_BUDGET: Duration = Duration::from_millis(80);

/// The words a command offers at the cursor. Entity paths go one segment at a time, like file
/// paths, so camera offers camera/ before camera/left.
#[derive(Debug, Default)]
struct Candidates {
    words: Vec<String>,
    paths: bool,
}

fn words(words: Vec<String>) -> Candidates {
    Candidates { words, paths: false }
}

fn paths(words: Vec<String>) -> Candidates {
    Candidates { words, paths: true }
}

/// One request as a command's completer sees it. Each source loads on first use, so a
/// completion that needs no mesh never touches it.
struct Request<'a> {
    ctx: &'a mut Context,
    /// After the command, options left out.
    words: Vec<String>,
    partial: String,
    workspace: Option<Option<Workspace>>,
    mesh: Option<Snapshot>,
}

impl Request<'_> {
    /// The workspace here, or rant's home, read whatever errors its config has.
    fn workspace(&mut self) -> Option<&Workspace> {
        if self.workspace.is_none() {
            let found = match (self.ctx.cwd(), self.ctx.home()) {
                (Ok(cwd), Ok(home)) => Settings::find(cwd, home).ok().map(|s| Workspace::load(s, cwd, home).0),
                _ => None,
            };
            self.workspace = Some(found);
        }
        self.workspace.as_ref().and_then(Option::as_ref)
    }

    /// The bare name, or package/name where another package has one of that name.
    fn node_types(&mut self) -> Vec<String> {
        let Some(ws) = self.workspace() else { return Vec::new() };
        let m = &ws.model;
        let uses = |name: &str| m.packages.iter().flat_map(|p| &p.nodes).chain(&m.loose).filter(|n| n.name == name).count();
        let mut out: Vec<String> = m.packages.iter().flat_map(|p| &p.nodes).map(|n| if uses(&n.name) == 1 { n.name.clone() } else { n.reference() }).collect();
        out.extend(m.loose.iter().filter(|n| uses(&n.name) == 1).map(|n| n.name.clone()));
        out
    }

    /// Each by the shortest name that finds it from here.
    fn groups(&mut self) -> Vec<String> {
        self.workspace().map(|ws| ws.group_names().into_iter().map(|g| g.short).collect()).unwrap_or_default()
    }

    /// The groups the services here name.
    fn services(&mut self) -> Vec<String> {
        self.workspace().map(|ws| ws.model.settings.services.iter().map(|s| s.group.clone()).collect()).unwrap_or_default()
    }

    fn packages(&mut self, installed: bool) -> Vec<String> {
        let Some(ws) = self.workspace() else { return Vec::new() };
        ws.model.packages.iter().filter(|p| installed || !p.installed).map(|p| p.name.clone()).collect()
    }

    fn group(&mut self, name: &str) -> Option<GroupInfo> {
        self.workspace()?.group(name).ok()
    }

    /// The nodes rant started anywhere on this machine.
    fn running_nodes(&mut self) -> Vec<String> {
        let states = states_anywhere(self.ctx, false).unwrap_or_default();
        states.into_iter().flat_map(|(_, st)| st.instances).map(|i| i.name).collect()
    }

    /// By the stem, which finds a group another workspace runs as stop looks it up, and by the
    /// short name here.
    fn running_groups(&mut self) -> Vec<String> {
        let states = states_anywhere(self.ctx, false).unwrap_or_default();
        let here: Vec<(String, String)> = match self.workspace() {
            Some(ws) => ws.group_names().into_iter().map(|g| (g.file.name.clone(), g.short)).collect(),
            None => Vec::new(),
        };
        let root = self.workspace().map(|ws| ws.model.settings.root.clone());
        let mut out = Vec::new();
        for (folder, st) in states {
            for r in st.roots.iter().filter(|r| r.kind == RootKind::Group) {
                let mine = root.as_deref().is_some_and(|w| same_path(w, &folder));
                let short = here.iter().find(|(full, _)| mine && *full == r.name).map(|(_, s)| s.clone());
                out.push(short.unwrap_or_else(|| r.name.rsplit('/').next().unwrap_or(&r.name).to_string()));
            }
        }
        out
    }

    /// Every node with a log, running or not.
    fn logged_nodes(&mut self) -> Vec<String> {
        self.workspace().map(|ws| crate::logs::logged_nodes(&ws.model.settings.logs)).unwrap_or_default()
    }

    fn mesh(&mut self) -> &Snapshot {
        if self.mesh.is_none() {
            let (domain, home) = (self.ctx.domain().unwrap_or_default(), self.ctx.home().ok().map(ToOwned::to_owned));
            let snapshot = home.map_or_else(Snapshot::default, |home| {
                if let Ok(client) = Client::new(domain, Some(&home)) {
                    client.settle(MESH_BUDGET);
                }
                Snapshot::load(&home, domain)
            });
            self.mesh = Some(snapshot);
        }
        self.mesh.as_ref().expect("loaded above")
    }

    fn mesh_nodes(&mut self) -> Vec<String> {
        self.mesh().nodes.iter().cloned().collect()
    }

    /// The entities of these kinds, every one for none.
    fn entities(&mut self, of: &[EntityKind]) -> Vec<String> {
        let kinds: Vec<&str> = of.iter().map(|&k| kind_name(k)).collect();
        self.mesh().entities.iter().filter(|(_, k)| kinds.is_empty() || kinds.contains(&k.as_str())).map(|(n, _)| n.clone()).collect()
    }

    /// `key=` for each param of the group not yet given, or the values of the one being typed.
    fn params(&mut self) -> Vec<String> {
        let Some(group) = self.words.first().cloned() else { return Vec::new() };
        let offered = self.group(&group).map(|g| offered(&g)).unwrap_or_default();
        params(&offered, &self.partial, &self.words[1..])
    }
}

/// A param as completion offers it: its name and the values it takes, when it names them.
struct Offered {
    name: String,
    values: Vec<String>,
}

fn offered(g: &GroupInfo) -> Vec<Offered> {
    let values = |p: &param::Param| match p.ty {
        ParamType::Bool if p.options.is_empty() => vec!["true".to_string(), "false".to_string()],
        _ => p.options.iter().map(param::text).collect(),
    };
    g.params.iter().map(|p| Offered { name: p.name.clone(), values: values(p) }).collect()
}

fn params(offered: &[Offered], partial: &str, given: &[String]) -> Vec<String> {
    if let Some((key, _)) = partial.split_once('=') {
        return offered.iter().filter(|p| p.name == key).flat_map(|p| p.values.iter().map(|v| format!("{key}={v}"))).collect();
    }
    let done = |name: &str| given.iter().any(|g| g.starts_with(&format!("{name}=")));
    offered.iter().filter(|p| !done(&p.name)).map(|p| format!("{}=", p.name)).collect()
}

/// w starts with partial, whatever the case of either, since names mix Nav and talker.
fn starts_with(w: &str, partial: &str) -> bool {
    w.len() >= partial.len() && w.is_char_boundary(partial.len()) && w[..partial.len()].eq_ignore_ascii_case(partial)
}

/// The candidates that start with partial in any case, each path cut after the next slash past
/// it, sorted and without repeats.
fn narrow(c: &Candidates, partial: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for w in c.words.iter().filter(|w| starts_with(w, partial)) {
        // a leading slash starts the path, it ends no segment
        let start = partial.len().max(1);
        let cut = if c.paths { w.get(start..).and_then(|rest| rest.find('/')).map(|i| i + start) } else { None };
        match cut {
            Some(slash) if slash + 1 < w.len() => out.insert(w[..=slash].to_string()),
            _ => out.insert(w.clone()),
        };
    }
    if c.paths {
        return out.into_iter().collect();
    }
    // a group and a node of one name, such as Transcoder and transcoder, are offered once, in
    // the spelling typed so far, or the shell could never complete either
    let mut names: Vec<String> = Vec::new();
    for w in out {
        match names.iter_mut().find(|n| same_name(n, &w)) {
            None => names.push(w),
            Some(n) if w.starts_with(partial) && !n.starts_with(partial) => *n = w,
            Some(_) => {}
        }
    }
    names
}

/// What a command offers once the command line names it.
fn offers(command: &str, r: &mut Request) -> Candidates {
    let home = r.ctx.home().ok().map(ToOwned::to_owned);
    let cached = |library: bool| home.as_deref().map(|h| packages::cached(h, false)).unwrap_or_default().into_iter().filter(move |s| s.library.is_some() == library).map(|s| s.name);
    let installed = || home.as_deref().map(packages::installed).unwrap_or_default();
    let w = r.words.clone();
    let first = w.first().map(String::as_str);
    match command {
        "add" if w.is_empty() => words(cached(true).collect()),
        "build" => words(r.packages(false).into_iter().filter(|p| !w.contains(p)).collect()),
        "call" if w.is_empty() => paths(r.entities(&[EntityKind::Function, EntityKind::Task])),
        "pub" if w.is_empty() => paths(r.entities(&[EntityKind::Topic, EntityKind::Variable])),
        "sub" if w.is_empty() => paths(r.entities(&[EntityKind::Topic, EntityKind::Variable, EntityKind::Function, EntityKind::Task])),
        "enable" if w.is_empty() => words(r.groups()),
        "disable" if w.is_empty() => words(r.services()),
        "enable" | "disable" => words(r.params()),
        "info" if w.is_empty() => paths(kinds::words(false).into_iter().chain(info_names(r, None)).collect()),
        "info" if w.len() == 1 => kinds::Kind::named(&w[0]).map_or_else(Candidates::default, |k| paths(info_names(r, Some(k)))),
        "install" => words(cached(false).collect()),
        "update" => words(installed().into_iter().chain(cached(true)).collect()),
        "remove" => {
            let skills = home.as_deref().map(packages::skills::skills).unwrap_or_default();
            words(installed().into_iter().chain(skills.into_iter().map(|s| s.name)).collect())
        }
        "log" if first == Some("node") => words(r.logged_nodes()),
        "log" => words(r.groups().into_iter().chain(r.logged_nodes()).chain(w.is_empty().then(|| "node".to_string())).collect()),
        "ls" if w.is_empty() => words(kinds::words(true)),
        "new" if w.is_empty() => words(crate::commands::new::KINDS.map(String::from).to_vec()),
        "setup" => words(shells::SHELLS.iter().map(|s| s.name.to_string()).collect()),
        "run" if w.is_empty() => words(r.groups().into_iter().chain(["node".to_string()]).collect()),
        "run" if first == Some("node") => words(if w.len() == 1 { r.node_types() } else { Vec::new() }),
        "stop" if w.is_empty() => words(r.running_groups().into_iter().chain(r.running_nodes()).chain(["node".to_string()]).collect()),
        "stop" if first == Some("node") => words(if w.len() == 1 { r.running_nodes() } else { Vec::new() }),
        "restart" if w.is_empty() => words(r.groups().into_iter().chain(r.running_nodes()).chain(r.node_types()).chain(["node".to_string()]).collect()),
        "restart" if first == Some("node") => words(if w.len() == 1 { r.running_nodes().into_iter().chain(r.node_types()).collect() } else { Vec::new() }),
        "run" | "stop" | "restart" => words(r.params()),
        _ => Candidates::default(),
    }
}

/// What info can explain of a kind, everything for none.
fn info_names(r: &mut Request, kind: Option<kinds::Kind>) -> Vec<String> {
    use kinds::Kind;
    let want = |k: Kind| kind.is_none_or(|w| w == k);
    let mut out = Vec::new();
    if want(Kind::Package) {
        out.extend(r.packages(true));
    }
    if want(Kind::Node) {
        out.extend(r.node_types());
    }
    if want(Kind::Group) {
        out.extend(r.groups());
    }
    if want(Kind::Node) {
        out.extend(r.mesh_nodes());
        out.extend(r.running_nodes());
    }
    let of = |k: EntityKind| vec![k];
    match kind {
        None | Some(Kind::Entity) => out.extend(r.entities(&[])),
        Some(Kind::Topic) => out.extend(r.entities(&of(EntityKind::Topic))),
        Some(Kind::Variable) => out.extend(r.entities(&of(EntityKind::Variable))),
        Some(Kind::Function) => out.extend(r.entities(&of(EntityKind::Function))),
        Some(Kind::Task) => out.extend(r.entities(&of(EntityKind::Task))),
        Some(Kind::Type) => out.extend(standard_types()),
        _ => {}
    }
    out
}

/// An option as the command line reads it.
struct Spec {
    long: Option<String>,
    short: Option<char>,
    takes_value: bool,
}

fn specs(cmd: &clap::Command) -> Vec<Spec> {
    let spec = |a: &clap::Arg| Spec { long: a.get_long().map(str::to_string), short: a.get_short(), takes_value: a.get_action().takes_values() };
    cmd.get_arguments().filter(|a| !a.is_positional()).map(spec).collect()
}

fn is_number(t: &str) -> bool {
    t.parse::<f64>().is_ok()
}

/// The words before the cursor read as the real parse reads them: options and their values
/// set aside, the first word the command.
struct Line {
    command: Option<String>,
    /// The first word names no command.
    unknown: bool,
    /// The cursor is on an option's value.
    on_value: bool,
    words: Vec<String>,
    specs: Vec<Spec>,
}

fn read_line(ctx: &mut Context, root: &clap::Command, before: &[String]) -> Line {
    let mut l = Line { command: None, unknown: false, on_value: false, words: Vec::new(), specs: specs(root) };
    let mut options_done = false;
    let mut i = 0;
    while i < before.len() {
        let t = &before[i];
        i += 1;
        if options_done || t.len() < 2 || !t.starts_with('-') || is_number(t) {
            match (&l.command, l.unknown, root.find_subcommand(t)) {
                (Some(_), _, _) | (_, true, _) => l.words.push(t.clone()),
                (None, false, Some(sub)) => {
                    l.command = Some(sub.get_name().to_string());
                    l.specs.extend(specs(sub));
                }
                (None, false, None) => l.unknown = true,
            }
            continue;
        }
        if t == "--" {
            options_done = true;
            continue;
        }
        let long = t.strip_prefix("--").map(|n| n.split_once('=').map_or(n, |(n, _)| n));
        let short = (t.len() == 2).then(|| t.chars().nth(1)).flatten();
        let Some(spec) = l.specs.iter().find(|s| long.map_or(s.short == short && short.is_some(), |n| s.long.as_deref() == Some(n))) else { continue };
        if !spec.takes_value {
            continue;
        }
        let value = match t.split_once('=') {
            Some((_, v)) if t.starts_with("--") => Some(v.to_string()),
            _ if i < before.len() => {
                i += 1;
                Some(before[i - 1].clone())
            }
            _ => None,
        };
        let Some(value) = value else {
            l.on_value = true;
            continue;
        };
        if spec.long.as_deref() == Some("domain") {
            ctx.domain_option = value.parse().ok().or(ctx.domain_option);
        }
    }
    l
}

fn answer(ctx: &mut Context, before: &[String], partial: &str) -> Vec<String> {
    let mut root = crate::cli::command();
    root.build();
    let l = read_line(ctx, &root, before);
    if l.on_value || l.unknown {
        return Vec::new();
    }
    let c = if partial.starts_with('-') && !is_number(partial) {
        words(l.specs.iter().filter_map(|s| s.long.as_ref().map(|n| format!("--{n}"))).collect())
    } else if let Some(command) = &l.command {
        let mut r = Request { ctx, words: l.words, partial: partial.to_string(), workspace: None, mesh: None };
        offers(command, &mut r)
    } else {
        words(root.get_subcommands().filter(|s| !s.is_hide_set()).map(|s| s.get_name().to_string()).collect())
    };
    narrow(&c, partial)
}

/// Answers one `__complete` request on stdout. Never fails: a broken workspace or a silent
/// mesh only means fewer candidates.
pub fn run(tokens: &[String]) -> i32 {
    let (partial, before) = match tokens.first().and_then(|t| t.strip_prefix("--cur=")) {
        Some(p) => (p.to_string(), &tokens[1..]),
        None => (String::new(), tokens),
    };
    let mut ctx = Context::new(Output::new(Format::Text, false), false, None);
    let mut out = std::io::stdout().lock();
    for w in answer(&mut ctx, before, &partial) {
        let _ = writeln!(out, "{w}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(list: &[&str], path: bool) -> Candidates {
        Candidates { words: list.iter().map(|s| s.to_string()).collect(), paths: path }
    }

    #[test]
    fn narrow_keeps_what_starts_with_the_word_sorted_once() {
        let c = of(&["stop", "start", "sub", "start"], false);
        assert_eq!(narrow(&c, "st"), ["start", "stop"]);
        assert!(narrow(&c, "x").is_empty());
    }

    #[test]
    fn narrow_ignores_case() {
        let c = of(&["Nav", "nav_tool", "talker"], false);
        assert_eq!(narrow(&c, "nav"), ["Nav", "nav_tool"]);
        assert_eq!(narrow(&c, "TAL"), ["talker"]);
    }

    #[test]
    fn narrow_offers_a_group_and_a_node_of_one_name_once_as_typed() {
        let c = of(&["Transcoder", "transcoder", "WebExplorer", "web_explorer", "talker"], false);
        assert_eq!(narrow(&c, "transco"), ["transcoder"]);
        assert_eq!(narrow(&c, "Transco"), ["Transcoder"]);
        assert_eq!(narrow(&c, "web"), ["web_explorer"]);
        assert_eq!(narrow(&c, ""), ["Transcoder", "WebExplorer", "talker"]);
    }

    #[test]
    fn narrow_keeps_paths_that_differ_in_case() {
        assert_eq!(narrow(&of(&["/Cam", "/cam"], true), ""), ["/Cam", "/cam"]);
    }

    #[test]
    fn narrow_offers_paths_one_segment_at_a_time() {
        let c = of(&["/camera/left", "/camera/right", "/odom", "/odom/rate"], true);
        assert_eq!(narrow(&c, ""), ["/camera/", "/odom", "/odom/"]);
        assert_eq!(narrow(&c, "/cam"), ["/camera/"]);
        assert_eq!(narrow(&c, "/camera/"), ["/camera/left", "/camera/right"]);
        let bare = of(&["camera/left", "camera/rate"], true);
        assert_eq!(narrow(&bare, ""), ["camera/"]);
        assert_eq!(narrow(&bare, "camera/"), ["camera/left", "camera/rate"]);
    }

    #[test]
    fn narrow_leaves_slashes_alone_outside_paths() {
        assert_eq!(narrow(&of(&["demo/sensor", "sensor"], false), ""), ["demo/sensor", "sensor"]);
    }

    #[test]
    fn params_offer_keys_then_values() {
        let p = |name: &str, values: &[&str]| Offered { name: name.into(), values: values.iter().map(|v| v.to_string()).collect() };
        let g = [p("target", &["red", "blue"]), p("fast", &["true", "false"]), p("speed", &[])];
        assert_eq!(params(&g, "", &[]), ["target=", "fast=", "speed="]);
        assert_eq!(params(&g, "", &["fast=true".into()]), ["target=", "speed="]);
        assert_eq!(params(&g, "target=", &[]), ["target=red", "target=blue"]);
        assert_eq!(params(&g, "fast=", &[]), ["fast=true", "fast=false"]);
        assert!(params(&g, "speed=", &[]).is_empty());
    }
}
