// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo xtask check-deps`: enforces the crate dependency rules
//! (see AGENTS.md, "Architecture rules").
//!
//! Rules, evaluated on `cargo metadata` output:
//! 1. every workspace member is declared in [`ALLOWED_EDGES`], and each of its
//!    direct workspace dependencies is listed there;
//! 2. the [`FORBIDDEN`] edges do not exist in the TRANSITIVE closure;
//! 3. the transitive closure of every connector crate only contains connector
//!    crates and third-party crates listed in `crates/connectors/allowed-deps.toml`.
//!
//! The closure of a crate includes its own dev-dependencies, then follows only
//! normal and build dependencies (dev-dependencies of dependencies are never
//! compiled into it).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context as _;
use cargo_metadata::{CargoOpt, DependencyKind, MetadataCommand};
use serde::Deserialize;

const CONNECTOR_PREFIX: &str = "em-connector-";

/// Direct workspace dependencies each workspace crate may declare.
const ALLOWED_EDGES: &[(&str, &[&str])] = &[
    ("em-core-domain", &[]),
    ("em-storage", &["em-core-domain"]),
    ("em-categorization", &["em-core-domain"]),
    ("em-analytics", &["em-core-domain"]),
    ("em-pricing", &["em-core-domain"]),
    ("em-secrets", &[]),
    (
        "em-services",
        &[
            "em-core-domain",
            "em-storage",
            "em-categorization",
            "em-analytics",
            "em-pricing",
        ],
    ),
    ("em-sync-facade", &["em-services", "em-core-domain"]),
    (
        "em-api",
        &["em-sync-facade", "em-services", "em-core-domain"],
    ),
    (
        "em-mcp",
        &["em-sync-facade", "em-services", "em-core-domain"],
    ),
    (
        "em-sync",
        &[
            "em-sync-facade",
            "em-services",
            "em-secrets",
            "em-connector-registry",
            "em-connector-sdk",
            "em-pricing",
            "em-core-domain",
        ],
    ),
    ("em-connector-sdk", &[]),
    (
        "em-connector-registry",
        &[
            "em-connector-sdk",
            "em-connector-fake-bank",
            "em-connector-enable-banking",
            "em-connector-trade-republic",
            "em-connector-banque-populaire",
            "em-connector-banque-postale",
        ],
    ),
    ("em-connector-fake-bank", &["em-connector-sdk"]),
    ("em-connector-enable-banking", &["em-connector-sdk"]),
    ("em-connector-trade-republic", &["em-connector-sdk"]),
    ("em-connector-banque-populaire", &["em-connector-sdk"]),
    ("em-connector-banque-postale", &["em-connector-sdk"]),
    ("em-server", &["em-api", "em-mcp", "em-sync"]),
    ("xtask", &[]),
];

/// `(from, to)`: `from` must never reach `to`, even transitively.
/// `"<connectors>"` stands for every `em-connector-*` crate.
const FORBIDDEN: &[(&[&str], &[&str])] = &[
    (
        &[
            "em-api",
            "em-mcp",
            "em-services",
            "em-pricing",
            "em-analytics",
        ],
        &["em-secrets"],
    ),
    (&["em-api", "em-mcp"], &["em-sync"]),
    (&["<connectors>"], &["em-core-domain", "em-storage"]),
];

/// A dependency graph keyed by package name.
#[derive(Debug, Default, Clone)]
pub struct Graph {
    /// Workspace member names.
    pub members: BTreeSet<String>,
    /// `name -> [(dependency name, is_dev_only)]`.
    pub deps: BTreeMap<String, Vec<(String, bool)>>,
}

impl Graph {
    fn add_edge(&mut self, from: &str, to: &str, dev_only: bool) {
        self.deps
            .entry(from.to_owned())
            .or_default()
            .push((to.to_owned(), dev_only));
    }

