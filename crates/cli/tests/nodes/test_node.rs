//! A node for the end to end tests: test_node [--name N] [--domain D] [--pub TOPIC] [--var NAME]
//! [--fn NAME] [--task NAME] [--life SECONDS] [--exit CODE] [--ignore-stop] [--ignore-cancel]

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rant::{FunctionOptions, Node, NodeOptions, PendingTask, Qos, Request, TaskOptions, TaskRequest, VariableOptions};

#[derive(rant::Message)]
struct Reading {
    seq: u32,
    value: f64,
}

static STOP: AtomicBool = AtomicBool::new(false);
static IGNORE_STOP: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Args {
    name: String,
    domain: u16,
    publish: Option<String>,
    variable: Option<String>,
    function: Option<String>,
    task: Option<String>,
    life: f64,
    exit_code: i32,
    ignore_cancel: bool,
}

fn main() {
    let args = parse(std::env::args().skip(1));
    on_stop_request();
    std::process::exit(run(args));
}

fn parse(mut words: impl Iterator<Item = String>) -> Args {
    // A node rant starts inherits the test's domain, since rant passes its environment on.
    let domain = std::env::var("TEST_NODE_DOMAIN").ok().and_then(|d| d.parse().ok()).unwrap_or(0);
    let mut args = Args { name: "test_node".into(), domain, ..Args::default() };
    while let Some(word) = words.next() {
        let mut value = || words.next().unwrap_or_else(|| usage(&format!("{word} needs a value")));
        match word.as_str() {
            "--name" => args.name = value(),
            "--domain" => args.domain = number(&value()),
            "--pub" => args.publish = Some(value()),
            "--var" => args.variable = Some(value()),
            "--fn" => args.function = Some(value()),
            "--task" => args.task = Some(value()),
            "--life" => args.life = number(&value()),
            "--exit" => args.exit_code = number(&value()),
            "--ignore-stop" => IGNORE_STOP.store(true, Ordering::SeqCst),
            "--ignore-cancel" => args.ignore_cancel = true,
            _ => usage(&format!("unknown argument {word}")),
        }
    }
    args
}

fn number<T: std::str::FromStr>(text: &str) -> T {
    text.parse().unwrap_or_else(|_| usage(&format!("{text} is no number")))
}

fn usage(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn run(args: Args) -> i32 {
    let node = Node::new(&args.name, NodeOptions::new().domain(args.domain).max_topics(16)).expect("the node opens");
    let publisher = args.publish.as_deref().map(|topic| node.publisher::<Reading>(topic, Qos::new()).expect("the topic opens"));
    let _variable = args
        .variable
        .as_deref()
        .map(|name| node.variable_definition::<f64>(name, VariableOptions::new().initial(1.5)).expect("the variable opens"));
    let _function = args.function.as_deref().map(|name| {
        node.function_definition(name, FunctionOptions::new(), |x: Request<f64, f64>| *x * 2.0)
            .expect("the function opens")
    });
    let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
    let _task = args.task.as_deref().map(|name| {
        let workers = workers.clone();
        let ignore_cancel = args.ignore_cancel;
        node.task_definition(name, TaskOptions::new(), move |request: TaskRequest<u32, u32, u32>| {
            let n = *request;
            let pending = request.defer();
            workers.lock().unwrap().push(thread::spawn(move || count(n, pending, ignore_cancel)));
        })
        .expect("the task opens")
    });

    say(&format!("ready {}", args.name));
    let start = Instant::now();
    let mut seq = 0u32;
    let mut lived = false;
    while !STOP.load(Ordering::SeqCst) {
        if args.life > 0.0 && start.elapsed().as_secs_f64() > args.life {
            lived = true;
            break;
        }
        if let Some(publisher) = &publisher {
            let _ = publisher.send(&Reading { seq, value: f64::from(seq) * 0.5 });
            seq += 1;
        }
        thread::sleep(Duration::from_millis(50));
    }
    for worker in workers.lock().unwrap().drain(..) {
        let _ = worker.join();
    }
    say("stopped");
    if lived { args.exit_code } else { 0 }
}

/// Counts to `n`, a step each 100 ms.
fn count(n: u32, pending: PendingTask<u32, u32>, ignore_cancel: bool) {
    for i in 1..=n {
        thread::sleep(Duration::from_millis(100));
        if pending.cancelled() && !ignore_cancel {
            let _ = pending.complete_cancelled("cancelled", Some(&i));
            return;
        }
        let _ = pending.progress(&i);
    }
    let _ = pending.complete(&n);
}

/// A line to stdout, which may be a pipe whose reader is gone.
fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

fn request_stop() {
    if !IGNORE_STOP.load(Ordering::SeqCst) {
        STOP.store(true, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn on_stop_request() {
    unsafe extern "system" fn handler(_: u32) -> i32 {
        request_stop();
        1
    }
    // SAFETY: the handler only touches atomics
    unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handler), 1) };
}

#[cfg(unix)]
fn on_stop_request() {
    extern "C" fn handler(_: libc::c_int) {
        request_stop();
    }
    // SAFETY: the handler only touches atomics
    unsafe {
        libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
    }
}
