//! `rant sub`: the newest value of a topic or a variable until Ctrl-C, or a line per value. A
//! function or task gives each step of its calls, which `calls` prints.

use std::sync::mpsc;
use std::time::Duration;

use rant::{Bytes, EntityKind, Qos, Schema, VariableOptions};

use crate::commands::calls;
use crate::commands::stream::{StreamArgs, ValueStream};
use crate::context::Context;
use crate::failure::{Result, failed};
use crate::interrupt;
use crate::mesh::{Client, FULL_SETTLE};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the topic, variable, function or task
    #[arg(value_name = "name")]
    pub name: String,
    /// stop after N values, or N calls ended
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub count: Option<u64>,
    #[command(flatten)]
    pub stream: StreamArgs,
}

/// Where the values come from: a topic's messages, or a variable's changes, the current value first.
enum Source {
    Topic(rant::Subscriber<Bytes>),
    Variable { var: rant::RemoteVariable<Bytes>, changes: mpsc::Receiver<Vec<u8>>, owner: String },
}

impl Source {
    /// Adds the next value to the stream, false when none came within wait.
    fn next(&mut self, out: &mut ValueStream, wait: Duration) -> Result<bool> {
        match self {
            Source::Topic(sub) => {
                let taken = sub.take(wait).map_err(|e| failed(e.to_string()))?;
                let Some(m) = taken else { return Ok(false) };
                let (from, data, schema): (String, Vec<u8>, Option<Schema>) = (m.publisher_name().to_string(), m.data().to_vec(), m.schema());
                out.add(&from, data, schema);
            }
            Source::Variable { var, changes, owner } => {
                let Ok(data) = changes.recv_timeout(wait) else { return Ok(false) };
                out.add(owner, data, var.schema());
            }
        }
        Ok(true)
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    interrupt::catch();
    let mesh = Client::new(ctx.domain()?, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let name = &args.name;
    let node = mesh.node();
    let reflection = node.reflection();
    if [EntityKind::Function, EntityKind::Task].into_iter().any(|k| reflection.find(k, name).is_some()) {
        return calls::run(ctx, &mesh, name, args);
    }
    let mut source = match reflection.find(EntityKind::Variable, name) {
        Some(e) => {
            let var = node.remote_variable::<Bytes>(name, VariableOptions::new().reflect_from_mesh(true)).map_err(|e| failed(e.to_string()))?;
            let (tx, changes) = mpsc::channel();
            var.on_change(move |u| _ = tx.send(u.data().to_vec())).map_err(|e| failed(e.to_string()))?;
            Source::Variable { var, changes, owner: mesh.provider_of(&e) }
        }
        None => {
            if reflection.find(EntityKind::Topic, name).is_none() {
                ctx.out.note(&format!("no publisher of `{name}` yet, waiting for one"));
            }
            Source::Topic(node.pulled_subscriber::<Bytes>(name, Qos::new().reflect_from_mesh(true)).map_err(|e| failed(e.to_string()))?)
        }
    };
    let mut out = ValueStream::new(ctx, name, args.stream.value_only);
    let more = |out: &ValueStream| args.count.is_none_or(|n| out.seen() < n);
    while !interrupt::interrupted() && more(&out) {
        let mut got = source.next(&mut out, Duration::from_millis(50))?;
        // every waiting value counts
        while got && more(&out) {
            got = source.next(&mut out, Duration::ZERO)?;
        }
        out.tick();
    }
    out.finish("");
    Ok(())
}
