//! A recording: an MCAP file of Rant's own bytes, which `rant record` writes and `rant play`,
//! `rant info` and `rant ls` read. One track per name, sender and schema.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcap::records::{MessageHeader, Metadata, Record};
use mcap::sans_io::{LinearReadEvent, LinearReader, LinearReaderOptions};
use rant::{CallEventKind, CallStatus, EntityKind, Schema};

use crate::context::Context;
use crate::failure::{Result, failed};
use crate::mesh::kind_name;

pub const EXTENSION: &str = "mcap";
const PROFILE: &str = "rant";
/// The metadata record that says what a recording is, and the layout this build writes and alone
/// reads, since recordings carry no promise to stay readable.
const ABOUT: &str = "rant";
const LAYOUT: &str = "1";
/// Schemas are their wire bytes. A value is its capture time, a flags byte and its payload.
const ENCODING: &str = "rant";
/// A step of a call, then its payload.
const CALL_ENCODING: &str = "rant.call";
const FORCED: u8 = 1;

/// Stored by position, so the order never changes within a layout.
const EVENTS: [CallEventKind; 5] = [CallEventKind::Call, CallEventKind::Running, CallEventKind::Progress, CallEventKind::Cancel, CallEventKind::End];
const STATUSES: [CallStatus; 9] = [
    CallStatus::Ok,
    CallStatus::AppError,
    CallStatus::NoHandler,
    CallStatus::Timeout,
    CallStatus::PeerLost,
    CallStatus::Cancelled,
    CallStatus::Running,
    CallStatus::NoProvider,
    CallStatus::Unknown,
];

/// One message as the recorder got it.
pub struct Sample {
    pub kind: EntityKind,
    pub name: String,
    /// The node that sent it.
    pub from: String,
    pub reliable: bool,
    pub schema: Option<Schema>,
    /// When the recorder got it, UTC microseconds.
    pub log_us: u64,
    /// When the sender wrote it, 0 when it opted out.
    pub written_us: u64,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A topic's message or a variable's value.
    Value { capture_us: u64, forced: bool, data: Vec<u8> },
    Call(Step),
}

/// One step of a call to a function or task, as its definition reported it.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub event: CallEventKind,
    pub status: CallStatus,
    pub call: u32,
    pub caller_lo: u32,
    pub caller: String,
    pub message: String,
    pub data: Vec<u8>,
}

impl Body {
    fn encoding(&self) -> &'static str {
        match self {
            Body::Value { .. } => ENCODING,
            Body::Call(_) => CALL_ENCODING,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        match self {
            Body::Value { capture_us, forced, data } => {
                b.extend(capture_us.to_le_bytes());
                b.push(if *forced { FORCED } else { 0 });
                b.extend(data);
            }
            Body::Call(s) => {
                b.push(EVENTS.iter().position(|e| *e == s.event).unwrap_or(0) as u8);
                b.push(STATUSES.iter().position(|e| *e == s.status).unwrap_or(STATUSES.len() - 1) as u8);
                b.extend(s.call.to_le_bytes());
                b.extend(s.caller_lo.to_le_bytes());
                for text in [&s.caller, &s.message] {
                    let text = &text.as_bytes()[..text.len().min(u16::MAX as usize)];
                    b.extend((text.len() as u16).to_le_bytes());
                    b.extend(text);
                }
                b.extend(&s.data);
            }
        }
        b
    }

    fn decode(encoding: &str, b: &[u8]) -> Option<Body> {
        let mut r = Bytes(b);
        match encoding {
            ENCODING => {
                let capture_us = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
                let forced = r.take(1)?[0] & FORCED != 0;
                Some(Body::Value { capture_us, forced, data: r.0.to_vec() })
            }
            CALL_ENCODING => {
                let event = *EVENTS.get(usize::from(r.take(1)?[0]))?;
                let status = STATUSES.get(usize::from(r.take(1)?[0])).copied().unwrap_or(CallStatus::Unknown);
                let call = u32::from_le_bytes(r.take(4)?.try_into().ok()?);
                let caller_lo = u32::from_le_bytes(r.take(4)?.try_into().ok()?);
                let mut text = || {
                    let n = u16::from_le_bytes(r.take(2)?.try_into().ok()?);
                    Some(String::from_utf8_lossy(r.take(usize::from(n))?).into_owned())
                };
                let (caller, message) = (text()?, text()?);
                Some(Body::Call(Step { event, status, call, caller_lo, caller, message, data: r.0.to_vec() }))
            }
            _ => None,
        }
    }
}

/// Bytes read from the front.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
}

/// A recording being written. Tracks and schemas are added as their first message comes.
pub struct Writer {
    mcap: mcap::Writer<BufWriter<File>>,
    schemas: HashMap<u64, u16>,
    /// By kind, name, sender and schema: the channel and its next sequence number.
    channels: HashMap<(EntityKind, String, String, u16), (u16, u32)>,
    pub messages: u64,
    pub bytes: u64,
}

