//! Reference resolution: unresolved references in, graph edges out.
//!
//! Resolution runs once, after every file has been extracted, because a call in
//! one file usually targets a declaration in another.
//!
//! The governing principle is **precision over recall**. A wrong edge is worse
//! than a missing one: it pollutes impact analysis, misleads `dead-code`, and
//! sends an agent to read the wrong file. So when a name is ambiguous and
//! nothing narrows it down, the reference is dropped rather than guessed. This
//! is why [`ResolveStats::ambiguous_dropped`] exists — it makes the cost of
//! that choice measurable instead of invisible.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::graph::extract::{FileExtraction, ImportBinding, Receiver};
use crate::graph::modpath;
use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, NodeV1};

/// How far to walk an inheritance chain looking for an inherited method.
///
/// Three levels covers essentially every real hierarchy while bounding the
/// search on pathological ones. Beyond this depth the connection is too weak to
/// be worth a possibly-wrong edge.
const MAX_INHERITANCE_DEPTH: usize = 3;

/// Counts describing what resolution could and could not do.
///
/// Surfaced by `tok mem check` so graph quality is observable rather than a
/// matter of faith.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolveStats {
    pub edges_created: usize,
    /// References whose name matched nothing in the graph — typically calls
    /// into third-party or standard-library code.
    pub unresolved: usize,
    /// References dropped because several candidates matched equally well.
    pub ambiguous_dropped: usize,
}

/// Build the edge set for a whole repository.
pub fn resolve(files: &[FileExtraction]) -> (Vec<EdgeV1>, ResolveStats) {
    let index = SymbolIndex::build(files);
    let mut edges = Vec::new();
    let mut stats = ResolveStats::default();

    for file in files {
        resolve_containment(file, &index, &mut edges);
        resolve_imports(file, &index, &mut edges, &mut stats);
        resolve_named_relations(file, &index, &mut edges, &mut stats);
        resolve_refs(file, &index, &mut edges, &mut stats);
    }

    edges.sort();
    edges.dedup();
    stats.edges_created = edges.len();

    (edges, stats)
}

/// Name-based lookup tables over every node in the repository.
struct SymbolIndex<'a> {
    /// Every node, by id.
    by_id: HashMap<&'a str, &'a NodeV1>,
    /// Candidate nodes for a bare name, across all files.
    by_name: HashMap<&'a str, Vec<&'a NodeV1>>,
    /// Methods keyed by (owning type id, method name).
    methods_by_owner: HashMap<(&'a str, &'a str), &'a NodeV1>,
    /// Owning type id for methods whose owner is named rather than lexical,
    /// which is to say Go's receiver-declared methods.
    owner_of_method: HashMap<&'a str, &'a str>,
    /// Type node ids a given type id extends or implements.
    supertypes: HashMap<String, Vec<String>>,
    /// Per-file `(variable -> type name)` bindings.
    bindings: HashMap<&'a str, BTreeMap<&'a str, &'a str>>,
    /// Import statements per file, used to narrow ambiguous names to the module
    /// they were actually imported from.
    imports_by_file: HashMap<&'a str, &'a [ImportBinding]>,
}

impl<'a> SymbolIndex<'a> {
    fn build(files: &'a [FileExtraction]) -> Self {
        let mut by_id = HashMap::new();
        let mut by_name: HashMap<&str, Vec<&NodeV1>> = HashMap::new();
        let mut methods_by_owner = HashMap::new();
        let mut bindings: HashMap<&str, BTreeMap<&str, &str>> = HashMap::new();
        let mut imports_by_file = HashMap::new();

        for file in files {
            imports_by_file.insert(file.path.as_str(), file.imports.as_slice());

            for node in &file.nodes {
                by_id.insert(node.id.as_str(), node);
                if node.kind != NodeKind::File {
                    by_name.entry(node.name.as_str()).or_default().push(node);
                }
                if let (NodeKind::Method, Some(parent)) = (node.kind, node.parent.as_deref()) {
                    methods_by_owner.insert((parent, node.name.as_str()), node);
                }
            }

            let file_bindings = bindings.entry(file.path.as_str()).or_default();
            for (var, ty) in &file.bindings {
                // First binding wins: a later shadow in a different scope is
                // more likely to mislead than to help, since extraction is not
                // scope-precise about locals.
                file_bindings.entry(var.as_str()).or_insert(ty.as_str());
            }
        }

        let mut index = Self {
            by_id,
            by_name,
            methods_by_owner,
            owner_of_method: HashMap::new(),
            supertypes: HashMap::new(),
            bindings,
            imports_by_file,
        };
        index.attach_named_owners(files);
        index.supertypes = index.build_supertypes(files);
        index
    }

