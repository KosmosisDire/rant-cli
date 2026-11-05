//! `rant record`: every message of the names asked for into a recording, until Ctrl-C or a
//! duration. A name that turns up later joins once a node provides it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use rant::{Bytes, CallObserverOptions, Entity, EntityKind, NodeOptions, Qos, VariableOptions};
use serde::Serialize;

use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::interrupt;
use crate::mesh::{Client, FULL_SETTLE, is_placeholder, kind_name};
use crate::names::selects;
use crate::output::{Data, LiveView, Style, Text, bytes, iso_ms};
use crate::recording::{self, Body, EXTENSION, Sample, Step, Writer, now_us};

/// How many names one recording can hold, each a handle on the recorder's node.
const MAX_NAMES: u16 = 512;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the topics, variables, functions and tasks, each a whole name or one with * and ?, `*` for all
    #[arg(value_name = "name", required = true)]
    pub names: Vec<String>,
    /// leave out the names it matches, whole or with * and ?
    #[arg(long, value_name = "name")]
    pub ignore: Vec<String>,
    /// stop after this many seconds
    #[arg(long, value_name = "seconds")]
    pub duration: Option<f64>,
    /// the file to write, else one named by the time in the workspace's recordings folder
    #[arg(short, long, value_name = "file")]
    pub output: Option<PathBuf>,
}

/// The handle that keeps a name recorded, sending its samples to the writer until dropped.
type Watch = Box<dyn std::any::Any>;

/// One recorded name and how many messages it gave.
#[derive(Serialize)]
struct Name {
    name: String,
    kind: &'static str,
    messages: u64,
}

#[derive(Serialize)]
struct Recorded {
    file: String,
    seconds: f64,
    messages: u64,
    bytes: u64,
    names: Vec<Name>,
}