impl Writer {
    /// A new file, never one that is there. about goes in the recording's own metadata.
    pub fn create(path: &Path, about: BTreeMap<String, String>) -> Result<Writer> {
        let file = File::create_new(path).map_err(|e| failed(format!("cannot create {}: {e}", rant_config::paths::slash(path))))?;
        let options = mcap::WriteOptions::new()
            .profile(PROFILE)
            .library(format!("rant-cli {}", env!("CARGO_PKG_VERSION")))
            .compression(Some(mcap::Compression::Zstd));
        let mut mcap = options.create(BufWriter::new(file)).map_err(bad_write)?;
        let mut metadata = about;
        metadata.insert("layout".into(), LAYOUT.into());
        mcap.write_metadata(&Metadata { name: ABOUT.into(), metadata }).map_err(bad_write)?;
        Ok(Writer { mcap, schemas: HashMap::new(), channels: HashMap::new(), messages: 0, bytes: 0 })
    }

    pub fn add(&mut self, s: Sample) -> Result<()> {
        let schema_id = match &s.schema {
            None => 0,
            Some(schema) => match self.schemas.get(&schema.hash()) {
                Some(&id) => id,
                None => {
                    let id = self.mcap.add_schema(&schema.name(), ENCODING, &schema.wire()).map_err(bad_write)?;
                    self.schemas.insert(schema.hash(), id);
                    id
                }
            },
        };
        let key = (s.kind, s.name, s.from, schema_id);
        if !self.channels.contains_key(&key) {
            let metadata = BTreeMap::from([
                ("kind".to_string(), kind_name(key.0).to_string()),
                ("from".to_string(), key.2.clone()),
                ("reliable".to_string(), s.reliable.to_string()),
            ]);
            let id = self.mcap.add_channel(schema_id, &key.1, s.body.encoding(), &metadata).map_err(bad_write)?;
            self.channels.insert(key.clone(), (id, 0));
        }
        let (channel_id, sequence) = self.channels.get_mut(&key).expect("added above");
        let header = MessageHeader { channel_id: *channel_id, sequence: *sequence, log_time: s.log_us * 1000, publish_time: s.written_us * 1000 };
        *sequence += 1;
        let data = s.body.encode();
        self.mcap.write_to_known_channel(&header, &data).map_err(bad_write)?;
        self.messages += 1;
        self.bytes += data.len() as u64;
        Ok(())
    }

    /// Writes the index and summary that end the file.
    pub fn finish(mut self) -> Result<()> {
        self.mcap.finish().map_err(bad_write)?;
        Ok(())
    }
}

fn bad_write(e: mcap::McapError) -> crate::failure::Failure {
    failed(format!("cannot write the recording: {e}"))
}

/// One name, sender and schema in a recording.
#[derive(Debug)]
pub struct Track {
    pub id: u16,
    pub name: String,
    pub kind: EntityKind,
    pub from: String,
    pub reliable: bool,
    /// The schema's type name, empty for an anonymous or untyped one.
    pub type_name: String,
    /// The schema's wire bytes, None for an untyped track.
    pub schema: Option<Vec<u8>>,
}

impl Track {
    fn of(c: &mcap::Channel) -> Track {
        let meta = |k: &str| c.metadata.get(k).cloned().unwrap_or_default();
        let kind = [EntityKind::Topic, EntityKind::Variable, EntityKind::Function, EntityKind::Task].into_iter().find(|&k| kind_name(k) == meta("kind"));
        Track {
            id: c.id,
            name: c.topic.clone(),
            kind: kind.unwrap_or(EntityKind::Unknown),
            from: meta("from"),
            reliable: meta("reliable") == "true",
            type_name: c.schema.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            schema: c.schema.as_ref().map(|s| s.data.to_vec()),
        }
    }
}

/// One message read back.
pub struct Entry {
    pub track: Arc<Track>,
    pub log_us: u64,
    pub body: Body,
}

/// A recording read in the order it was written.
pub struct Reader {
    pub about: BTreeMap<String, String>,
    path: PathBuf,
    messages: mcap::io::MessageReader<BufReader<File>>,
    tracks: HashMap<u16, Arc<Track>>,
}

impl Reader {
    /// Refuses a file that is no recording, or one of another layout.
    pub fn open(path: &Path) -> Result<Reader> {
        let about = about(path)?;
        let file = File::open(path).map_err(|e| failed(format!("cannot read {}: {e}", shown(path))))?;
        Ok(Reader { about, path: path.to_path_buf(), messages: mcap::io::MessageReader::new(BufReader::new(file)), tracks: HashMap::new() })
    }
}