    /// Attach methods whose owner was declared by name to their type.
    ///
    /// Runs after the name index exists, because the receiver type of a Go
    /// method may be declared in another file of the same package.
    fn attach_named_owners(&mut self, files: &'a [FileExtraction]) {
        for file in files {
            for (method_id, type_name) in &file.method_owners {
                let (Some(method), Some(owner)) = (
                    self.by_id.get(method_id.as_str()).copied(),
                    self.type_named_near(type_name, &file.path),
                ) else {
                    continue;
                };

                self.owner_of_method
                    .insert(method.id.as_str(), owner.id.as_str());
                self.methods_by_owner
                    .insert((owner.id.as_str(), method.name.as_str()), method);
            }
        }
    }

    /// The type that owns a method, whether by lexical nesting or by receiver.
    fn owner_of(&self, node: &'a NodeV1) -> Option<&'a str> {
        node.parent
            .as_deref()
            .or_else(|| self.owner_of_method.get(node.id.as_str()).copied())
    }

    /// Resolve a name through the import that brought it into this file.
    ///
    /// This is what separates `./util`'s `normalize` from the three other
    /// `normalize`s in the repository. Without it, every shared helper name
    /// collapses into an ambiguity drop.
    fn via_import(&self, file: &str, local: &str) -> Option<&'a NodeV1> {
        let binding = self
            .imports_by_file
            .get(file)?
            .iter()
            .find(|i| i.local == local)?;

        let target_files = modpath::candidates(file, &binding.module);
        if target_files.is_empty() {
            return None;
        }

        let matches: Vec<_> = self
            .by_name
            .get(binding.imported.as_str())?
            .iter()
            .filter(|n| target_files.iter().any(|f| f == &n.file))
            .collect();

        match matches.as_slice() {
            [only] => Some(**only),
            _ => None,
        }
    }

    /// Map each type to the types it inherits from, for method lookup.
    fn build_supertypes(&self, files: &'a [FileExtraction]) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();

        for file in files {
            for r in &file.refs {
                if !matches!(r.kind, EdgeKind::Extends | EdgeKind::Implements) {
                    continue;
                }
                if let Some(target) = self.type_named_near(&r.name, &file.path) {
                    out.entry(r.from.clone())
                        .or_default()
                        .push(target.id.clone());
                }
            }

            for relation in &file.named_relations {
                if !matches!(relation.kind, EdgeKind::Extends | EdgeKind::Implements) {
                    continue;
                }
                let (Some(from), Some(to)) = (
                    self.type_named_near(&relation.from_name, &file.path),
                    self.type_named_near(&relation.to_name, &file.path),
                ) else {
                    continue;
                };
                out.entry(from.id.clone()).or_default().push(to.id.clone());
            }
        }

        out
    }