impl Data for Recorded {
    fn text(&self, t: &Text) -> String {
        let of = if self.names.len() == 1 { "1 name".to_string() } else { format!("{} names", self.names.len()) };
        let what = format!("recorded {} messages of {of} in {:.1} s, {}", self.messages, self.seconds, bytes(self.bytes));
        format!("{what}\n{}", t.paint(Style::Dim, &self.file))
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if args.duration.is_some_and(|d| !d.is_finite() || d <= 0.0) {
        return Err(usage("--duration takes a number of seconds above 0"));
    }
    interrupt::catch();
    let path = target(ctx, args)?;
    let domain = ctx.domain()?;
    let mesh = Client::open("recorder", NodeOptions::new().max_topics(MAX_NAMES), domain, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let about = BTreeMap::from([("domain".to_string(), domain.to_string()), ("names".to_string(), args.names.join(" ")), ("started".to_string(), iso_ms(now_us()))]);
    let mut writer = Writer::create(&path, about)?;
    let (tx, rx) = mpsc::channel::<Sample>();
    let mut names: BTreeMap<String, (EntityKind, Option<Watch>, u64)> = BTreeMap::new();
    let started = Instant::now();
    let deadline = args.duration.map(|s| started + Duration::from_secs_f64(s));
    let mut view = ctx.out.redraws().then(LiveView::new);
    let (mut next_scan, mut next_draw) = (started, started);
    let mut said_waiting = false;
    while !interrupt::interrupted() && deadline.is_none_or(|d| Instant::now() < d) {
        if Instant::now() >= next_scan {
            watch_new(ctx, &mesh, args, &mut names, &tx, view.is_none());
            next_scan += Duration::from_secs(1);
            if names.is_empty() && !said_waiting {
                ctx.out.note(&format!("nothing on domain {domain} matches yet, waiting"));
                said_waiting = true;
            }
        }
        let mut got = rx.recv_timeout(Duration::from_millis(50)).ok();
        while let Some(s) = got {
            if let Some(n) = names.get_mut(&s.name) {
                n.2 += 1;
            }
            writer.add(s)?;
            got = rx.try_recv().ok();
        }
        if let Some(v) = view.as_mut().filter(|_| Instant::now() >= next_draw) {
            v.draw(&progress(ctx, &path, started, &writer, &names));
            next_draw = Instant::now() + Duration::from_millis(250);
        }
    }
    // closing every handle ends the samples, and what came before still goes in
    for n in names.values_mut() {
        n.1 = None;
    }
    drop(tx);
    for s in rx.try_iter() {
        if let Some(n) = names.get_mut(&s.name) {
            n.2 += 1;
        }
        writer.add(s)?;
    }
    if let Some(v) = view {
        v.erase();
    }
    let (messages, written) = (writer.messages, writer.bytes);
    writer.finish()?;
    if messages == 0 {
        let _ = std::fs::remove_file(&path);
        return Err(failed("nothing was recorded, so no file was kept"));
    }
    let names = names.into_iter().map(|(name, (kind, _, messages))| Name { name, kind: kind_name(kind), messages }).collect();
    ctx.show(&Recorded { file: ctx.shown(&path), seconds: started.elapsed().as_secs_f64(), messages, bytes: written, names });
    Ok(())
}

/// The file asked for, else a new one in the recordings folder named by the UTC time.
fn target(ctx: &mut Context, args: &Args) -> Result<PathBuf> {
    let path = match &args.output {
        Some(p) if p.extension().is_none() => ctx.cwd()?.join(p).with_extension(EXTENSION),
        Some(p) => ctx.cwd()?.join(p),
        None => {
            // 2026-11-05T09:30:00.123Z becomes 2026-11-05_09-30-00Z
            let stamp = iso_ms(now_us())[..19].replace('T', "_").replace(':', "-");
            recording::folder(ctx)?.join(format!("{stamp}Z.{EXTENSION}"))
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| failed(format!("cannot make {}: {e}", ctx.shown(dir))))?;
    }
    Ok(path)
}

fn wanted(args: &Args, name: &str) -> bool {
    args.names.iter().any(|p| selects(p, name)) && !args.ignore.iter().any(|p| selects(p, name))
}

/// Starts recording each wanted name a node now provides. A name that cannot be watched is
/// said once and left out.
fn watch_new(ctx: &Context, mesh: &Client, args: &Args, names: &mut BTreeMap<String, (EntityKind, Option<Watch>, u64)>, tx: &Sender<Sample>, say: bool) {
    for e in mesh.entities() {
        if names.contains_key(&e.name) || !e.provides || e.incomplete || is_placeholder(&e.name) || !wanted(args, &e.name) {
            continue;
        }
        let watch = match watch(mesh, &e, tx.clone()) {
            Ok(w) => Some(w),
            Err(f) => {
                f.messages().iter().for_each(|m| ctx.out.warn(&format!("cannot record `{}`: {m}", e.name)));
                None
            }
        };
        if say && watch.is_some() {
            ctx.out.note(&format!("recording {} `{}`", kind_name(e.kind), e.name));
        }
        names.insert(e.name.clone(), (e.kind, watch, 0));
    }
}

fn watch(mesh: &Client, e: &Entity, tx: Sender<Sample>) -> Result<Watch> {
    let node = mesh.node();
    let (kind, name, reliable) = (e.kind, e.name.clone(), e.reliable);
    let sample = move |from: &str, schema, written_us, body| Sample { kind, name: name.clone(), from: from.to_string(), reliable, schema, log_us: now_us(), written_us, body };
    Ok(match e.kind {
        EntityKind::Topic => {
            let qos = Qos::new().reflect_from_mesh(true).reliable(e.reliable);
            Box::new(node.subscriber::<Bytes>(&e.name, qos, move |m| {
                let v = m.view();
                let body = Body::Value { capture_us: v.capture_us(), forced: false, data: v.data().to_vec() };
                let _ = tx.send(sample(v.publisher_name(), v.schema(), v.written_us(), body));
            })?)
        }
        EntityKind::Variable => {
            let var = node.remote_variable::<Bytes>(&e.name, VariableOptions::new().reflect_from_mesh(true))?;
            let (owner, schema) = (mesh.provider_of(e), e.schema.clone());
            var.on_change(move |u| {
                let body = Body::Value { capture_us: 0, forced: u.forced(), data: u.data().to_vec() };
                let _ = tx.send(sample(&owner, schema.clone(), u.written_us(), body));
            })
            .map_err(|e| failed(e.to_string()))?;
            Box::new(var)
        }
        EntityKind::Function | EntityKind::Task => Box::new(node.call_observer(&e.name, CallObserverOptions::new().reflect_from_mesh(true), move |c| {
            let step = Step {
                event: c.kind(),
                status: c.status(),
                call: c.call_id(),
                caller_lo: c.caller_lo(),
                caller: c.caller_name().to_string(),
                message: c.message().to_string(),
                data: c.data().to_vec(),
            };
            let _ = tx.send(sample(c.provider_name(), c.schema(), c.written_us(), Body::Call(step)));
        })?),
        _ => return Err(failed("it is of a kind this rant-cli does not know")),
    })
}

/// The lines redrawn while recording: the file and totals, then each name with its count.
fn progress(ctx: &Context, path: &Path, started: Instant, w: &Writer, names: &BTreeMap<String, (EntityKind, Option<Watch>, u64)>) -> Vec<String> {
    let paint = |s, text: &str| ctx.out.paint_err(s, text);
    let head = format!("recording {:.0} s, {} messages, {}", started.elapsed().as_secs_f64(), w.messages, bytes(w.bytes));
    let mut lines = vec![format!("{}  {}", paint(Style::Bold, &head), paint(Style::Dim, &ctx.shown(path)))];
    let width = names.keys().map(|n| n.chars().count()).max().unwrap_or(0);
    for (name, (kind, watch, count)) in names {
        let state = if watch.is_some() { count.to_string() } else { "left out".to_string() };
        lines.push(format!("  {name:width$}  {}  {state}", paint(Style::Dim, kind_name(*kind))));
    }
    lines
}
