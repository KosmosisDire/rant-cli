//! What run, stop or restart did to nodes: a header, a row per node, a closing line. It redraws
//! on stderr while it works, or with --lines prints each row as it comes and changes.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use rant_process::Stopped;
use serde::Serialize;

use crate::context::Context;
use crate::failure::{Failure, Result};
use crate::logs;
use crate::mesh::Client;
use crate::nodes::{self, GRACE};
use crate::output::{Data, Format, LiveView, Style, Table, Text};
use crate::state::{Instance, group_of_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RowState {
    /// Started, not up on the mesh yet.
    Starting,
    Running,
    /// Exited soon after it started, or never came up.
    Failed,
    Stopped,
    /// Running, but started outside rant, which leaves it be. Reads as running in JSON.
    #[serde(rename = "running")]
    Outside,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    /// Its name on the mesh.
    pub name: String,
    pub state: RowState,
    /// "started", "stopped", "already running".
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u64>,
    /// The log whose end shows below a failed node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
}

impl Row {
    pub fn new(name: &str, state: RowState, status: &str, detail: &str) -> Row {
        Row { name: name.into(), state, status: status.into(), detail: detail.into(), pid: None, log: None, running: None }
    }

    /// A started node's row: its pid and log, and status starting until it comes up.
    pub fn started(rec: &Instance) -> Row {
        Row { pid: Some(rec.pid), log: Some(rec.log.clone()), ..Row::new(&rec.name, RowState::Starting, "starting", "") }
    }

    /// A node of the plan already running outside rant, which neither starts nor stops it.
    pub fn outside(name: &str) -> Row {
        Row::new(name, RowState::Outside, "already running", "started outside rant, left as it is")
    }

    /// A stopped node's row, saying how it stopped.
    pub fn stopped(name: &str, how: Stopped) -> Row {
        let (status, detail) = match how {
            Stopped::Gracefully => ("stopped", String::new()),
            Stopped::Killed => ("killed", format!("did not stop within {} s", GRACE.as_secs())),
            Stopped::KilledAtOnce => ("killed", "Windows cannot ask it to stop".into()),
            Stopped::AlreadyGone => ("already exited", String::new()),
        };
        Row::new(name, RowState::Stopped, status, &detail)
    }

    fn mark(&self, t: &Text) -> String {
        match self.state {
            RowState::Starting | RowState::Running => t.paint(Style::Green, "\u{25cf}"),
            RowState::Outside => t.paint(Style::Amber, "\u{25cf}"),
            RowState::Stopped => t.paint(Style::Dim, "\u{25cb}"),
            RowState::Failed => t.paint(Style::Red, "\u{2715}"),
        }
    }

    /// The pid shown while it runs, lined up over digits, then its log.
    fn detail(&self, t: &Text, digits: usize) -> String {
        let mut detail = self.detail.clone();
        if let (Some(pid), Some(log), RowState::Starting | RowState::Running) = (self.pid.filter(|&p| p != 0), &self.log, self.state) {
            let pid = pid.to_string();
            let gap = if detail.is_empty() { "" } else { "   " };
            detail += &format!("{gap}pid {pid}{}{}", " ".repeat(digits.saturating_sub(pid.len()) + 3), t.path(Path::new(log)));
        }
        t.paint(Style::Dim, &detail)
    }
}

/// Why a node other roots hold stays up: "started on its own" for its own start, then lead and
/// the groups, such as "shared with nav" or "still needed by nav".
pub fn held_by(root_keys: &[String], lead: &str) -> String {
    let groups: Vec<&str> = root_keys.iter().filter_map(|k| group_of_key(k)).collect();
    let alone = root_keys.iter().any(|k| group_of_key(k).is_none()).then_some("started on its own");
    match (alone, groups.is_empty()) {
        (_, true) => alone.unwrap_or_default().to_string(),
        (Some(a), false) => format!("{a}, {lead} {}", groups.join(", ")),
        (None, false) => format!("{lead} {}", groups.join(", ")),
    }
}

/// The block as data: the rows, and for a person the header and the closing line.
#[derive(Debug, Serialize)]
pub struct Report {
    #[serde(rename = "nodes")]
    pub rows: Vec<Row>,
    #[serde(skip)]
    header: String,
    #[serde(skip)]
    params: String,
    #[serde(skip)]
    pub closing: String,
}

impl Report {
    fn head(&self, t: &Text) -> Vec<String> {
        if self.header.is_empty() {
            return Vec::new();
        }
        let params = if self.params.is_empty() { String::new() } else { format!("  {}", t.paint(Style::Dim, &self.params)) };
        vec![t.paint(Style::Bold, &self.header) + &params, String::new()]
    }

    /// The end of each failed node's log, then the closing line, each after a blank line
    /// unless it comes first.
    fn tail(&self, t: &Text) -> Vec<String> {
        let mut out = Vec::new();
        for r in self.rows.iter().filter(|r| r.state == RowState::Failed) {
            let Some(log) = &r.log else { continue };
            out.push(String::new());
            out.push(format!("  {}, the end of {}:", r.name, t.path(Path::new(log))));
            out.extend(logs::tail(Path::new(log), 10).iter().map(|l| format!("    {}", t.paint(Style::Dim, l))));
        }
        if !self.closing.is_empty() {
            out.extend([String::new(), self.closing.clone()]);
        }
        if self.rows.is_empty() && self.header.is_empty() {
            out.retain(|l| !l.is_empty());
        }
        out
    }

    fn lines(&self, t: &Text) -> Vec<String> {
        let digits = self.rows.iter().filter_map(|r| r.pid.filter(|&p| p != 0)).map(|p| p.to_string().len()).max().unwrap_or(0);
        let mut table = Table::default();
        for r in &self.rows {
            table.row(vec![format!("{} {}", r.mark(t), r.name), r.status.clone(), r.detail(t, digits)]);
        }
        let mut out = self.head(t);
        out.extend(table.lines("  "));
        out.extend(self.tail(t));
        out
    }

    /// The rows counted by status in the order they first come: "2 stopped, 1 kept running".
    pub fn counts(&self) -> String {
        let mut seen: Vec<(&str, usize)> = Vec::new();
        for r in &self.rows {
            match seen.iter_mut().find(|(s, _)| *s == r.status) {
                Some((_, n)) => *n += 1,
                None => seen.push((&r.status, 1)),
            }
        }
        seen.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ")
    }
}

impl Data for Report {
    fn text(&self, t: &Text) -> String {
        self.lines(t).join("\n")
    }
}

enum Progress {
    /// Redrawn on stderr, made at the first draw.
    Live(Option<LiveView>),
    /// Each row on stdout as it comes and changes: what was printed of each.
    Lines { sent: Vec<String> },
    /// Shown once at the end.
    Quiet,
}

pub struct Summary {
    pub report: Report,
    progress: Progress,
    /// A node or the build failed, so rant exits 1.
    pub failed: bool,
}

impl Summary {
    /// header is the group, empty for nodes on their own. params follow it, dim.
    pub fn new(ctx: &Context, header: &str, params: &str) -> Summary {
        let progress = if ctx.out.lines() {
            Progress::Lines { sent: Vec::new() }
        } else if ctx.out.redraws() {
            Progress::Live(None)
        } else {
            Progress::Quiet
        };
        let report = Report { rows: Vec::new(), header: header.into(), params: params.into(), closing: String::new() };
        Summary { report, progress, failed: false }
    }

    pub fn add(&mut self, ctx: &Context, r: Row) {
        self.report.rows.push(r);
        self.show(ctx);
    }

    pub fn row(&mut self, name: &str) -> &mut Row {
        self.report.rows.iter_mut().find(|r| r.name == name).expect("every started node has a row")
    }

    /// Draws what changed.
    pub fn show(&mut self, ctx: &Context) {
        match &mut self.progress {
            Progress::Live(view) => {
                let lines = self.report.lines(&ctx.out.text_err(ctx.from()));
                view.get_or_insert_with(LiveView::new).draw(&lines);
            }
            Progress::Lines { sent } => {
                let t = ctx.out.text_out(ctx.from());
                if sent.is_empty() && !self.report.rows.is_empty() && ctx.out.format() == Format::Text {
                    self.report.head(&t).iter().for_each(|l| ctx.out.line(l));
                }
                for (i, r) in self.report.rows.iter().enumerate() {
                    let now = serde_json::to_string(r).unwrap_or_default();
                    if sent.get(i) == Some(&now) {
                        continue;
                    }
                    let text = format!("  {} {}  {}  {}", r.mark(&t), r.name, r.status, r.detail(&t, 0));
                    ctx.out.record(r, text.trim_end());
                    match sent.get_mut(i) {
                        Some(s) => *s = now,
                        None => sent.push(now),
                    }
                }
            }
            Progress::Quiet => {}
        }
    }

    /// Prints the block: in full, or with --lines only what did not print yet. Fails quietly
    /// when a node or the build failed, since the block says why.
    pub fn finish(mut self, ctx: &Context) -> Result<()> {
        match std::mem::replace(&mut self.progress, Progress::Quiet) {
            Progress::Live(view) => {
                if let Some(v) = view {
                    v.erase();
                }
                ctx.show(&self.report);
            }
            Progress::Lines { .. } if ctx.out.format() == Format::Text => {
                self.report.tail(&ctx.out.text_out(ctx.from())).iter().for_each(|l| ctx.out.line(l));
            }
            Progress::Lines { .. } => {}
            Progress::Quiet => ctx.show(&self.report),
        }
        if self.failed { Err(Failure::Reported) } else { Ok(()) }
    }
}

/// How long run and restart wait for started nodes, and then watch them.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Watch {
    /// start what is built already, without building first
    #[arg(long)]
    pub no_build: bool,
    /// seconds to wait for started nodes to come up on the mesh, 10 by default, 0 for none
    #[arg(long, value_name = "S", value_parser = seconds)]
    pub wait: Option<f64>,
    /// seconds to watch them once up for an early exit, 2 by default, 0 for none
    #[arg(long, value_name = "S", value_parser = seconds)]
    pub watch: Option<f64>,
}