    /// The single type-like node with this name, or `None` when absent or
    /// ambiguous.
    fn unique_type_named(&self, name: &str) -> Option<&'a NodeV1> {
        self.type_named_near(name, "")
    }

    /// The type with this name, preferring one declared near `file`.
    ///
    /// Names like `Cache` recur across a repository, so a globally unique match
    /// is too strict for relations that are inherently local — a Go receiver
    /// type, or a Rust `impl` block, resolves within its own file or package.
    /// Locality narrows those without licensing a cross-repository guess.
    fn type_named_near(&self, name: &str, file: &str) -> Option<&'a NodeV1> {
        let candidates: Vec<&'a NodeV1> = self
            .by_name
            .get(name)?
            .iter()
            .filter(|n| n.kind.is_type_like())
            .copied()
            .collect();

        if let [only] = candidates.as_slice() {
            return Some(only);
        }
        if file.is_empty() {
            return None;
        }

        let in_file = only_one(candidates.iter().filter(|n| n.file == file));
        if in_file.is_some() {
            return in_file;
        }

        let dir = directory_of(file);
        only_one(
            candidates
                .iter()
                .filter(|n| directory_of(&n.file) == dir && same_language(&n.file, file)),
        )
    }

    /// Resolve a bare name to a single callable target.
    ///
    /// `Ok(None)` means nothing matched; `Err(Ambiguous)` means several did.
    fn unique_callable(&self, name: &str, prefer_file: &str) -> Resolution<'a> {
        let callable: Vec<_> = self
            .by_name
            .get(name)
            .map(|c| {
                c.iter()
                    .filter(|n| n.kind.is_callable() || n.kind.is_type_like())
                    .copied()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        match callable.as_slice() {
            // An aliased import names nothing locally, so the module is the
            // only route to a target.
            [] => match self.via_import(prefer_file, name) {
                Some(found) => Resolution::Found(found),
                None => Resolution::Missing,
            },
            [only] => Resolution::Found(only),
            many => {
                // A same-file definition wins: local shadowing is the common
                // case and resolving to it is almost always right.
                let local: Vec<_> = many.iter().filter(|n| n.file == prefer_file).collect();
                if let [only] = local.as_slice() {
                    return Resolution::Found(only);
                }

                match self.via_import(prefer_file, name) {
                    Some(found) => Resolution::Found(found),
                    None => Resolution::Ambiguous,
                }
            }
        }
    }

    /// Find a method on a type, walking up the inheritance chain.
    ///
    /// Breadth-first so the nearest definition wins when a subclass overrides
    /// a base method. The visited set makes a circular hierarchy terminate.
    fn method_on_type(&self, type_id: &str, method: &str) -> Option<&'a NodeV1> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
        queue.push_back((type_id, 0));

        while let Some((current, depth)) = queue.pop_front() {
            if depth > MAX_INHERITANCE_DEPTH || !seen.insert(current.to_string()) {
                continue;
            }

            if let Some(found) = self.methods_by_owner.get(&(current, method)) {
                return Some(found);
            }

            if let Some(parents) = self.supertypes.get(current) {
                for parent in parents {
                    queue.push_back((parent.as_str(), depth + 1));
                }
            }
        }

        None
    }

    /// The type name bound to `var` within `file`, if extraction inferred one.
    fn binding(&self, file: &str, var: &str) -> Option<&'a str> {
        self.bindings.get(file)?.get(var).copied()
    }
}

/// The single element of an iterator, or `None` for zero or many.
fn only_one<'a, 'b, I>(mut items: I) -> Option<&'a NodeV1>
where
    I: Iterator<Item = &'b &'a NodeV1>,
    'a: 'b,
{
    let first = *items.next()?;
    match items.next() {
        Some(_) => None,
        None => Some(first),
    }
}

/// Everything before the last slash; the empty string for a root-level file.
fn directory_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Whether two paths share a file extension, which stands in for "same
/// language" when deciding whether a sibling file could plausibly declare a
/// referenced type.
fn same_language(a: &str, b: &str) -> bool {
    let ext = |p: &str| p.rsplit_once('.').map(|(_, e)| e.to_string());
    ext(a) == ext(b)
}

/// Outcome of resolving one reference.
enum Resolution<'a> {
    Found(&'a NodeV1),
    Missing,
    Ambiguous,
}

/// `CONTAINS` edges from a file to its top-level declarations, and from a type
/// to its methods.
fn resolve_containment(file: &FileExtraction, index: &SymbolIndex, edges: &mut Vec<EdgeV1>) {
    let file_id = &file.path;

    for node in &file.nodes {
        if node.kind == NodeKind::File {
            continue;
        }

        let owner = index.owner_of(node).unwrap_or(file_id);
        edges.push(EdgeV1::new(owner, &node.id, EdgeKind::Contains));

        // A method owned by a type in another file still belongs to its file.
        if owner != file_id.as_str() && index.by_id.get(owner).is_some_and(|o| o.file != file.path)
        {
            edges.push(EdgeV1::new(file_id, &node.id, EdgeKind::Contains));
        }
    }
}

