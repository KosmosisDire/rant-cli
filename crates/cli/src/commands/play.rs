//! `rant play`: a recording sent again as it came, relative to now, faster or slower. Every
//! recorded time moves by one map, while nodes' own timers and rates stay on the real clock.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rant::types::Timestamp;
use rant::{Bytes, EntityKind, FieldType, NodeOptions, Qos, Schema, VariableOptions};
use serde::Serialize;

use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::interrupt;
use crate::mesh::{Client, FULL_SETTLE, kind_name, send_failure};
use crate::names::selects;
use crate::output::{Data, LiveView, Style, Text, spoken};
use crate::recording::{self, Body, Reader, Track, now_us};

/// How many names one player can send, each a handle on its node.
const MAX_NAMES: u16 = 512;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the recording, a file or a name `rant ls recordings` lists
    #[arg(value_name = "recording")]
    pub file: PathBuf,
    /// how much faster than it was recorded, 0.5 for half as fast
    #[arg(long, value_name = "factor", default_value_t = 1.0)]
    pub speed: f64,
    /// begin this many seconds in, each variable first set to its value there
    #[arg(long, value_name = "seconds", default_value_t = 0.0)]
    pub start: f64,
    /// begin again when the end comes, until Ctrl-C
    #[arg(long = "loop")]
    pub again: bool,
    /// play only the names it matches, whole or with * and ?
    #[arg(long, value_name = "name")]
    pub only: Vec<String>,
    /// leave out the names it matches, whole or with * and ?
    #[arg(long, value_name = "name")]
    pub ignore: Vec<String>,
    /// play every name under this prefix, such as Replay, beside the live ones
    #[arg(long, value_name = "prefix")]
    pub prefix: Option<String>,
}

/// Where a track's messages go, and the offsets of the Timestamp fields to move.
enum Out {
    Topic(rant::Publisher<Bytes>),
    Variable(rant::VariableDefinition<Bytes>),
}

struct Sender {
    name: String,
    /// The schema's hash, 0 for none.
    hash: u64,
    out: Out,
    stamps: Vec<usize>,
    /// A send failed and was said once.
    failing: bool,
}

#[derive(Serialize)]
struct Played {
    file: String,
    messages: u64,
    names: usize,
    /// Of the recording, every pass counted.
    seconds: f64,
    took: f64,
}

