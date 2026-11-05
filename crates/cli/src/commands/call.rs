//! `rant call`: a function's reply, or a task run with its progress streamed as sub streams and
//! its result last. Ctrl-C asks the task's provider to cancel and waits for its answer.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context as Poller, Poll, Waker};
use std::time::{Duration, Instant};

use rant::{Bytes, CallOptions, CallStatus, Entity, EntityKind, FunctionOptions, Response, TaskOptions};

use crate::commands::stream::{StreamArgs, ValueStream, print_value};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::interrupt;
use crate::mesh::{Assignments, Client, FULL_SETTLE, encode, parse_words, send_failure, to_json};
use crate::output::Format;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the function or task, then its request: one value, or `field=value` words
    #[arg(value_name = "name", required = true)]
    pub words: Vec<String>,
    /// seconds to wait for an answer, 5 by default
    #[arg(short, long, value_name = "S", default_value_t = 5.0)]
    pub timeout: f64,
    #[command(flatten)]
    pub stream: StreamArgs,
}

/// How long a cancelled task may take to answer before the caller leaves without it.
const CANCEL_WAIT: Duration = Duration::from_secs(10);

pub fn status_text(s: CallStatus) -> &'static str {
    match s {
        CallStatus::Ok => "ok",
        CallStatus::AppError => "failed",
        CallStatus::NoHandler => "the definition has no handler",
        CallStatus::Timeout => "no answer in time",
        CallStatus::PeerLost => "the provider went away",
        CallStatus::Cancelled => "cancelled",
        CallStatus::Running => "it is still running",
        CallStatus::NoProvider => "nobody provides it",
        _ => "it failed",
    }
}

/// The outcome of a call that did not succeed, with the provider's own words when it gave any.
fn fail(name: &str, r: &Response<Bytes>) -> Failure {
    let mut why = status_text(r.status()).to_string();
    if !r.message().is_empty() && r.message() != why {
        why += &format!(": {}", r.message());
    }
    failed(format!("calling `{name}`: {why}"))
}

fn result_of(r: &Response<Bytes>) -> serde_json::Value {
    to_json(r.value().map_or(&[][..], Vec::as_slice), r.schema())
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let [name, request @ ..] = args.words.as_slice() else { unreachable!("clap requires a name") };
    let values = if request.is_empty() { Assignments::new() } else { parse_words(request)? };
    if args.timeout <= 0.0 || !args.timeout.is_finite() {
        return Err(usage("--timeout takes seconds, more than 0"));
    }
    let timeout = Duration::from_secs_f64(args.timeout);
    interrupt::catch();
    let mesh = Client::new(ctx.domain()?, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let e = mesh.require_entity(&[EntityKind::Function, EntityKind::Task], name)?;
    if e.kind == EntityKind::Task {
        return call_task(ctx, &mesh, &e, &values, timeout, args.stream.value_only);
    }
    let f = mesh.node().remote_function::<Bytes, Bytes>(name, FunctionOptions::new().reflect_from_mesh(true)).map_err(|e| failed(e.to_string()))?;
    let req = encode(&values, f.request_schema().as_ref())?;
    let r = f.call(&req, CallOptions::new().timeout(timeout)).map_err(|e| failed(format!("cannot call `{name}`: {}", send_failure(e))))?;
    if !r.is_ok() {
        return Err(fail(name, &r));
    }
    print_value(ctx, &result_of(&r));
    Ok(())
}

/// What the progress handler sends from the thread that answers.
enum Update {
    /// The provider's start acknowledgment, with the call id.
    Started(u32),
    Progress(Vec<u8>),
}

fn call_task(ctx: &Context, mesh: &Client, e: &Entity, values: &Assignments, timeout: Duration, value_only: bool) -> Result<()> {
    let name = &e.name;
    let options = TaskOptions::new().reflect_from_mesh(true).timeout(timeout);
    let task = mesh.node().remote_task::<Bytes, Bytes, Bytes>(name, options).map_err(|e| failed(e.to_string()))?;
    let req = encode(values, task.request_schema().as_ref())?;
    let from = mesh.provider_of(e);
    // CSV lines leave progress out, so the output stays one table: the result's
    let show_progress = ctx.out.live_out() || ctx.out.format() != Format::Csv;
    let (tx, updates) = mpsc::channel();
    let progress = move |p: rant::Progress<'_, Bytes>| {
        let _ = tx.send(match p.value() {
            Some(v) => Update::Progress(v.data().to_vec()),
            None => Update::Started(p.call_id()),
        });
    };
    let mut call = task.call_async(&req, CallOptions::new(), progress).map_err(|e| failed(format!("cannot call `{name}`: {}", send_failure(e))))?;
    let id = call.id();
    let mut out = ValueStream::new(ctx, name, value_only);
    let take = |u: Update, out: &mut ValueStream| match u {
        Update::Started(id) => ctx.out.note(&format!("task {id} running, Ctrl-C cancels it")),
        Update::Progress(data) if show_progress => out.add(&from, data, task.progress_schema()),
        Update::Progress(_) => {}
    };
    let mut poller = Poller::from_waker(Waker::noop());
    let mut cancelled: Option<Instant> = None;
    let r = loop {
        if let Poll::Ready(r) = Pin::new(&mut call).poll(&mut poller) {
            break r;
        }
        if let Ok(u) = updates.recv_timeout(Duration::from_millis(50)) {
            take(u, &mut out);
        }
        out.tick();
        if cancelled.is_none() && interrupt::interrupted() {
            cancelled = Some(Instant::now());
            out.note(&format!("cancelling task {id}, waiting up to {} s for {from} to stop it", CANCEL_WAIT.as_secs()));
            if let Err(s) = call.cancel() {
                out.finish("");
                call.detach();
                return Err(failed(format!("cannot cancel task {id}: {}, it may still run at {from}", send_failure(s))));
            }
        }
        if cancelled.is_some_and(|t| t.elapsed() >= CANCEL_WAIT) {
            out.finish("");
            call.detach();
            return Err(failed(format!("task {id} did not stop within {} s, it may still run at {from}", CANCEL_WAIT.as_secs())));
        }
    };
    // progress that came before the outcome is shown first
    for u in updates.try_iter() {
        take(u, &mut out);
    }
    let partial = r.status() == CallStatus::Cancelled && r.value().is_some_and(|v| !v.is_empty());
    out.finish(if partial { "partial result" } else if r.is_ok() { "result" } else { "" });
    if partial {
        print_value(ctx, &result_of(&r));
    }
    if !r.is_ok() {
        return Err(fail(name, &r));
    }
    print_value(ctx, &result_of(&r));
    Ok(())
}
