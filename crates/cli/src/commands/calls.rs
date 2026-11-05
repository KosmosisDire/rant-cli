//! `rant sub` on a function or task: a record per step of every call any node makes to it, as
//! its definitions report them, never redrawn.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rant::{CallEventKind, CallObserverOptions, CallStatus};
use serde_json::{Map, Value, json};

use crate::commands::call::status_text;
use crate::commands::sub::Args;
use crate::context::Context;
use crate::failure::Result;
use crate::interrupt;
use crate::mesh::{Client, to_json};
use crate::output::{Format, Yaml};

/// The columns of the CSV table, every record's keys in their order.
const COLUMNS: [&str; 10] = ["event", "time", "from", "caller", "call", "status", "by", "message", "took", "value"];

/// One step of a call, copied out of the observer's handler.
struct Step {
    kind: CallEventKind,
    written_us: u64,
    from: String,
    caller: String,
    caller_lo: u32,
    call: u32,
    status: CallStatus,
    message: String,
    value: Option<Value>,
}

pub fn run(ctx: &Context, mesh: &Client, name: &str, args: &Args) -> Result<()> {
    let (tx, steps) = mpsc::channel();
    let options = CallObserverOptions::new().reflect_from_mesh(true);
    let watch = mesh
        .node()
        .call_observer(name, options, move |e| {
            let carries = matches!(e.kind(), CallEventKind::Call | CallEventKind::Progress | CallEventKind::End) && !e.data().is_empty();
            let _ = tx.send(Step {
                kind: e.kind(),
                written_us: e.written_us(),
                from: e.provider_name().to_string(),
                caller: e.caller_name().to_string(),
                caller_lo: e.caller_lo(),
                call: e.call_id(),
                status: e.status(),
                message: e.message().to_string(),
                value: carries.then(|| to_json(e.data(), e.schema().as_ref())),
            });
        })?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while watch.match_count() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if watch.match_count() == 0 {
        ctx.out.note(&format!("no provider of `{name}` yet, waiting for one"));
    }
    let mut yaml = Yaml::new(ctx.out.text_out(None).colored(), true);
    // when each call this sub saw began, by its caller and number
    let mut began: HashMap<(u32, u32), u64> = HashMap::new();
    let mut ended = 0;
    while !interrupt::interrupted() && args.count.is_none_or(|n| ended < n) {
        let Ok(s) = steps.recv_timeout(Duration::from_millis(50)) else { continue };
        let key = (s.caller_lo, s.call);
        let mut took = None;
        match s.kind {
            CallEventKind::Call => _ = began.insert(key, s.written_us),
            CallEventKind::End => {
                if let Some(start) = began.remove(&key) {
                    ended += 1;
                    took = (start != 0 && s.written_us >= start).then(|| ((s.written_us - start) as f64 / 1000.0).round() / 1000.0);
                }
            }
            _ => {}
        }
        if args.stream.value_only {
            if let Some(v) = &s.value {
                ctx.out.record(v, &yaml.flow(v));
            }
            continue;
        }
        let r = Value::Object(record(&s, took));
        if ctx.out.format() == Format::Csv {
            ctx.out.record(&csv_row(r), "");
        } else {
            ctx.out.record(&r, &yaml.flow(&r));
        }
    }
    Ok(())
}

/// A step as a record, each key left out when it has nothing to say.
fn record(s: &Step, took: Option<f64>) -> Map<String, Value> {
    let mut r = Map::new();
    let event = match s.kind {
        CallEventKind::Call => "call",
        CallEventKind::Running => "running",
        CallEventKind::Progress => "progress",
        CallEventKind::Cancel => "cancel",
        CallEventKind::End => "end",
    };
    r.insert("event".into(), json!(event));
    if s.written_us != 0 {
        r.insert("time".into(), json!(iso_ms(s.written_us)));
    }
    if !s.from.is_empty() {
        r.insert("from".into(), json!(s.from));
    }
    if !s.caller.is_empty() {
        r.insert("caller".into(), json!(s.caller));
    }
    r.insert("call".into(), json!(s.call));
    let status = status_text(s.status);
    if s.kind == CallEventKind::End {
        r.insert("status".into(), json!(status));
    }
    if s.kind == CallEventKind::Cancel && !s.message.is_empty() {
        r.insert("by".into(), json!(s.message));
    }
    if s.kind == CallEventKind::End && !s.message.is_empty() && s.message != status {
        r.insert("message".into(), json!(s.message));
    }
    if let Some(t) = took {
        r.insert("took".into(), json!(t));
    }
    if let Some(v) = &s.value {
        r.insert("value".into(), v.clone());
    }
    r
}

/// A record with every column, its value one cell of compact JSON.
fn csv_row(r: Value) -> Value {
    let row = COLUMNS.iter().map(|&c| {
        let cell = match (c, r.get(c)) {
            ("value", Some(v)) => Value::String(v.to_string()),
            (_, v) => v.cloned().unwrap_or(Value::Null),
        };
        (c.to_string(), cell)
    });
    Value::Object(row.collect())
}

/// Microseconds since 1970 as ISO 8601 UTC to the millisecond, such as 2026-11-04T09:30:00.123Z.
fn iso_ms(us: u64) -> String {
    let (secs, ms) = (us / 1_000_000, us / 1000 % 1000);
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // the civil date of a day count, after Howard Hinnant's days_to_civil
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(kind: CallEventKind, status: CallStatus, message: &str, value: Option<Value>) -> Step {
        Step { kind, written_us: 1_700_000_000_123_456, from: "arm".into(), caller: "panel".into(), caller_lo: 9, call: 3, status, message: message.into(), value }
    }

    #[test]
    fn times_read_as_utc_dates() {
        assert_eq!(iso_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_ms(1_700_000_000_123_456), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso_ms(951_782_400_000_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn a_record_says_only_what_its_step_has() {
        let call = record(&step(CallEventKind::Call, CallStatus::Ok, "", Some(json!(21.0))), None);
        assert_eq!(Value::Object(call), json!({"event": "call", "time": "2023-11-14T22:13:20.123Z", "from": "arm", "caller": "panel", "call": 3, "value": 21.0}));
        let end = record(&step(CallEventKind::End, CallStatus::AppError, "no arm", None), Some(0.25));
        assert_eq!(Value::Object(end), json!({"event": "end", "time": "2023-11-14T22:13:20.123Z", "from": "arm", "caller": "panel", "call": 3, "status": "failed", "message": "no arm", "took": 0.25}));
        let cancel = record(&step(CallEventKind::Cancel, CallStatus::Ok, "panel", None), None);
        assert_eq!(cancel.get("by"), Some(&json!("panel")));
        let cancelled = record(&step(CallEventKind::End, CallStatus::Cancelled, "cancelled", None), None);
        assert_eq!(cancelled.get("message"), None, "a message that only repeats the status");
    }

    #[test]
    fn a_csv_row_holds_every_column() {
        let r = Value::Object(record(&step(CallEventKind::Call, CallStatus::Ok, "", Some(json!({"x": 1}))), None));
        let row = csv_row(r);
        assert_eq!(row.as_object().unwrap().keys().collect::<Vec<_>>(), COLUMNS.iter().collect::<Vec<_>>());
        assert_eq!(row["value"], json!(r#"{"x":1}"#));
        assert_eq!(row["status"], Value::Null);
    }
}
