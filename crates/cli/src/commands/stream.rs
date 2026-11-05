//! A stream of values: the newest redrawn in place on a terminal under a header of who sends it
//! and how fast, else a record a line. --value-only leaves out the sender and the rate.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rant::Schema;
use serde_json::{Value, json};

use crate::context::Context;
use crate::mesh::to_json;
use crate::output::{LiveView, Style, Yaml};

/// The options of a command that prints a stream.
#[derive(clap::Args, Debug)]
pub struct StreamArgs {
    /// print only the values, without who sent them, the rate or the header
    #[arg(long)]
    pub value_only: bool,
}

/// Values a second over the last two seconds, measured across the span the values cover, so it
/// is right from the second value on.
#[derive(Default)]
struct Rate {
    times: VecDeque<Instant>,
}

impl Rate {
    fn hit(&mut self, t: Instant) {
        self.times.push_back(t);
    }

    fn hz(&mut self, now: Instant) -> f64 {
        while self.times.front().is_some_and(|&t| now - t > Duration::from_secs(2)) {
            self.times.pop_front();
        }
        match (self.times.front(), self.times.back()) {
            (Some(a), Some(b)) if self.times.len() > 1 && b > a => (self.times.len() - 1) as f64 / (*b - *a).as_secs_f64(),
            _ => 0.0,
        }
    }
}

struct Latest {
    data: Vec<u8>,
    schema: Option<Schema>,
    from: String,
}

pub struct ValueStream<'a> {
    ctx: &'a Context,
    name: String,
    yaml: Yaml,
    view: Option<LiveView>,
    latest: Option<Latest>,
    note: String,
    rate: Rate,
    value_only: bool,
    seen: u64,
    last_msg: Instant,
    last_draw: Instant,
    dirty: bool,
}

impl<'a> ValueStream<'a> {
    pub fn new(ctx: &'a Context, name: &str, value_only: bool) -> ValueStream<'a> {
        let view = ctx.out.live_out().then(LiveView::stdout);
        let now = Instant::now();
        let yaml = Yaml::new(ctx.out.text_out(None).colored(), true);
        ValueStream { ctx, name: name.to_string(), yaml, view, latest: None, note: String::new(), rate: Rate::default(), value_only, seen: 0, last_msg: now, last_draw: now, dirty: true }
    }

    pub fn seen(&self) -> u64 {
        self.seen
    }

    /// One value as it came. Live, only the newest is decoded, when it is drawn.
    pub fn add(&mut self, from: &str, data: Vec<u8>, schema: Option<Schema>) {
        self.seen += 1;
        self.last_msg = Instant::now();
        self.rate.hit(self.last_msg);
        if self.view.is_none() {
            let value = to_json(&data, schema.as_ref());
            let hz = (self.rate.hz(self.last_msg) * 10.0).round() / 10.0;
            let record = if self.value_only { value } else { json!({"from": from, "hz": hz, "value": value}) };
            self.ctx.out.record(&record, &self.yaml.flow(&record));
            return;
        }
        self.latest = Some(Latest { data, schema, from: from.to_string() });
        self.dirty = true;
        self.tick();
    }

    /// Redraws when due: a new value at 30 frames a second at most, and the rate and age keep
    /// moving between values at 4.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let since = now - self.last_draw;
        if self.view.is_some() && ((self.dirty && since >= Duration::from_millis(33)) || since >= Duration::from_millis(250)) {
            self.draw(now);
        }
    }

    /// A line over the value while the view shows one, such as a task cancelling. On stderr when
    /// nothing is drawn.
    pub fn note(&mut self, text: &str) {
        if self.view.is_none() || self.latest.is_none() {
            return self.ctx.out.note(text);
        }
        self.note = text.to_string();
        self.draw(Instant::now());
    }

    /// Draws a value not yet shown and leaves it on screen. A label, such as "result", heads
    /// what is printed next when the view showed a value.
    pub fn finish(&mut self, label: &str) {
        if self.view.is_none() {
            return;
        }
        if self.dirty && self.latest.is_some() {
            self.draw(Instant::now());
        }
        self.view = None;
        if self.latest.is_none() || label.is_empty() {
            return;
        }
        let out = &self.ctx.out;
        out.line(&if self.value_only { String::new() } else { format!("{}  {}", out.paint_out(Style::Bold, &self.name), out.paint_out(Style::Faint, label)) });
    }

    fn draw(&mut self, now: Instant) {
        let mut lines = Vec::new();
        if !self.value_only {
            lines.push(self.header(now));
        }
        if !self.note.is_empty() {
            lines.push(self.ctx.out.paint_out(Style::Amber, &self.note));
        }
        if let Some(l) = &self.latest {
            let body = self.ctx.out.block(&to_json(&l.data, l.schema.as_ref()), &mut self.yaml);
            lines.extend(body.split('\n').map(str::to_string));
        }
        if let Some(view) = &mut self.view {
            view.draw(&lines);
        }
        self.last_draw = now;
        self.dirty = false;
    }

    /// Who sends it, how fast and how many.
    fn header(&mut self, now: Instant) -> String {
        let status = match &self.latest {
            None => "waiting for a message".to_string(),
            Some(l) => {
                // the header holds still too
                let hz = format!("{:>6.1}", self.rate.hz(now));
                let count = if self.seen == 1 { "message" } else { "messages" };
                let age = (now - self.last_msg).as_secs_f64();
                let late = if age > 1.0 { format!("  last {age:.1} s ago") } else { String::new() };
                format!("from {}  {hz} Hz  {} {count}{late}", l.from, self.seen)
            }
        };
        let out = &self.ctx.out;
        format!("{}  {}", out.paint_out(Style::Bold, &self.name), out.paint_out(Style::Faint, &status))
    }
}

/// One value, such as a call's result: whole on a terminal, one line with --lines or in a pipe.
pub fn print_value(ctx: &Context, v: &Value) {
    let mut yaml = Yaml::new(ctx.out.text_out(None).colored(), false);
    if ctx.out.live_out() {
        ctx.out.line(&ctx.out.block(v, &mut yaml));
    } else {
        ctx.out.record(v, &yaml.flow(v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_counts_over_the_span_its_values_cover() {
        let mut r = Rate::default();
        let t = Instant::now();
        assert_eq!(r.hz(t), 0.0);
        for i in 0..5 {
            r.hit(t + Duration::from_millis(100 * i));
        }
        assert!((r.hz(t + Duration::from_millis(400)) - 10.0).abs() < 1e-9);
        assert_eq!(r.hz(t + Duration::from_secs(10)), 0.0, "old values drop out");
    }
}