/// `IMPORTS` edges from the importing file to the imported symbol.
fn resolve_imports(
    file: &FileExtraction,
    index: &SymbolIndex,
    edges: &mut Vec<EdgeV1>,
    stats: &mut ResolveStats,
) {
    for import in &file.imports {
        // Namespace and default imports name no specific symbol.
        if import.imported == "*" || import.imported == "default" {
            continue;
        }

        // The specifier is the strongest signal available; only fall back to a
        // bare name match when it points outside the repository.
        let resolved = match index.via_import(&file.path, &import.local) {
            Some(target) => Resolution::Found(target),
            None => index.unique_callable(&import.imported, &file.path),
        };

        match resolved {
            Resolution::Found(target) => {
                // Skip self-imports, which a re-export can produce.
                if target.file != file.path {
                    edges.push(EdgeV1::new(&file.path, &target.id, EdgeKind::Imports));
                }
            }
            Resolution::Missing => stats.unresolved += 1,
            Resolution::Ambiguous => stats.ambiguous_dropped += 1,
        }
    }
}

/// Inheritance declared between two names, such as Rust's `impl Trait for Type`.
fn resolve_named_relations(
    file: &FileExtraction,
    index: &SymbolIndex,
    edges: &mut Vec<EdgeV1>,
    stats: &mut ResolveStats,
) {
    for relation in &file.named_relations {
        let (Some(from), Some(to)) = (
            index.type_named_near(&relation.from_name, &file.path),
            index.type_named_near(&relation.to_name, &file.path),
        ) else {
            stats.unresolved += 1;
            continue;
        };
        edges.push(EdgeV1::new(&from.id, &to.id, relation.kind));
    }
}

/// Calls, inheritance, and type references recorded against a known source node.
fn resolve_refs(
    file: &FileExtraction,
    index: &SymbolIndex,
    edges: &mut Vec<EdgeV1>,
    stats: &mut ResolveStats,
) {
    for r in &file.refs {
        // A reference from a node that no longer exists cannot be placed.
        if !index.by_id.contains_key(r.from.as_str()) {
            continue;
        }

        let resolved = match &r.receiver {
            // `self.method()` — look on the enclosing type.
            Some(Receiver::SelfType) => resolve_self_call(file, index, &r.from, &r.name),

            // `Type::method()` — look on the named type.
            Some(Receiver::Typed(type_name)) => index
                .type_named_near(type_name, &file.path)
                .and_then(|t| index.method_on_type(&t.id, &r.name))
                .map(Resolution::Found)
                .unwrap_or(Resolution::Missing),

            // `foo.method()` — use the inferred type of `foo` when we have one,
            // otherwise fall back to a global unique-name match.
            Some(Receiver::Untyped(var)) => resolve_untyped_call(file, index, var, &r.name),

            None => index.unique_callable(&r.name, &file.path),
        };

        match resolved {
            Resolution::Found(target) => {
                // A self-edge says nothing useful and distorts centrality.
                if target.id != r.from {
                    edges.push(EdgeV1::new(&r.from, &target.id, r.kind));
                }
            }
            Resolution::Missing => stats.unresolved += 1,
            Resolution::Ambiguous => stats.ambiguous_dropped += 1,
        }
    }
}

fn resolve_self_call<'a>(
    file: &FileExtraction,
    index: &SymbolIndex<'a>,
    from_id: &str,
    method: &str,
) -> Resolution<'a> {
    let Some(from) = index.by_id.get(from_id) else {
        return Resolution::Missing;
    };

    // The enclosing type is the caller's owner, or the caller itself when the
    // reference came from the type's own body.
    let owner = index.owner_of(from).unwrap_or(from.id.as_str());

    match index.method_on_type(owner, method) {
        Some(found) => Resolution::Found(found),
        // A `self.x()` that matches no method is often a field holding a
        // closure; fall back to a file-local name match rather than inventing
        // a cross-file edge.
        None => match index.unique_callable(method, &file.path) {
            Resolution::Found(n) if n.file == file.path => Resolution::Found(n),
            _ => Resolution::Missing,
        },
    }
}

