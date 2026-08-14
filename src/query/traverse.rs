//! Bounded breadth-first traversal of the code graph.
//!
//! Shared by every command that answers a "what connects to this" question:
//! callers, callees, impact radius, and the neighbourhood expansion behind
//! `map`. Centralized because the bounds are the interesting part — an
//! unbounded walk on a real repo reaches most of it within four hops and
//! returns a result too large to be worth reading, which defeats the purpose of
//! a token-saving tool.
//!
//! Results are returned in BFS order, so callers can truncate to a budget and
//! keep the closest, most relevant symbols.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::graph::types::{EdgeKind, EdgeV1, GraphV1};

/// Which way to follow edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges forwards: what this symbol uses.
    Out,
    /// Follow edges backwards: what uses this symbol.
    In,
    /// Both, which is what "impact" means.
    Both,
}

impl Direction {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "out" | "callees" | "down" => Direction::Out,
            "in" | "callers" | "up" => Direction::In,
            "both" | "all" => Direction::Both,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
            Direction::Both => "both",
        }
    }
}

/// A symbol reached by the walk, with how far away it is and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reached {
    pub id: String,
    /// Hops from the start node. The start itself is depth 0.
    pub depth: usize,
    /// The edge kind that first reached this node.
    pub via: EdgeKind,
}

/// Adjacency prepared once and walked many times.
pub struct Neighbours<'a> {
    out: BTreeMap<&'a str, Vec<(&'a str, EdgeKind)>>,
    incoming: BTreeMap<&'a str, Vec<(&'a str, EdgeKind)>>,
}

impl<'a> Neighbours<'a> {
    pub fn build(edges: &'a [EdgeV1]) -> Self {
        let mut out: BTreeMap<&str, Vec<(&str, EdgeKind)>> = BTreeMap::new();
        let mut incoming: BTreeMap<&str, Vec<(&str, EdgeKind)>> = BTreeMap::new();

        for edge in edges {
            out.entry(edge.from.as_str())
                .or_default()
                .push((edge.to.as_str(), edge.kind));
            incoming
                .entry(edge.to.as_str())
                .or_default()
                .push((edge.from.as_str(), edge.kind));
        }

        Self { out, incoming }
    }

    fn step(&self, id: &str, direction: Direction) -> Vec<(&'a str, EdgeKind)> {
        let mut next = Vec::new();

        if matches!(direction, Direction::Out | Direction::Both) {
            if let Some(edges) = self.out.get(id) {
                next.extend(edges.iter().copied());
            }
        }
        if matches!(direction, Direction::In | Direction::Both) {
            if let Some(edges) = self.incoming.get(id) {
                next.extend(edges.iter().copied());
            }
        }

        next
    }
}

/// Walk outwards from `start`, up to `max_depth` hops.
///
/// The start node is not included in the result: the caller already knows about
/// it, and spending output tokens restating the question is exactly the waste
/// this tool exists to avoid.
///
/// `kinds` restricts which edge kinds may be traversed. An empty slice means
/// every dependency edge — see [`EdgeKind::is_dependency`]. Containment is
/// reachable only by naming it, because "callers of X" must not include every
/// symbol that merely shares a file with X.
pub fn walk(
    graph: &GraphV1,
    start: &str,
    direction: Direction,
    max_depth: usize,
    kinds: &[EdgeKind],
) -> Vec<Reached> {
    let neighbours = Neighbours::build(&graph.edges);
    walk_with(&neighbours, start, direction, max_depth, kinds)
}

