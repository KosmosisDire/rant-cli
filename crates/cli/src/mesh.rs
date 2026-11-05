//! One short lived observer node per command. It opens with details fetched for every peer,
//! so a single bounded settle answers who is there and what they offer.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rant::{Entity, EntityKind, MetaSections, Node, NodeOptions};
use serde::{Deserialize, Serialize};

use crate::failure::{Result, failed};

/// How long a settle waits by default, long enough to hear from every peer on a LAN.
pub const FULL_SETTLE: Duration = Duration::from_millis(600);

/// A live node on the mesh.
#[derive(Debug, Clone)]
pub struct Peer {
    pub id: u32,
    pub name: String,
    /// "ip:port".
    pub address: String,
    /// On this machine: its ip is one of ours.
    pub here: bool,
}

impl Peer {
    /// The ip alone.
    pub fn host(&self) -> String {
        self.address.parse::<SocketAddr>().map_or_else(|_| self.address.clone(), |a| a.ip().to_string())
    }
}

/// Names under @rant/ are Rant's own: the builtin entities and the CLI's nodes.
pub fn is_internal(name: &str) -> bool {
    name.starts_with("@rant/")
}

/// A name like "0x1234abcd" stands in for an entity whose details have not arrived yet.
pub fn is_placeholder(name: &str) -> bool {
    name.len() == 10 && name.starts_with("0x")
}

pub fn kind_name(k: EntityKind) -> &'static str {
    match k {
        EntityKind::Topic => "topic",
        EntityKind::Variable => "var",
        EntityKind::Function => "fn",
        EntityKind::Task => "task",
        _ => "?",
    }
}

pub struct Client {
    node: Node,
    domain: u16,
    home: Option<PathBuf>,
}

impl Client {
    /// home holds the snapshot of names a settle saw, for completion.
    pub fn new(domain: u16, home: Option<&Path>) -> Result<Client> {
        let options = NodeOptions::new().domain(domain).fetch_details(true).disable_logs(true).max_peers(256).max_topics(16);
        let node = Node::new("@rant/cli", options).map_err(|e| failed(format!("cannot join the mesh on domain {domain}: {e}")))?;
        // a peer's schema clash is not the CLI's error
        node.on_event(|_| {});
        Ok(Client { node, domain, home: home.map(Path::to_path_buf) })
    }

    /// Solicits the mesh and waits until every peer that answered is known in detail, or until
    /// limit. Nothing answering costs the whole limit.
    pub fn settle(&self, limit: Duration) {
        let deadline = Instant::now() + limit;
        self.node.settle(Some(limit));
        let r = self.node.reflection();
        while r.mesh().iter().any(|e| is_placeholder(&e.name)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(home) = &self.home {
            // a full settle saw the whole mesh and replaces the snapshot, a shorter one adds to it
            let mut s = if limit >= FULL_SETTLE { Snapshot::default() } else { Snapshot::load(home, self.domain) };
            s.add(self);
            s.save(home, self.domain);
        }
    }

    /// Active, internal ones hidden, by name.
    pub fn peers(&self) -> Vec<Peer> {
        let ours = crate::net::local_addresses();
        let mut out: Vec<Peer> = self
            .node
            .reflection()
            .peers()
            .into_iter()
            .filter(|p| p.active && !is_internal(&p.name))
            .map(|p| {
                let here = p.address.parse::<SocketAddr>().is_ok_and(|a| ours.contains(&a.ip()));
                Peer { id: p.id, name: p.name, address: p.address, here }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The mesh folded, builtins hidden, by name.
    pub fn entities(&self) -> Vec<Entity> {
        public(self.node.reflection().mesh())
    }

    /// Each peer's process id from its @rant/meta answer. A peer that does not answer within
    /// limit is left out.
    pub fn pids(&self, peers: &[u32], limit: Duration) -> BTreeMap<u32, u64> {
        let r = self.node.reflection();
        std::thread::scope(|s| {
            let r = &r;
            let asked: Vec<_> = peers.iter().map(|&p| (p, s.spawn(move || r.meta(p, MetaSections::PROC, limit)))).collect();
            asked.into_iter().filter_map(|(p, h)| Some((p, h.join().ok()?.ok()?.process?.pid))).collect()
        })
    }
}

fn public(mut es: Vec<Entity>) -> Vec<Entity> {
    es.retain(|e| !is_internal(&e.name));
    es.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| kind_name(a.kind).cmp(kind_name(b.kind))));
    es
}

/// The names last seen on a domain, kept in the home's cache so completion can answer at once
/// from what an earlier command saw.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub nodes: BTreeSet<String>,
    /// Name and kind, as kind_name writes it.
    pub entities: BTreeSet<(String, String)>,
}

impl Snapshot {
    fn file(home: &Path, domain: u16) -> PathBuf {
        home.join("cache").join(format!("mesh-{domain}.json"))
    }

    pub fn load(home: &Path, domain: u16) -> Snapshot {
        std::fs::read(Snapshot::file(home, domain)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// A failure only costs a slower completion later.
    pub fn save(&self, home: &Path, domain: u16) {
        let file = Snapshot::file(home, domain);
        let _ = std::fs::create_dir_all(home.join("cache"));
        let _ = crate::files::write_atomic(&file, &serde_json::to_string(self).unwrap_or_default());
    }

    fn add(&mut self, mesh: &Client) {
        self.nodes.extend(mesh.peers().into_iter().map(|p| p.name));
        let named = mesh.entities().into_iter().filter(|e| !is_placeholder(&e.name));
        self.entities.extend(named.map(|e| (e.name, kind_name(e.kind).to_string())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_host_is_its_ip_alone() {
        let peer = |address: &str| Peer { id: 1, name: "a".into(), address: address.into(), here: false };
        assert_eq!(peer("10.0.0.2:6731").host(), "10.0.0.2");
        assert_eq!(peer("[fe80::1]:6731").host(), "fe80::1");
        assert!(is_internal("@rant/cli") && !is_internal("rant/cli"));
        assert!(is_placeholder("0x1234abcd") && !is_placeholder("0x1234abc"));
    }

    #[test]
    fn a_snapshot_round_trips() {
        let home = std::env::temp_dir().join(format!("rant-unit-snapshot-{}", std::process::id()));
        let s = Snapshot { nodes: BTreeSet::from(["talker".into()]), entities: BTreeSet::from([("chatter".into(), "topic".into())]) };
        s.save(&home, 140);
        assert_eq!(Snapshot::load(&home, 140), s);
        assert_eq!(Snapshot::load(&home, 141), Snapshot::default());
        std::fs::remove_dir_all(&home).unwrap();
    }
}