fn resolve_untyped_call<'a>(
    file: &FileExtraction,
    index: &SymbolIndex<'a>,
    var: &str,
    method: &str,
) -> Resolution<'a> {
    // A known local type turns this into a precise method lookup.
    if let Some(type_name) = index.binding(&file.path, var) {
        if let Some(ty) = index.type_named_near(type_name, &file.path) {
            if let Some(found) = index.method_on_type(&ty.id, method) {
                return Resolution::Found(found);
            }
        }
    }

    // `foo` may itself be an imported module or a type used statically.
    if let Some(ty) = index.type_named_near(var, &file.path) {
        if let Some(found) = index.method_on_type(&ty.id, method) {
            return Resolution::Found(found);
        }
    }

    // Last resort: a globally unique method name. Anything ambiguous is
    // dropped, which is the whole point of this pass.
    let candidates: Vec<_> = index
        .by_name
        .get(method)
        .map(|v| v.iter().filter(|n| n.kind.is_callable()).collect())
        .unwrap_or_default();

    match candidates.as_slice() {
        [] => Resolution::Missing,
        [only] => Resolution::Found(only),
        _ => Resolution::Ambiguous,
    }
}

#[cfg(all(test, feature = "graph"))]
mod tests {
    use super::*;
    use crate::graph::extract::extract_file;
    use crate::graph::Language;

    fn extract_all(files: &[(&str, &str, Language)]) -> Vec<FileExtraction> {
        files
            .iter()
            .map(|(path, src, lang)| {
                extract_file(path, src, *lang)
                    .expect("extraction succeeds")
                    .expect("grammar available")
            })
            .collect()
    }