    /// Transitive closure of `root` (root's dev-dependencies included).
    fn closure(&self, root: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<String> = self
            .deps
            .get(root)
            .into_iter()
            .flatten()
            .map(|(d, _)| d.clone())
            .collect();
        while let Some(name) = stack.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            for (dep, dev_only) in self.deps.get(&name).into_iter().flatten() {
                if !dev_only && !seen.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
        seen
    }
}

fn is_connector(name: &str) -> bool {
    name.starts_with(CONNECTOR_PREFIX)
}

fn expand(group: &[&str], members: &BTreeSet<String>) -> Vec<String> {
    group
        .iter()
        .flat_map(|g| {
            if *g == "<connectors>" {
                members
                    .iter()
                    .filter(|m| is_connector(m))
                    .cloned()
                    .collect()
            } else {
                vec![(*g).to_owned()]
            }
        })
        .collect()
}

/// Returns every rule violation found in `graph` (empty = OK).
pub fn check(graph: &Graph, connector_allowed_external: &BTreeSet<String>) -> Vec<String> {
    let mut violations = Vec::new();
    let allowed: BTreeMap<&str, &[&str]> = ALLOWED_EDGES.iter().copied().collect();

    // Rule 1: declared crates, allowed direct workspace edges.
    for member in &graph.members {
        let Some(allowed_deps) = allowed.get(member.as_str()) else {
            violations.push(format!(
                "{member}: not declared in xtask check-deps ALLOWED_EDGES (add it to the table and to AGENTS.md)"
            ));
            continue;
        };
        for (dep, _) in graph.deps.get(member).into_iter().flatten() {
            if graph.members.contains(dep) && !allowed_deps.contains(&dep.as_str()) {
                violations.push(format!(
                    "{member} -> {dep}: direct workspace edge not allowed (see AGENTS.md, Architecture rules)"
                ));
            }
        }
    }

    // Rule 2: forbidden transitive edges.
    for (from_group, to_group) in FORBIDDEN {
        for from in expand(from_group, &graph.members) {
            if !graph.members.contains(&from) {
                continue;
            }
            let closure = graph.closure(&from);
            for to in expand(to_group, &graph.members) {
                if closure.contains(&to) {
                    violations.push(format!(
                        "{from} ->* {to}: forbidden transitive dependency (see AGENTS.md, Architecture rules)"
                    ));
                }
            }
        }
    }

    // Rule 3: connector closure limited to connectors + allowed-deps.toml.
    for connector in graph.members.iter().filter(|m| is_connector(m)) {
        for dep in graph.closure(connector) {
            let ok = if graph.members.contains(&dep) {
                is_connector(&dep)
            } else {
                connector_allowed_external.contains(&dep)
            };
            if !ok {
                violations.push(format!(
                    "{connector} ->* {dep}: not allowed for connectors (see crates/connectors/allowed-deps.toml)"
                ));
            }
        }
    }

    violations
}

/// Builds the graph of the current workspace from `cargo metadata`.
pub fn workspace_graph() -> anyhow::Result<Graph> {
    graph_for(None)
}

/// Builds the graph for the workspace at `manifest_path` (current one if `None`).
///
/// All features are enabled so that optional dependencies behind non-default
/// features are checked too.
fn graph_for(manifest_path: Option<&Path>) -> anyhow::Result<Graph> {
    let mut cmd = MetadataCommand::new();
    cmd.features(CargoOpt::AllFeatures);
    if let Some(path) = manifest_path {
        cmd.manifest_path(path);
    }
    let metadata = cmd.exec().context("cargo metadata failed")?;
    let names: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|p| (p.id.clone(), p.name.to_string()))
        .collect();
    let mut graph = Graph {
        members: metadata
            .workspace_members
            .iter()
            .filter_map(|id| names.get(id).cloned())
            .collect(),
        ..Graph::default()
    };
    let resolve = metadata
        .resolve
        .context("cargo metadata returned no resolve graph")?;
    for node in resolve.nodes {
        let from = names
            .get(&node.id)
            .with_context(|| format!("unknown package id {}", node.id))?;
        for dep in node.deps {
            let to = names
                .get(&dep.pkg)
                .with_context(|| format!("unknown package id {}", dep.pkg))?;
            let dev_only = !dep.dep_kinds.is_empty()
                && dep
                    .dep_kinds
                    .iter()
                    .all(|k| k.kind == DependencyKind::Development);
            graph.add_edge(from, to, dev_only);
        }
    }
    Ok(graph)
}

#[derive(Deserialize)]
struct AllowedDeps {
    allowed: BTreeSet<String>,
}

fn load_allowed_deps(workspace_root: &Path) -> anyhow::Result<BTreeSet<String>> {
    let path = workspace_root.join("crates/connectors/allowed-deps.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let parsed: AllowedDeps =
        toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
    Ok(parsed.allowed)
}

fn workspace_root() -> &'static Path {
    // xtask lives at <root>/xtask.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
}

