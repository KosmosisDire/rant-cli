//! `rant pub`: one value to a topic or a variable, or the same value at a rate. A variable's
//! write counts once its owner holds the value.

use std::time::{Duration, Instant};

use rant::{Bytes, EntityKind, Qos, SendError, VariableOptions};

use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::interrupt;
use crate::mesh::{Client, FULL_SETTLE, encode, parse_words, send_failure};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the topic or variable, then the value: one, such as `2.5` or `{x: 1}`, or `field=value` words
    #[arg(value_name = "name", required = true)]
    pub words: Vec<String>,
    /// send this many times a second until Ctrl-C
    #[arg(short, long, value_name = "HZ")]
    pub rate: Option<f64>,
    /// stop after N values
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub count: Option<u64>,
}

/// Sends one value, or the same value at a rate until the count or Ctrl-C. Returns how many
/// went. what names the act in a failure, such as "publish to `chatter`".
fn send_all(what: &str, rate: Option<f64>, count: Option<u64>, mut send: impl FnMut() -> std::result::Result<(), SendError>) -> Result<u64> {
    let period = rate.map_or(Duration::ZERO, |r| Duration::from_secs_f64(1.0 / r));
    let count = count.or(if rate.is_some() { None } else { Some(1) });
    let mut next = Instant::now();
    let mut sent = 0;
    while !interrupt::interrupted() && count.is_none_or(|n| sent < n) {
        send().map_err(|e| failed(format!("cannot {what}: {}", send_failure(e))))?;
        sent += 1;
        if count.is_some_and(|n| sent >= n) {
            break;
        }
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    Ok(sent)
}

fn times(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let [name, value @ ..] = args.words.as_slice() else { unreachable!("clap requires a name") };
    if value.is_empty() {
        return Err(usage("pub takes a topic or variable and a value, such as `rant pub speed 2.5`"));
    }
    let values = parse_words(value)?;
    if args.rate.is_some_and(|r| r <= 0.0 || !r.is_finite()) {
        return Err(usage("--rate takes values a second, more than 0"));
    }
    interrupt::catch();
    let mesh = Client::new(ctx.domain()?, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let e = mesh.require_entity(&[EntityKind::Topic, EntityKind::Variable], name)?;
    let node = mesh.node();
    let refused = |e: rant::Error| failed(e.to_string());

    if e.kind == EntityKind::Topic {
        let publisher = node.publisher::<Bytes>(name, Qos::new().reflect_from_mesh(true)).map_err(refused)?;
        let bytes = encode(&values, publisher.schema().as_ref())?;
        let sent = send_all(&format!("publish to `{name}`"), args.rate, args.count, || publisher.send(&bytes))?;
        publisher.drain(Duration::from_secs(1));
        ctx.out.note(&format!("published {} to {name}", times(sent, "message", "messages")));
        return Ok(());
    }
    if !e.writable {
        return Err(failed(format!("`{name}` is read only")));
    }
    let var = node.remote_variable::<Bytes>(name, VariableOptions::new().reflect_from_mesh(true)).map_err(refused)?;
    if !var.wait(Duration::from_secs(2)) {
        return Err(failed(format!("the owner of `{name}` did not answer in time")));
    }
    let bytes = encode(&values, var.schema().as_ref())?;
    let sent = send_all(&format!("write `{name}`"), args.rate, args.count, || var.set(&bytes))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while var.get().ok().flatten().as_ref() != Some(&bytes) {
        if Instant::now() > deadline {
            return Err(failed(format!("the owner of `{name}` did not apply the value in time")));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ctx.out.note(&format!("wrote {} to {name}", times(sent, "value", "values")));
    Ok(())
}