    fn edge_names(files: &[FileExtraction], edges: &[EdgeV1], kind: EdgeKind) -> Vec<String> {
        let nodes: HashMap<&str, &NodeV1> = files
            .iter()
            .flat_map(|f| f.nodes.iter())
            .map(|n| (n.id.as_str(), n))
            .collect();

        edges
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| {
                let from = nodes
                    .get(e.from.as_str())
                    .map(|n| n.name.as_str())
                    .unwrap_or(&e.from);
                let to = nodes
                    .get(e.to.as_str())
                    .map(|n| n.name.as_str())
                    .unwrap_or(&e.to);
                format!("{from}->{to}")
            })
            .collect()
    }

    #[test]
    fn resolves_a_plain_call() {
        let files = extract_all(&[(
            "a.rs",
            "fn helper() {}\nfn main() { helper(); }",
            Language::Rust,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"main->helper".to_string()));
    }

    #[test]
    fn resolves_a_call_across_files() {
        let files = extract_all(&[
            ("util.ts", "export function normalize(s: string) { return s; }", Language::TypeScript),
            (
                "cache.ts",
                "import { normalize } from './util';\nexport function get(k: string) { return normalize(k); }",
                Language::TypeScript,
            ),
        ]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"get->normalize".to_string()));
    }

    #[test]
    fn resolves_self_method_calls() {
        let files = extract_all(&[(
            "c.ts",
            "class Cache { read() {} load() { this.read(); } }",
            Language::TypeScript,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"load->read".to_string()));
    }

    #[test]
    fn resolves_a_method_through_a_typed_local() {
        let files = extract_all(&[(
            "c.ts",
            "class Cache { read() {} }\nfunction go() { const c = new Cache(); c.read(); }",
            Language::TypeScript,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"go->read".to_string()));
    }

    #[test]
    fn resolves_inherited_methods() {
        let files = extract_all(&[(
            "c.ts",
            "class Base { read() {} }\nclass Cache extends Base {}\n\
             function go() { const c = new Cache(); c.read(); }",
            Language::TypeScript,
        )]);
        let (edges, _) = resolve(&files);
        assert!(
            edge_names(&files, &edges, EdgeKind::Calls).contains(&"go->read".to_string()),
            "method inherited from a base class should resolve"
        );
    }

    /// Precision over recall: two identically named methods on unrelated types,
    /// called through an untyped receiver, must not produce a guessed edge.
    #[test]
    fn ambiguous_method_calls_are_dropped_not_guessed() {
        let files = extract_all(&[(
            "c.ts",
            "class A { save() {} }\nclass B { save() {} }\n\
             function go(thing) { thing.save(); }",
            Language::TypeScript,
        )]);
        let (edges, stats) = resolve(&files);

        let calls = edge_names(&files, &edges, EdgeKind::Calls);
        assert!(
            !calls.iter().any(|e| e.starts_with("go->save")),
            "should not guess between A.save and B.save, got {calls:?}"
        );
        assert_eq!(
            stats.ambiguous_dropped, 1,
            "the drop is counted, not hidden"
        );
    }

    #[test]
    fn calls_into_unknown_code_are_counted_as_unresolved() {
        let files = extract_all(&[(
            "a.rs",
            "fn main() { some_external_crate_fn(); }",
            Language::Rust,
        )]);
        let (_, stats) = resolve(&files);
        assert!(stats.unresolved >= 1);
    }

    #[test]
    fn rust_impl_for_becomes_an_implements_edge() {
        let files = extract_all(&[(
            "a.rs",
            "pub struct M;\npub trait T {}\nimpl T for M {}",
            Language::Rust,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Implements).contains(&"M->T".to_string()));
    }

    #[test]
    fn typescript_extends_and_implements_resolve_separately() {
        let files = extract_all(&[(
            "c.ts",
            "interface Storable {}\nclass Base {}\nclass Cache extends Base implements Storable {}",
            Language::TypeScript,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Extends).contains(&"Cache->Base".to_string()));
        assert!(edge_names(&files, &edges, EdgeKind::Implements)
            .contains(&"Cache->Storable".to_string()));
    }

    #[test]
    fn imports_link_files_to_symbols() {
        let files = extract_all(&[
            (
                "util.ts",
                "export function normalize(s) { return s; }",
                Language::TypeScript,
            ),
            (
                "cache.ts",
                "import { normalize } from './util';",
                Language::TypeScript,
            ),
        ]);
        let (edges, _) = resolve(&files);
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Imports && e.from == "cache.ts"));
    }

    #[test]
    fn files_contain_their_declarations() {
        let files = extract_all(&[("a.rs", "pub fn f() {}", Language::Rust)]);
        let (edges, _) = resolve(&files);
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Contains && e.from == "a.rs" && e.to == "a.rs::f"));
    }

    #[test]
    fn types_contain_their_methods() {
        let files = extract_all(&[("c.ts", "class Cache { read() {} }", Language::TypeScript)]);
        let (edges, _) = resolve(&files);
        assert!(edges.iter().any(|e| {
            e.kind == EdgeKind::Contains && e.from == "c.ts::Cache" && e.to == "c.ts::read"
        }));
    }

    #[test]
    fn no_self_edges_are_emitted() {
        // Direct recursion would otherwise produce f->f, which distorts
        // centrality without telling a reader anything.
        let files = extract_all(&[("a.rs", "fn f() { f(); }", Language::Rust)]);
        let (edges, _) = resolve(&files);
        assert!(!edges.iter().any(|e| e.from == e.to));
    }

    #[test]
    fn resolution_is_deterministic() {
        let src = &[
            (
                "a.rs",
                "pub fn one() {}\npub fn two() { one(); }",
                Language::Rust,
            ),
            ("b.rs", "pub fn three() {}", Language::Rust),
        ];
        let first = resolve(&extract_all(src)).0;
        let second = resolve(&extract_all(src)).0;
        assert_eq!(first, second);
    }

    #[test]
    fn inheritance_search_terminates_on_a_cycle() {
        // A malformed or circular hierarchy must not hang the resolver.
        let files = extract_all(&[(
            "c.ts",
            "class A extends B {}\nclass B extends A {}\n\
             function go() { const a = new A(); a.missing(); }",
            Language::TypeScript,
        )]);
        let (_, stats) = resolve(&files);
        assert!(stats.unresolved >= 1);
    }

    #[test]
    fn go_methods_resolve_through_their_receiver() {
        let files = extract_all(&[(
            "c.go",
            "package m\ntype Cache struct{}\nfunc (c *Cache) read() {}\n\
             func (c *Cache) load() { c.read() }\n",
            Language::Go,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"load->read".to_string()));
    }

    #[test]
    fn python_self_calls_resolve() {
        let files = extract_all(&[(
            "c.py",
            "class Cache:\n    def read(self):\n        pass\n\
             \n    def load(self):\n        self.read()\n",
            Language::Python,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"load->read".to_string()));
    }

    #[test]
    fn python_inherited_methods_resolve() {
        let files = extract_all(&[(
            "c.py",
            "class Base:\n    def read(self):\n        pass\n\
             \n\nclass Cache(Base):\n    def load(self):\n        self.read()\n",
            Language::Python,
        )]);
        let (edges, _) = resolve(&files);
        assert!(edge_names(&files, &edges, EdgeKind::Calls).contains(&"load->read".to_string()));
    }
}