impl Iterator for Reader {
    type Item = Result<Entry>;

    fn next(&mut self) -> Option<Result<Entry>> {
        let m = match self.messages.next()? {
            Ok(m) => m,
            Err(e) => return Some(Err(failed(format!("{} ends early: {e}", shown(&self.path))))),
        };
        let track = Arc::clone(self.tracks.entry(m.channel.id).or_insert_with(|| Arc::new(Track::of(&m.channel))));
        let Some(body) = Body::decode(&m.channel.message_encoding, &m.data) else {
            return Some(Err(failed(format!("{} holds a message of `{}` it cannot read", shown(&self.path), track.name))));
        };
        Some(Ok(Entry { track, log_us: m.log_time / 1000, body }))
    }
}

/// The recording's own metadata, read up to the first message.
fn about(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut file = File::open(path).map_err(|e| failed(format!("cannot read {}: {e}", shown(path))))?;
    let not_ours = || failed(format!("{} is not a Rant recording", shown(path)));
    let mut reader = LinearReader::new_with_options(LinearReaderOptions::default().with_emit_chunks(true));
    let mut profile = String::new();
    while let Some(event) = reader.next_event() {
        let record = match event.map_err(|_| not_ours())? {
            LinearReadEvent::ReadRequest(n) => {
                let read = file.read(reader.insert(n)).map_err(|e| failed(format!("cannot read {}: {e}", shown(path))))?;
                reader.notify_read(read);
                continue;
            }
            LinearReadEvent::Record { data, opcode } => mcap::parse_record(opcode, data).map_err(|_| not_ours())?,
        };
        match record {
            Record::Header(h) => profile = h.profile,
            Record::Metadata(m) if m.name == ABOUT && profile == PROFILE => {
                let layout = m.metadata.get("layout").cloned().unwrap_or_default();
                if layout != LAYOUT {
                    return Err(failed(format!("{} is a recording of layout {layout}, and this rant-cli reads only layout {LAYOUT}", shown(path))));
                }
                return Ok(m.metadata);
            }
            Record::Chunk { .. } | Record::Message { .. } | Record::DataEnd(_) => break,
            _ => {}
        }
    }
    Err(not_ours())
}

/// Each track with how many messages it holds and when they span, and the whole recording's span.
pub struct Contents {
    pub about: BTreeMap<String, String>,
    pub tracks: Vec<(Arc<Track>, u64)>,
    pub first_us: u64,
    pub last_us: u64,
    /// Why the file ends before its summary, as a recorder that died leaves it.
    pub cut: Option<String>,
}

/// Reads a whole recording once.
pub fn contents(path: &Path) -> Result<Contents> {
    let mut reader = Reader::open(path)?;
    let mut counts: BTreeMap<u16, (Arc<Track>, u64)> = BTreeMap::new();
    let (mut first_us, mut last_us, mut cut) = (0, 0, None);
    for entry in reader.by_ref() {
        let e = match entry {
            Ok(e) => e,
            Err(f) => {
                cut = f.messages().into_iter().next();
                break;
            }
        };
        if first_us == 0 {
            first_us = e.log_us;
        }
        last_us = last_us.max(e.log_us);
        counts.entry(e.track.id).or_insert_with(|| (Arc::clone(&e.track), 0)).1 += 1;
    }
    let mut tracks: Vec<(Arc<Track>, u64)> = counts.into_values().collect();
    tracks.sort_by(|a, b| a.0.name.cmp(&b.0.name).then_with(|| a.0.from.cmp(&b.0.from)));
    Ok(Contents { about: reader.about, tracks, first_us, last_us, cut })
}

/// The wall clock in UTC microseconds, the clock Rant stamps with.
pub fn now_us() -> u64 {
    rant::types::Timestamp::now().us.max(0) as u64
}

/// Where recordings go: the workspace's recordings folder, else the folder rant runs in.
pub fn folder(ctx: &mut Context) -> Result<PathBuf> {
    match ctx.workspace() {
        Some(s) => Ok(s.recordings.clone()),
        None => Ok(ctx.cwd()?.to_path_buf()),
    }
}

/// The recordings in the recordings folder, by file name.
pub fn listed(ctx: &mut Context) -> Result<Vec<PathBuf>> {
    let files = std::fs::read_dir(folder(ctx)?).into_iter().flatten().flatten().map(|e| e.path());
    let mut out: Vec<PathBuf> = files.filter(|p| p.is_file() && p.extension().is_some_and(|e| e == EXTENSION)).collect();
    out.sort();
    Ok(out)
}

