//! The kinds of thing ls lists and info explains. Topic, Variable, Function and Task narrow
//! Entity. Library, Source and Service only ls lists.

use rant::EntityKind;

use crate::mesh::kind_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Node,
    Entity,
    Topic,
    Variable,
    Function,
    Task,
    Type,
    Package,
    Library,
    Group,
    Source,
    Service,
    Recording,
}

struct Words {
    kind: Kind,
    singular: &'static str,
    plural: &'static str,
    /// Other words a user may type.
    more: &'static [&'static str],
    /// What ls lists for it.
    listed: &'static str,
}

const TABLE: &[Words] = &[
    Words { kind: Kind::Node, singular: "node", plural: "nodes", more: &[], listed: "running nodes, and in a workspace the ones that could start" },
    Words { kind: Kind::Entity, singular: "entity", plural: "entities", more: &[], listed: "every topic, variable, function and task on the mesh" },
    Words { kind: Kind::Topic, singular: "topic", plural: "topics", more: &[], listed: "topics on the mesh" },
    Words { kind: Kind::Variable, singular: "var", plural: "vars", more: &["variable", "variables"], listed: "variables on the mesh" },
    Words { kind: Kind::Function, singular: "fn", plural: "fns", more: &["function", "functions"], listed: "functions on the mesh" },
    Words { kind: Kind::Task, singular: "task", plural: "tasks", more: &[], listed: "tasks on the mesh" },
    Words { kind: Kind::Type, singular: "type", plural: "types", more: &[], listed: "message types, the standard ones and those the mesh uses" },
    Words { kind: Kind::Package, singular: "package", plural: "packages", more: &["pkg", "pkgs"], listed: "the packages of this workspace" },
    Words { kind: Kind::Library, singular: "library", plural: "libraries", more: &["lib", "libs"], listed: "the libraries each package here uses, such as Rant" },
    Words { kind: Kind::Group, singular: "group", plural: "groups", more: &[], listed: "the groups of this workspace" },
    Words { kind: Kind::Source, singular: "source", plural: "sources", more: &[], listed: "where rant install finds packages, in the order it looks" },
    Words { kind: Kind::Service, singular: "service", plural: "services", more: &[], listed: "the services of each enabled workspace, and whether they run" },
    Words { kind: Kind::Recording, singular: "recording", plural: "recordings", more: &[], listed: "the recordings in the workspace's recordings folder" },
];

impl Kind {
    /// The kind a word names, singular or plural, short or long: node, nodes, pkg, packages.
    pub fn named(word: &str) -> Option<Kind> {
        TABLE.iter().find(|w| w.singular == word || w.plural == word || w.more.contains(&word)).map(|w| w.kind)
    }

    /// The word for a kind in messages and in info: "node", "package".
    pub fn word(self) -> &'static str {
        TABLE.iter().find(|w| w.kind == self).map_or("?", |w| w.singular)
    }

    /// Whether info explains things of this kind.
    pub fn explained(self) -> bool {
        !matches!(self, Kind::Library | Kind::Source | Kind::Service)
    }

    pub fn on_mesh(self) -> bool {
        !matches!(self, Kind::Package | Kind::Library | Kind::Group | Kind::Source | Kind::Service | Kind::Recording)
    }

    /// An entity matches Entity, or the kind that names its own.
    pub fn takes(self, e: EntityKind) -> bool {
        self == Kind::Entity || (self.on_mesh() && self != Kind::Node && kind_name(e) == self.word())
    }
}

/// The words ls takes, each kind's plural, or with plural false those info explains, singular.
pub fn words(plural: bool) -> Vec<String> {
    let taken = TABLE.iter().filter(|w| plural || w.kind.explained());
    taken.map(|w| (if plural { w.plural } else { w.singular }).to_string()).collect()
}

/// Each kind's plural with what ls lists for it, for its help.
pub fn help() -> String {
    let lines: Vec<String> = TABLE.iter().map(|w| format!("  {:22} {}", w.plural, w.listed)).collect();
    format!("Kinds:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_names_its_kind() {
        for (word, kind) in [("pkgs", Kind::Package), ("variable", Kind::Variable), ("fn", Kind::Function), ("nodes", Kind::Node), ("lib", Kind::Library)] {
            assert_eq!(Kind::named(word), Some(kind), "{word}");
        }
        assert_eq!(Kind::named("thing"), None);
        assert!(Kind::Variable.takes(EntityKind::Variable) && !Kind::Variable.takes(EntityKind::Topic) && Kind::Entity.takes(EntityKind::Task));
        assert!(!Kind::Node.takes(EntityKind::Topic) && !Kind::Group.takes(EntityKind::Topic));
        assert!(help().starts_with("Kinds:\n  nodes                  running nodes"));
    }
}