pub fn run() -> anyhow::Result<()> {
    let graph = workspace_graph()?;
    let allowed = load_allowed_deps(workspace_root())?;
    let violations = check(&graph, &allowed);
    if violations.is_empty() {
        println!(
            "check-deps: OK ({} workspace crates checked)",
            graph.members.len()
        );
        return Ok(());
    }
    for v in &violations {
        eprintln!("check-deps: {v}");
    }
    anyhow::bail!("{} dependency rule violation(s)", violations.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The graph exactly as allowed by the rules (one external crate on the server).
    fn reference_graph() -> Graph {
        let mut g = Graph::default();
        for (from, deps) in ALLOWED_EDGES {
            g.members.insert((*from).to_owned());
            for to in *deps {
                g.add_edge(from, to, false);
            }
        }
        g.add_edge("em-server", "tokio", false);
        g
    }

    #[test]
    fn reference_graph_passes() {
        assert_eq!(
            check(&reference_graph(), &BTreeSet::new()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn real_workspace_passes() {
        let graph = workspace_graph().unwrap();
        let allowed = load_allowed_deps(workspace_root()).unwrap();
        assert_eq!(check(&graph, &allowed), Vec::<String>::new());
    }

    #[test]
    fn api_to_secrets_fails() {
        let mut g = reference_graph();
        g.add_edge("em-api", "em-secrets", false);
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter()
                .any(|m| m.starts_with("em-api ->* em-secrets: forbidden")),
            "{v:?}"
        );
        assert!(
            v.iter()
                .any(|m| m.starts_with("em-api -> em-secrets: direct")),
            "{v:?}"
        );
    }

    #[test]
    fn api_to_secrets_as_dev_dependency_fails() {
        let mut g = reference_graph();
        g.add_edge("em-api", "em-secrets", true);
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter().any(|m| m.starts_with("em-api ->* em-secrets")),
            "{v:?}"
        );
    }

    #[test]
    fn transitive_secrets_leak_is_caught() {
        // storage -> secrets is not a direct api edge, but api reaches it through
        // sync-facade -> services -> storage.
        let mut g = reference_graph();
        g.add_edge("em-storage", "em-secrets", false);
        let v = check(&g, &BTreeSet::new());
        for from in ["em-api", "em-mcp", "em-services"] {
            assert!(
                v.iter()
                    .any(|m| m.starts_with(&format!("{from} ->* em-secrets"))),
                "{from}: {v:?}"
            );
        }
    }

    #[test]
    fn transitive_through_external_crate_is_caught() {
        let mut g = reference_graph();
        g.add_edge("em-pricing", "some-ext", false);
        g.add_edge("some-ext", "em-secrets", false);
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter().any(|m| m.starts_with("em-pricing ->* em-secrets")),
            "{v:?}"
        );
    }

    #[test]
    fn api_to_sync_fails() {
        let mut g = reference_graph();
        g.add_edge("em-mcp", "em-sync", false);
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter().any(|m| m.starts_with("em-mcp ->* em-sync:")),
            "{v:?}"
        );
    }

    #[test]
    fn connector_rules() {
        let mut g = reference_graph();
        g.add_edge("em-connector-fake-bank", "em-core-domain", false);
        g.add_edge("em-connector-sdk", "reqwest", false);
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter()
                .any(|m| m.starts_with("em-connector-fake-bank ->* em-core-domain: forbidden")),
            "{v:?}"
        );
        assert!(
            v.iter()
                .any(|m| m.starts_with("em-connector-sdk ->* reqwest: not allowed")),
            "{v:?}"
        );
        // Allow-listing the external crate clears the connector rule for it.
        let allowed: BTreeSet<String> = ["reqwest".to_owned()].into();
        assert!(!check(&g, &allowed).iter().any(|m| m.contains("reqwest")));
    }

    #[test]
    fn optional_dependency_behind_feature_fails() {
        // A throwaway workspace where em-api reaches em-secrets only through an
        // optional dependency enabled by a non-default feature.
        let root =
            std::env::temp_dir().join(format!("em-check-deps-feature-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, manifest) in [
            (
                "secrets",
                "[package]\nname = \"em-secrets\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
            ),
            (
                "api",
                "[package]\nname = \"em-api\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
                 [dependencies]\nem-secrets = { path = \"../secrets\", optional = true }\n\n\
                 [features]\ndefault = []\nleak = [\"dep:em-secrets\"]\n",
            ),
        ] {
            std::fs::create_dir_all(root.join(dir).join("src")).unwrap();
            std::fs::write(root.join(dir).join("Cargo.toml"), manifest).unwrap();
            std::fs::write(root.join(dir).join("src/lib.rs"), "").unwrap();
        }
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\nmembers = [\"api\", \"secrets\"]\n",
        )
        .unwrap();

        let graph = graph_for(Some(&root.join("Cargo.toml"))).unwrap();
        let v = check(&graph, &BTreeSet::new());
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            v.iter()
                .any(|m| m.starts_with("em-api ->* em-secrets: forbidden")),
            "{v:?}"
        );
    }

    #[test]
    fn undeclared_crate_fails() {
        let mut g = reference_graph();
        g.members.insert("em-new".to_owned());
        let v = check(&g, &BTreeSet::new());
        assert!(
            v.iter().any(|m| m.starts_with("em-new: not declared")),
            "{v:?}"
        );
    }
}