fn seconds(text: &str) -> std::result::Result<f64, String> {
    text.parse::<f64>().ok().filter(|s| *s >= 0.0 && s.is_finite()).ok_or_else(|| "takes seconds, 0 or more".into())
}

/// Seconds with at most decimals places and no trailing zeros: 2, 0.4, 1.25.
fn secs(s: f64, decimals: usize) -> String {
    let t = format!("{s:.decimals$}");
    if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t }
}

/// Waits for started nodes to show on the mesh, then watches them a moment, since one that
/// crashes at once otherwise looks started. group is what `rant log` shows when several failed.
pub fn bring_up(ctx: &mut Context, summary: &mut Summary, data: &Path, started: Vec<Instance>, group: &str, verb: &str, w: &Watch) -> Result<()> {
    let (wait, span) = (w.wait.unwrap_or(10.0), w.watch.unwrap_or(2.0));
    let home = ctx.home()?.to_path_buf();
    let started = nodes::hand_over(&home, data, started)?;
    for rec in &started {
        summary.row(&rec.name).pid = Some(rec.pid);
    }
    let begin = Instant::now();
    let mut exited: BTreeMap<String, Duration> = BTreeMap::new();
    let mut late: Vec<String> = Vec::new();

    // a node is up once its name is among its domain's peers, a program that never joins the
    // mesh once started
    let mut waiting: Vec<&Instance> = Vec::new();
    for rec in &started {
        if rec.expect_mesh && wait > 0.0 {
            waiting.push(rec);
        } else {
            summary.row(&rec.name).status = verb.into();
            summary.row(&rec.name).state = RowState::Running;
        }
    }
    summary.show(ctx);
    if !waiting.is_empty() {
        let fallback = ctx.domain()?;
        let domain_of = |rec: &Instance| rec.env.get("RANT_DOMAIN").and_then(|d| d.parse().ok()).unwrap_or(fallback);
        let mut meshes: BTreeMap<u16, Client> = BTreeMap::new();
        for rec in &waiting {
            let d = domain_of(rec);
            if let std::collections::btree_map::Entry::Vacant(e) = meshes.entry(d) {
                e.insert(Client::new(d, Some(&home))?);
            }
        }
        let end = begin + Duration::from_secs_f64(wait);
        while !waiting.is_empty() {
            summary.report.closing = ctx.out.paint_err(Style::Dim, &format!("waiting for {} to come up", waiting.len()));
            waiting.retain(|rec| {
                if meshes[&domain_of(rec)].peers().iter().any(|p| p.name == rec.name) {
                    let row = summary.row(&rec.name);
                    (row.status, row.state) = (verb.into(), RowState::Running);
                    return false;
                }
                if !rant_process::alive(&rec.tracking()) {
                    exited.insert(rec.name.clone(), begin.elapsed());
                    return false;
                }
                true
            });
            summary.show(ctx);
            if waiting.is_empty() || Instant::now() >= end {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        late = waiting.iter().map(|r| r.name.clone()).collect();
    }
    // what came up must stay up a moment
    let up: Vec<Instance> = started.iter().filter(|r| !exited.contains_key(&r.name) && !late.contains(&r.name)).cloned().collect();
    let counts = summary.report.counts();
    if span > 0.0 && !up.is_empty() {
        summary.report.closing = ctx.out.paint_err(Style::Dim, &format!("watching them for {} s", secs(span, 2)));
        summary.show(ctx);
        let offset = begin.elapsed();
        for g in nodes::exited_within(&up, Duration::from_secs_f64(span)) {
            exited.insert(g.name, offset + g.after);
        }
    }

    let failed = |summary: &mut Summary, name: &str, status: String| {
        let row = summary.row(name);
        let log = row.log.clone().unwrap_or_default();
        (row.state, row.status, row.running) = (RowState::Failed, status, Some(false));
        row.detail = ctx.shown(Path::new(&log));
    };
    for (name, after) in &exited {
        failed(summary, name, format!("exited after {} s", secs(after.as_secs_f64(), 1)));
    }
    for name in &late {
        failed(summary, name, format!("not up after {} s", secs(wait, 2)));
    }
    if exited.is_empty() && late.is_empty() {
        let all = if span > 0.0 && !up.is_empty() { format!(", all running after {} s", secs(span, 2)) } else { String::new() };
        summary.report.closing = counts + &all;
    } else {
        let mut bad: Vec<String> = exited.keys().map(|n| format!("{n} exited")).collect();
        bad.extend(late.iter().map(|n| format!("{n} never came up")));
        let rows = &summary.report.rows;
        let running = rows.iter().filter(|r| matches!(r.state, RowState::Running | RowState::Starting | RowState::Outside)).count();
        let target = if exited.len() + late.len() == 1 || group.is_empty() { exited.keys().next().unwrap_or_else(|| &late[0]).clone() } else { group.to_string() };
        summary.report.closing = format!("{running} of {} running, {}. `rant log {target}` shows why", rows.len(), crate::output::spoken(&bad));
        summary.failed = true;
    }
    summary.show(ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_by_names_its_own_start_then_the_groups() {
        let keys = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(held_by(&keys(&["group nav a=1", "group pick"]), "shared with"), "shared with nav, pick");
        assert_eq!(held_by(&keys(&["node talker", "group nav"]), "still needed by"), "started on its own, still needed by nav");
        assert_eq!(held_by(&keys(&["node talker"]), "inside"), "started on its own");
    }

    #[test]
    fn seconds_drop_trailing_zeros_and_lists_end_with_and() {
        assert_eq!(secs(2.0, 2), "2");
        assert_eq!(secs(0.4, 1), "0.4");
        assert_eq!(secs(1.25, 2), "1.25");
        assert!(seconds("-1").is_err() && seconds("x").is_err() && seconds("0.5") == Ok(0.5));
    }

    #[test]
    fn rows_serialize_in_the_order_the_manager_reads() {
        let r = Row { pid: Some(5), log: Some("l".into()), running: Some(false), ..Row::new("brief", RowState::Failed, "exited after 0.3 s", "logs/brief.log") };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"name":"brief","state":"failed","status":"exited after 0.3 s","detail":"logs/brief.log","pid":5,"log":"l","running":false}"#);
        let r = Row::new("a", RowState::Running, "already running", "");
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"name":"a","state":"running","status":"already running"}"#);
        let r = Row::outside("a");
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"name":"a","state":"running","status":"already running","detail":"started outside rant, left as it is"}"#);
    }

    #[test]
    fn counts_go_by_status_in_first_order() {
        let rows = vec![Row::stopped("a", Stopped::Gracefully), Row::new("b", RowState::Running, "kept running", ""), Row::stopped("c", Stopped::Gracefully)];
        let r = Report { rows, header: String::new(), params: String::new(), closing: String::new() };
        assert_eq!(r.counts(), "2 stopped, 1 kept running");
    }
}