impl Data for Played {
    fn text(&self, _: &Text) -> String {
        let of = if self.names == 1 { "1 name".to_string() } else { format!("{} names", self.names) };
        format!("played {} messages of {of}, {:.1} s of recording in {:.1} s", self.messages, self.seconds, self.took)
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if !args.speed.is_finite() || args.speed <= 0.0 {
        return Err(usage("--speed takes a factor above 0, such as 2 or 0.5"));
    }
    if !args.start.is_finite() || args.start < 0.0 {
        return Err(usage("--start takes a number of seconds from 0 up"));
    }
    interrupt::catch();
    let path = recording::locate(ctx, &args.file)?;
    let contents = recording::contents(&path)?;
    if let Some(cut) = &contents.cut {
        ctx.out.warn(cut);
    }
    let wanted = |name: &str| (args.only.is_empty() || args.only.iter().any(|p| selects(p, name))) && !args.ignore.iter().any(|p| selects(p, name));
    let tracks: Vec<&Track> = contents.tracks.iter().map(|(t, _)| t.as_ref()).filter(|t| wanted(&t.name)).collect();
    let calls: Vec<&str> = tracks.iter().filter(|t| matches!(t.kind, EntityKind::Function | EntityKind::Task)).map(|t| t.name.as_str()).collect();
    if !calls.is_empty() {
        let calls: Vec<String> = dedup(calls).iter().map(|n| format!("`{n}`")).collect();
        ctx.out.note(&format!("calls are not played yet, so {} {} left out", spoken(&calls), if calls.len() == 1 { "is" } else { "are" }));
    }
    let played: Vec<&Track> = tracks.into_iter().filter(|t| matches!(t.kind, EntityKind::Topic | EntityKind::Variable)).collect();
    if played.is_empty() {
        return Err(failed("nothing in the recording to play, see `rant info recording`"));
    }
    let from_us = contents.first_us + (args.start * 1e6) as u64;
    if from_us > contents.last_us {
        return Err(usage(format!("the recording is {:.1} s long", (contents.last_us - contents.first_us) as f64 / 1e6)));
    }

    let domain = ctx.domain()?;
    let mut options = NodeOptions::new().max_topics(MAX_NAMES);
    if let Some(p) = &args.prefix {
        options = options.prefix(p.as_str());
    }
    let mesh = Client::open("player", options, domain, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let on_mesh = |name: &str| args.prefix.as_ref().map_or_else(|| name.to_string(), |p| format!("{p}/{name}"));
    check(ctx, &mesh, &played.iter().map(|t| on_mesh(&t.name)).collect::<Vec<_>>(), domain, args.prefix.is_some())?;

    let mut senders: Vec<Sender> = Vec::new();
    let mut of_track: HashMap<u16, usize> = HashMap::new();
    for t in &played {
        let schema = match &t.schema {
            Some(wire) => Some(mesh.node().schema_from_wire(wire)?),
            None => None,
        };
        // tracks of one name and schema from several senders go out as one
        let hash = schema.as_ref().map_or(0, Schema::hash);
        let index = match senders.iter().position(|s| s.name == t.name && s.hash == hash) {
            Some(i) => i,
            None => match open(&mesh, t, schema.as_ref()) {
                Ok(out) => {
                    senders.push(Sender { name: t.name.clone(), hash, out, stamps: schema.as_ref().map(stamp_offsets).unwrap_or_default(), failing: false });
                    senders.len() - 1
                }
                Err(f) => {
                    f.messages().iter().for_each(|m| ctx.out.warn(&format!("cannot play `{}`: {m}", t.name)));
                    continue;
                }
            },
        };
        of_track.insert(t.id, index);
    }
    wait_for_subscribers(&senders, Duration::from_secs(2));

    let mut player = Player { senders, of_track, from_us, speed: args.speed, total_s: (contents.last_us - from_us) as f64 / 1e6, view: ctx.out.redraws().then(LiveView::new) };
    let began = Instant::now();
    let (mut messages, mut seconds) = (0u64, 0.0);
    loop {
        let (sent, spanned) = player.pass(ctx, &path)?;
        (messages, seconds) = (messages + sent, seconds + spanned);
        if !args.again || interrupt::interrupted() {
            break;
        }
    }
    for s in &player.senders {
        if let Out::Topic(p) = &s.out {
            p.drain(Duration::from_secs(1));
        }
    }
    if let Some(v) = player.view.take() {
        v.erase();
    }
    let names = player.senders.len();
    ctx.show(&Played { file: ctx.shown(&path), messages, names, seconds, took: began.elapsed().as_secs_f64() });
    Ok(())
}

fn dedup(mut names: Vec<&str>) -> Vec<&str> {
    names.sort_unstable();
    names.dedup();
    names
}

/// Refuses names a live node publishes or owns, and asks before feeding names live nodes read.
fn check(ctx: &Context, mesh: &Client, names: &[String], domain: u16, prefixed: bool) -> Result<()> {
    let live = mesh.entities();
    let (mut owned, mut read) = (Vec::new(), Vec::new());
    for e in live.iter().filter(|e| names.contains(&e.name)) {
        if e.providers > 0 {
            owned.push(format!("{} {} from {}", kind_name(e.kind), e.name, mesh.provider_of(e)));
        } else if e.consumers > 0 && !read.contains(&e.name) {
            read.push(e.name.clone());
        }
    }
    if !owned.is_empty() {
        let ways = "play them under a prefix with --prefix, on another domain with --domain, or leave them out with --ignore";
        return Err(failed(format!("live nodes on domain {domain} send these, and a second source would mix with them:\n  {}\n{ways}", owned.join("\n  "))));
    }
    if read.is_empty() {
        return Ok(());
    }
    let what = format!("live nodes on domain {domain} read {}, and would act on recorded data", spoken(&read));
    if !ctx.yes && !std::io::stdin().is_terminal() {
        let ways = if prefixed { "with --yes" } else { "with --yes or under a prefix with --prefix" };
        return Err(failed(format!("{what}, so play them {ways}")));
    }
    if !ctx.confirm(&format!("{what}. Play?")) {
        return Err(failed("nothing was played"));
    }
    Ok(())
}

/// The handle that sends a track: a topic's publisher, or a variable's owner that takes no sets.
fn open(mesh: &Client, t: &Track, schema: Option<&Schema>) -> Result<Out> {
    let node = mesh.node();
    match t.kind {
        EntityKind::Topic => {
            let mut qos = Qos::new().reliable(t.reliable);
            if let Some(s) = schema {
                qos = qos.schema(s);
            }
            Ok(Out::Topic(node.publisher::<Bytes>(&t.name, qos)?))
        }
        _ => {
            let schema = schema.ok_or_else(|| failed("it has no type"))?;
            Ok(Out::Variable(node.variable_definition::<Bytes>(&t.name, VariableOptions::new().schema(schema).read_only(true))?))
        }
    }
}

/// Where a schema holds a Timestamp outside every array, as byte offsets into a value.
fn stamp_offsets(schema: &Schema) -> Vec<usize> {
    let stamp = |f: &rant::Field| f.type_name.as_deref() == Some("Timestamp") && f.kind == FieldType::I64 && f.arr_parent == 0xFFFF;
    schema.fields().iter().filter(|f| stamp(f)).map(|f| f.offset as usize).collect()
}

/// Each Timestamp at the offsets through the map, a 0 left as it says no time.
fn move_stamps(data: &mut [u8], stamps: &[usize], map: &impl Fn(i64) -> i64) {
    for &at in stamps {
        if let Some(b) = data.get_mut(at..at + 8) {
            let t = i64::from_le_bytes(b.try_into().expect("eight bytes"));
            if t != 0 {
                b.copy_from_slice(&map(t).to_le_bytes());
            }
        }
    }
}

/// Waits until each topic has matched its subscribers or settled with none, so the first
/// messages reach the nodes already listening.
fn wait_for_subscribers(senders: &[Sender], limit: Duration) {
    let deadline = Instant::now() + limit;
    let waiting = || senders.iter().any(|s| matches!(&s.out, Out::Topic(p) if !p.ready()));
    while waiting() && Instant::now() < deadline && !interrupt::interrupted() {
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Player {
    senders: Vec<Sender>,
    of_track: HashMap<u16, usize>,
    from_us: u64,
    speed: f64,
    total_s: f64,
    view: Option<LiveView>,
}

impl Player {
    /// One pass from `from_us` to the end: the messages sent and the seconds of recording they spanned.
    fn pass(&mut self, ctx: &Context, path: &Path) -> Result<(u64, f64)> {
        let (anchor, anchor_us, from_us, speed) = (Instant::now(), now_us() as i64, self.from_us, self.speed);
        // the one map every recorded time takes: the start lands on now, the spacing scaled by speed
        let map = move |t: i64| anchor_us + ((t - from_us as i64) as f64 / speed) as i64;
        let mut before: HashMap<usize, Body> = HashMap::new();
        let (mut sent, mut last_us, mut started) = (0u64, from_us, false);
        let mut next_draw = anchor;
        for entry in Reader::open(path)? {
            let e = match entry {
                Ok(e) => e,
                Err(f) => {
                    f.messages().iter().for_each(|m| ctx.out.warn(m));
                    break;
                }
            };
            let Some(&i) = self.of_track.get(&e.track.id) else { continue };
            if e.log_us < from_us {
                if matches!(self.senders[i].out, Out::Variable(_)) {
                    before.insert(i, e.body);
                }
                continue;
            }
            if !started {
                // each variable as it stood where the pass begins
                for (j, body) in before.drain() {
                    send(ctx, &mut self.senders[j], body, &map);
                }
                started = true;
            }
            let due = anchor + Duration::from_micros(((e.log_us - from_us) as f64 / speed) as u64);
            while Instant::now() < due && !interrupt::interrupted() {
                std::thread::sleep((due - Instant::now()).min(Duration::from_millis(50)));
                self.draw(ctx, &mut next_draw, last_us, sent, Duration::ZERO);
            }
            if interrupt::interrupted() {
                break;
            }
            send(ctx, &mut self.senders[i], e.body, &map);
            (sent, last_us) = (sent + 1, e.log_us);
            self.draw(ctx, &mut next_draw, last_us, sent, Instant::now().saturating_duration_since(due));
        }
        for (j, body) in before.drain() {
            send(ctx, &mut self.senders[j], body, &map);
        }
        Ok((sent, (last_us - from_us) as f64 / 1e6))
    }

    /// The progress line, at most four times a second.
    fn draw(&mut self, ctx: &Context, next: &mut Instant, at_us: u64, sent: u64, behind: Duration) {
        let Some(view) = self.view.as_mut().filter(|_| Instant::now() >= *next) else { return };
        let mut line = format!("playing {:.1} of {:.1} s at {}x, {sent} messages", (at_us - self.from_us) as f64 / 1e6, self.total_s, self.speed);
        if behind > Duration::from_millis(100) {
            line += &ctx.out.paint_err(Style::Amber, &format!(", {:.1} s behind", behind.as_secs_f64()));
        }
        view.draw(&[line]);
        *next = Instant::now() + Duration::from_millis(250);
    }
}

/// Sends one value with its times moved. A failed send is said once per name.
fn send(ctx: &Context, s: &mut Sender, body: Body, map: &impl Fn(i64) -> i64) {
    let Body::Value { capture_us, mut data, .. } = body else { return };
    move_stamps(&mut data, &s.stamps, map);
    let result = match &s.out {
        Out::Topic(p) if capture_us != 0 => p.send_captured(&data, Timestamp { us: map(capture_us as i64) }),
        Out::Topic(p) => p.send(&data),
        Out::Variable(v) => v.set(&data),
    };
    match result {
        Ok(()) => s.failing = false,
        Err(e) if !s.failing => {
            ctx.out.warn(&format!("cannot send `{}`: {}", s.name, send_failure(e)));
            s.failing = true;
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rant::MessageBuilder;

    #[test]
    fn every_timestamp_outside_an_array_moves_and_nothing_else() {
        let node = crate::mesh::test_node();
        let schema = node.schema("Inner { at: Timestamp, n: u32 }
Stamped { label: string, at: Timestamp, x: f64, inner: Inner, unset: Timestamp, times: Timestamp[] }").unwrap();
        let stamps = stamp_offsets(&schema);
        assert_eq!(stamps.len(), 3, "at, inner.at and unset, not the array's elements");
        let mut b = MessageBuilder::new(&schema);
        b.set_string("label", "a long label before the stamp").unwrap();
        b.set_int("at", 1_000).unwrap().set_f64("x", 2.5).unwrap().set_int("inner.at", 2_000).unwrap().set_uint("inner.n", 7).unwrap();
        let mut data = b.bytes().to_vec();
        move_stamps(&mut data, &stamps, &|t| t + 5);
        let v = schema.view(&data);
        assert_eq!((v.get_int("at"), v.get_int("inner.at"), v.get_int("unset")), (1_005, 2_005, 0));
        assert_eq!((v.get_f64("x"), v.get_uint("inner.n"), v.get_string("label").as_ref()), (2.5, 7, "a long label before the stamp"));
    }
}