/// Walk using a prebuilt adjacency, for callers issuing many walks over one
/// graph.
pub fn walk_with(
    neighbours: &Neighbours,
    start: &str,
    direction: Direction,
    max_depth: usize,
    kinds: &[EdgeKind],
) -> Vec<Reached> {
    if max_depth == 0 {
        return Vec::new();
    }

    let allowed = |kind: EdgeKind| {
        if kinds.is_empty() {
            kind.is_dependency()
        } else {
            kinds.contains(&kind)
        }
    };

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    seen.insert(start);

    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
    queue.push_back((start, 0));

    let mut reached = Vec::new();

    while let Some((id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        for (neighbour, kind) in neighbours.step(id, direction) {
            if !allowed(kind) || !seen.insert(neighbour) {
                continue;
            }

            reached.push(Reached {
                id: neighbour.to_string(),
                depth: depth + 1,
                via: kind,
            });
            queue.push_back((neighbour, depth + 1));
        }
    }

    reached
}

/// Direct neighbours only, the common case for "who calls this".
pub fn immediate(graph: &GraphV1, start: &str, direction: Direction) -> Vec<Reached> {
    walk(graph, start, direction, 1, &[EdgeKind::Calls])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeV1, Span};

    fn graph(edges: Vec<EdgeV1>) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test");
        let ids: BTreeSet<String> = edges
            .iter()
            .flat_map(|e| [e.from.clone(), e.to.clone()])
            .collect();
        g.nodes = ids
            .into_iter()
            .map(|id| {
                NodeV1::new(
                    id.clone(),
                    NodeKind::Function,
                    id,
                    "src/a.ts".to_string(),
                    Span::new(1, 2),
                )
            })
            .collect();
        g.edges = edges;
        g.normalize();
        g
    }

    fn ids(reached: &[Reached]) -> Vec<&str> {
        reached.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn outward_walk_finds_callees() {
        let g = graph(vec![EdgeV1::new("a", "b", EdgeKind::Calls)]);

        assert_eq!(ids(&walk(&g, "a", Direction::Out, 3, &[])), vec!["b"]);
    }

    #[test]
    fn inward_walk_finds_callers() {
        let g = graph(vec![EdgeV1::new("a", "b", EdgeKind::Calls)]);

        assert_eq!(ids(&walk(&g, "b", Direction::In, 3, &[])), vec!["a"]);
    }

    #[test]
    fn both_directions_reach_callers_and_callees() {
        let g = graph(vec![
            EdgeV1::new("caller", "mid", EdgeKind::Calls),
            EdgeV1::new("mid", "callee", EdgeKind::Calls),
        ]);

        let walked = walk(&g, "mid", Direction::Both, 1, &[]);
        let reached = ids(&walked);

        assert!(reached.contains(&"caller"));
        assert!(reached.contains(&"callee"));
    }

    #[test]
    fn depth_bounds_the_walk() {
        let g = graph(vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("b", "c", EdgeKind::Calls),
            EdgeV1::new("c", "d", EdgeKind::Calls),
        ]);

        assert_eq!(ids(&walk(&g, "a", Direction::Out, 2, &[])), vec!["b", "c"]);
    }

    #[test]
    fn depth_zero_reaches_nothing() {
        let g = graph(vec![EdgeV1::new("a", "b", EdgeKind::Calls)]);

        assert!(walk(&g, "a", Direction::Out, 0, &[]).is_empty());
    }

    #[test]
    fn the_start_node_is_not_in_its_own_results() {
        let g = graph(vec![EdgeV1::new("a", "b", EdgeKind::Calls)]);

        assert!(!ids(&walk(&g, "a", Direction::Out, 3, &[])).contains(&"a"));
    }

    /// Cycles are ubiquitous in real code (mutual recursion, back-references),
    /// so the visited set has to hold or the walk never terminates.
    #[test]
    fn cycles_terminate() {
        let g = graph(vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("b", "a", EdgeKind::Calls),
        ]);

        assert_eq!(ids(&walk(&g, "a", Direction::Out, 10, &[])), vec!["b"]);
    }

    #[test]
    fn a_self_loop_terminates() {
        let g = graph(vec![EdgeV1::new("a", "a", EdgeKind::Calls)]);

        assert!(walk(&g, "a", Direction::Out, 5, &[]).is_empty());
    }

    #[test]
    fn depth_is_the_shortest_hop_count() {
        let g = graph(vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("b", "c", EdgeKind::Calls),
            EdgeV1::new("a", "c", EdgeKind::Calls),
        ]);

        let reached = walk(&g, "a", Direction::Out, 3, &[]);
        let c = reached.iter().find(|r| r.id == "c").expect("c reached");

        assert_eq!(c.depth, 1);
    }

    #[test]
    fn edge_kinds_can_be_restricted() {
        let g = graph(vec![
            EdgeV1::new("a", "called", EdgeKind::Calls),
            EdgeV1::new("a", "imported", EdgeKind::Imports),
        ]);

        assert_eq!(
            ids(&walk(&g, "a", Direction::Out, 3, &[EdgeKind::Calls])),
            vec!["called"]
        );
    }

    #[test]
    fn an_unknown_start_node_reaches_nothing() {
        let g = graph(vec![EdgeV1::new("a", "b", EdgeKind::Calls)]);

        assert!(walk(&g, "missing", Direction::Out, 3, &[]).is_empty());
    }

    #[test]
    fn traversal_order_is_stable_across_runs() {
        let g = graph(vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("a", "c", EdgeKind::Calls),
            EdgeV1::new("b", "d", EdgeKind::Calls),
            EdgeV1::new("c", "d", EdgeKind::Calls),
        ]);

        let first = walk(&g, "a", Direction::Out, 3, &[]);
        for _ in 0..5 {
            assert_eq!(walk(&g, "a", Direction::Out, 3, &[]), first);
        }
    }

    #[test]
    fn direction_parses_its_aliases() {
        assert_eq!(Direction::parse("callers"), Some(Direction::In));
        assert_eq!(Direction::parse("callees"), Some(Direction::Out));
        assert_eq!(Direction::parse("both"), Some(Direction::Both));
        assert_eq!(Direction::parse("sideways"), None);
    }
}