/// The name a recording goes by: its file name without `.mcap`.
pub fn name_of(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A recording by its path, or by its name in the recordings folder, with or without `.mcap`.
pub fn locate(ctx: &mut Context, name: &Path) -> Result<PathBuf> {
    let with_ext = |p: PathBuf| if p.extension().is_none() { p.with_extension(EXTENSION) } else { p };
    let tries = [ctx.cwd()?.join(name), with_ext(ctx.cwd()?.join(name)), with_ext(folder(ctx)?.join(name))];
    tries.into_iter().find(|p| p.is_file()).ok_or_else(|| failed(format!("no recording `{}`, see `rant ls recordings`", shown(name))))
}

fn shown(path: &Path) -> String {
    rant_config::paths::slash(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    fn value(data: &[u8]) -> Body {
        Body::Value { capture_us: 7, forced: true, data: data.to_vec() }
    }

    #[test]
    fn a_body_reads_back_as_written() {
        let step = Step { event: CallEventKind::End, status: CallStatus::AppError, call: 9, caller_lo: 3, caller: "panel".into(), message: "no arm".into(), data: vec![1, 2] };
        for body in [value(b"abc"), value(b""), Body::Call(step)] {
            assert_eq!(Body::decode(body.encoding(), &body.encode()), Some(body));
        }
        assert_eq!(Body::decode(ENCODING, &[1, 2]), None, "too short for its capture time");
        assert_eq!(Body::decode("json", b"{}"), None);
    }

    #[test]
    fn a_recording_reads_back_in_order_with_its_tracks() {
        let t = Scratch::new("recording");
        let path = t.0.join("a.mcap");
        let node = crate::mesh::test_node();
        let schema = node.schema("Pose2 { x: f64, y: f64 }").unwrap();
        let mut w = Writer::create(&path, BTreeMap::from([("domain".to_string(), "5".to_string())])).unwrap();
        for (i, from) in ["arm", "arm", "cam"].into_iter().enumerate() {
            let s = Sample { kind: EntityKind::Topic, name: "pose".into(), from: from.into(), reliable: true, schema: Some(schema.clone()), log_us: 1_000 + i as u64, written_us: 900, body: value(&[i as u8]) };
            w.add(s).unwrap();
        }
        w.add(Sample { kind: EntityKind::Variable, name: "mode".into(), from: "arm".into(), reliable: true, schema: None, log_us: 2_000, written_us: 0, body: value(b"x") }).unwrap();
        assert!(Writer::create(&path, BTreeMap::new()).is_err(), "never over a file that is there");
        w.finish().unwrap();

        let entries: Vec<Entry> = Reader::open(&path).unwrap().map(|e| e.unwrap()).collect();
        let seen: Vec<(&str, &str, u64)> = entries.iter().map(|e| (e.track.name.as_str(), e.track.from.as_str(), e.log_us)).collect();
        assert_eq!(seen, [("pose", "arm", 1_000), ("pose", "arm", 1_001), ("pose", "cam", 1_002), ("mode", "arm", 2_000)]);
        assert_eq!(entries[0].body, value(&[0]));
        let pose = &entries[0].track;
        assert_eq!((pose.kind, pose.reliable, pose.type_name.as_str()), (EntityKind::Topic, true, "Pose2"));
        assert_eq!(node.schema_from_wire(pose.schema.as_deref().unwrap()).unwrap().hash(), schema.hash());
        assert!(entries[3].track.schema.is_none() && entries[3].track.kind == EntityKind::Variable);

        let c = contents(&path).unwrap();
        assert_eq!((c.first_us, c.last_us, c.cut.as_deref()), (1_000, 2_000, None));
        let counted: Vec<(&str, &str, u64)> = c.tracks.iter().map(|(t, n)| (t.name.as_str(), t.from.as_str(), *n)).collect();
        assert_eq!(counted, [("mode", "arm", 1), ("pose", "arm", 2), ("pose", "cam", 1)]);
        assert_eq!(c.about.get("domain").map(String::as_str), Some("5"));
    }

    #[test]
    fn only_a_recording_of_this_layout_opens() {
        let t = Scratch::new("recording-layout");
        let other = t.0.join("other.mcap");
        let mut w = mcap::WriteOptions::new().profile("ros2").create(BufWriter::new(File::create(&other).unwrap())).unwrap();
        w.finish().unwrap();
        drop(w);
        assert!(Reader::open(&other).err().unwrap().messages()[0].ends_with("is not a Rant recording"));

        let old = t.0.join("old.mcap");
        let mut w = mcap::WriteOptions::new().profile(PROFILE).create(BufWriter::new(File::create(&old).unwrap())).unwrap();
        w.write_metadata(&Metadata { name: ABOUT.into(), metadata: BTreeMap::from([("layout".to_string(), "0".to_string())]) }).unwrap();
        w.finish().unwrap();
        drop(w);
        assert!(Reader::open(&old).err().unwrap().messages()[0].contains("of layout 0"));
        assert!(Reader::open(&t.0.join("missing.mcap")).is_err());
    }
}
