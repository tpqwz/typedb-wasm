use std::path::Path;
use std::sync::Arc;

use wasm_bindgen::prelude::*;

use answer::variable_value::VariableValue;
use answer::{Concept, Thing, Type};
use compiler::VariablePosition;
use concept::thing::thing_manager::ThingManager;
use concept::type_::type_manager::TypeManager;
use concept::type_::TypeAPI;
use database::query::{WriteQueryAnswer, execute_schema_query, execute_write_query_in_write};
use database::transaction::{CommitIntent, TransactionRead, TransactionSchema, TransactionWrite};
use database::Database;
use diagnostics::diagnostics_manager::DiagnosticsManager;
use durability::vfs;
use executor::batch::Batch;
use executor::document::{ConceptDocument, DocumentLeaf, DocumentMap, DocumentNode};
use executor::pipeline::stage::StageIterator;
use executor::ExecutionInterrupt;
use ir::pipeline::ParameterRegistry;
use itertools::Either;
use options::byte_size::ByteSize;
use options::{MvccCleanupStrategy, QueryOptions, TransactionOptions};
use query::given_rows::GivenRowsSimple;
use resource::profile::StorageCounters;
use storage::durability_client::WALClient;
use storage::keyspace::rocks_resources::RocksResources;
use storage::snapshot::ReadableSnapshot;
use typeql::query::pipeline::stage::Stage;
use typeql::query::QueryStructure;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(msg: &str);
}

fn install_panic_hook() {
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| {
        error(&info.to_string());
    }));
}

/// Reads the wasm clock. `unix_ms` is wall time. `elapsed_ms` is the gap between two instants.
#[wasm_bindgen]
pub fn clock() -> String {
    install_panic_hook();
    let start = std::time::Instant::now();
    let unix_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let elapsed_ms = start.elapsed().as_millis();
    format!("unix_ms={unix_ms} elapsed_ms={elapsed_ms}")
}

/// Opens a database through the real TypeDB database crate.
/// The keyspace and the write-ahead log are both in memory.
#[wasm_bindgen]
pub fn probe(name: &str) -> String {
    match open(name) {
        Ok(_) => "open".to_string(),
        Err(error) => error,
    }
}

/// Runs one TypeQL query, or several separated by `end;`.
/// Schema answers are `ok`. Rows are `$name=value` lines.
/// A failure starts with `error` and stops the rest of the script.
#[wasm_bindgen]
pub fn query(name: &str, typeql_query: &str) -> String {
    match run_script(name, typeql_query) {
        Ok(answer) => answer,
        Err(error) => format!("error\n{error}"),
    }
}

/// Defines an entity, inserts one, and counts it back.
#[wasm_bindgen]
pub fn roundtrip() -> String {
    match define_insert_match() {
        Ok(count) => count.to_string(),
        Err(error) => error,
    }
}

/// Bytes of one database's log. Empty if that database has not been created.
#[wasm_bindgen]
pub fn save(name: &str) -> Vec<u8> {
    vfs::export_tree(&Path::new("/typedb").join(name))
}

/// How many `page` entities a reopened database still has.
#[wasm_bindgen]
pub fn count(name: &str) -> String {
    match count_pages(name) {
        Ok(count) => count.to_string(),
        Err(error) => error,
    }
}

/// Puts a previously saved log back. Opening the same name after this replays it.
#[wasm_bindgen]
pub fn restore(bytes: &[u8]) -> String {
    match vfs::import_tree(bytes) {
        Ok(()) => "restored".to_string(),
        Err(error) => error.to_string(),
    }
}

fn open(name: &str) -> Result<Arc<Database<WALClient>>, String> {
    install_panic_hook();
    let diagnostics = DiagnosticsManager::new_disabled();
    let rocks = RocksResources::new(ByteSize::mb(8), ByteSize::mb(8));
    let path = Path::new("/typedb").join(name);
    Database::<WALClient>::open(&path, &diagnostics, &rocks, MvccCleanupStrategy::Disabled)
        .map(Arc::new)
        .map_err(|error| format!("{error:?}"))
}

fn define_insert_match() -> Result<usize, String> {
    let defined = run_query("round", "define entity page;")?;
    if defined != "ok" {
        return Err(defined);
    }
    run_query("round", "insert $p isa page;")?;
    let matched = run_query("round", "match $p isa page;")?;
    Ok(matched.lines().filter(|line| !line.is_empty()).count())
}

fn count_pages(name: &str) -> Result<usize, String> {
    let matched = run_query(name, "match $p isa page;")?;
    Ok(matched.lines().filter(|line| !line.is_empty()).count())
}

fn run_script(name: &str, source: &str) -> Result<String, String> {
    let mut rest = source.trim_start();
    if rest.is_empty() {
        return Err("no query".to_string());
    }
    let mut answers = Vec::new();
    while !rest.trim().is_empty() {
        let (_query, end) = match typeql::parse_query_from(rest) {
            Ok(parsed) => parsed,
            Err(error) => {
                answers.push(format!("error\n{error:?}"));
                break;
            }
        };
        if end == 0 {
            return Err("could not split queries".to_string());
        }
        match run_query(name, &rest[..end]) {
            Ok(answer) => answers.push(answer),
            Err(error) => {
                answers.push(format!("error\n{error}"));
                break;
            }
        }
        rest = rest[end..].trim_start();
    }
    Ok(answers.join("\n"))
}

fn run_query(name: &str, source: &str) -> Result<String, String> {
    let database = open(name)?;
    let parsed = typeql::parse_query(source).map_err(|error| format!("{error:?}"))?.into_structure();
    match parsed {
        QueryStructure::Schema(schema) => {
            let tx = TransactionSchema::open(database, TransactionOptions::default()).map_err(|error| format!("{error:?}"))?;
            let (tx, result) = execute_schema_query(tx, schema, source.to_string());
            result.map_err(|error| format!("{error:?}"))?;
            let (mut profile, intent) = tx.finalise();
            intent.map_err(|error| format!("{error:?}"))?.commit(profile.commit_profile()).map_err(|error| format!("{error:?}"))?;
            Ok("ok".to_string())
        }
        QueryStructure::Pipeline(pipeline) => {
            if is_write(&pipeline) {
                let tx = TransactionWrite::open(database, TransactionOptions::default()).map_err(|error| format!("{error:?}"))?;
                let (tx, result) = execute_write_query_in_write(
                    tx,
                    QueryOptions::default_grpc(),
                    Arc::new(pipeline),
                    None::<GivenRowsSimple>,
                    source.to_string(),
                    ExecutionInterrupt::new_uninterruptible(),
                );
                let answer = result.map_err(|error| format!("{error:?}"))?;
                let text = format_write(answer, tx.snapshot.as_ref(), tx.thing_manager.as_ref(), tx.type_manager.as_ref());
                let (mut profile, intent) = tx.finalise();
                intent.map_err(|error| format!("{error:?}"))?.commit(profile.commit_profile()).map_err(|error| format!("{error:?}"))?;
                Ok(text)
            } else {
                let tx = TransactionRead::open(database, TransactionOptions::default()).map_err(|error| format!("{error:?}"))?;
                let prepared = tx
                    .query_manager
                    .prepare_read_pipeline(
                        tx.snapshot.clone(),
                        &tx.type_manager,
                        tx.thing_manager.clone(),
                        tx.function_manager.clone(),
                        &pipeline,
                        None::<GivenRowsSimple>,
                        source,
                    )
                    .map_err(|error| format!("{error:?}"))?;
                let positions = prepared.rows_positions().cloned().unwrap_or_default();
                if prepared.has_fetch() {
                    let (iterator, context) = prepared
                        .into_documents_iterator(ExecutionInterrupt::new_uninterruptible())
                        .map_err(|error| format!("{error:?}"))?;
                    let mut lines = Vec::new();
                    for document in iterator {
                        let document = document.map_err(|error| format!("{error:?}"))?;
                        lines.push(format_document(
                            &document,
                            &context.parameters,
                            tx.snapshot.as_ref(),
                            tx.thing_manager.as_ref(),
                            tx.type_manager.as_ref(),
                        ));
                    }
                    Ok(lines.join("\n"))
                } else {
                    let (iterator, _context) = prepared
                        .into_rows_iterator(ExecutionInterrupt::new_uninterruptible())
                        .map_err(|error| format!("{error:?}"))?;
                    let batch = iterator.collect_owned().map_err(|error| format!("{error:?}"))?;
                    Ok(format_rows(positions, &batch, tx.snapshot.as_ref(), tx.thing_manager.as_ref(), tx.type_manager.as_ref()))
                }
            }
        }
    }
}

fn is_write(pipeline: &typeql::query::Pipeline) -> bool {
    pipeline.stages.iter().any(|stage| {
        matches!(stage, Stage::Insert(_) | Stage::Put(_) | Stage::Delete(_) | Stage::Update(_))
    })
}

fn format_write<S: ReadableSnapshot>(
    answer: WriteQueryAnswer,
    snapshot: &S,
    things: &ThingManager,
    types: &TypeManager,
) -> String {
    match answer.answer {
        Either::Left((positions, batch, _)) => format_rows(positions, &batch, snapshot, things, types),
        Either::Right((parameters, documents)) => documents
            .iter()
            .map(|document| format_document(document, &parameters, snapshot, things, types))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn format_rows<S: ReadableSnapshot>(
    positions: impl IntoIterator<Item = (String, VariablePosition)>,
    batch: &Batch,
    snapshot: &S,
    things: &ThingManager,
    types: &TypeManager,
) -> String {
    let mut columns: Vec<_> = positions.into_iter().collect();
    columns.sort_by_key(|(_, position)| position.as_usize());
    let mut lines = Vec::with_capacity(batch.len());
    for row in batch.iter() {
        let cells: Vec<_> = columns
            .iter()
            .map(|(name, position)| format!("${name}={}", show_value(row.get(*position), snapshot, things, types)))
            .collect();
        lines.push(cells.join(" "));
    }
    lines.join("\n")
}

fn show_value<S: ReadableSnapshot>(value: &VariableValue, snapshot: &S, things: &ThingManager, types: &TypeManager) -> String {
    match value {
        VariableValue::Thing(thing) => show_thing(thing, snapshot, things, types),
        VariableValue::ThingList(list) => {
            let items: Vec<_> = list.iter().map(|thing| show_thing(thing, snapshot, things, types)).collect();
            format!("[{}]", items.join(", "))
        }
        VariableValue::Type(type_) => show_type(type_, snapshot, types),
        other => other.to_string(),
    }
}

fn show_thing<S: ReadableSnapshot>(thing: &Thing, snapshot: &S, things: &ThingManager, types: &TypeManager) -> String {
    let name = show_type(&thing.type_(), snapshot, types);
    match thing {
        Thing::Attribute(attribute) => match attribute.get_value(snapshot, things, StorageCounters::DISABLED) {
            Ok(value) => format!("{name} {value}"),
            Err(_) => format!("{name} {attribute}"),
        },
        other => format!("{name} {other}"),
    }
}

fn show_type<S: ReadableSnapshot>(type_: &Type, snapshot: &S, types: &TypeManager) -> String {
    let label = match type_ {
        Type::Entity(type_) => type_.get_label(snapshot, types),
        Type::Relation(type_) => type_.get_label(snapshot, types),
        Type::Attribute(type_) => type_.get_label(snapshot, types),
        Type::RoleType(type_) => type_.get_label(snapshot, types),
    };
    match label {
        Ok(label) => format!("{}", &*label),
        Err(_) => type_.to_string(),
    }
}

fn format_document<S: ReadableSnapshot>(
    document: &ConceptDocument,
    parameters: &ParameterRegistry,
    snapshot: &S,
    things: &ThingManager,
    types: &TypeManager,
) -> String {
    format_node(&document.root, parameters, snapshot, things, types)
}

fn format_node<S: ReadableSnapshot>(
    node: &DocumentNode,
    parameters: &ParameterRegistry,
    snapshot: &S,
    things: &ThingManager,
    types: &TypeManager,
) -> String {
    match node {
        DocumentNode::List(list) => {
            let items: Vec<_> = list.list.iter().map(|node| format_node(node, parameters, snapshot, things, types)).collect();
            format!("[{}]", items.join(", "))
        }
        DocumentNode::Map(DocumentMap::UserKeys(map)) => {
            let mut entries: Vec<_> = map
                .iter()
                .map(|(id, node)| {
                    let key = parameters.fetch_key(id).map(String::as_str).unwrap_or("?");
                    (key.to_string(), format_node(node, parameters, snapshot, things, types))
                })
                .collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let body: Vec<_> = entries.iter().map(|(key, value)| format!("\"{key}\": {value}")).collect();
            format!("{{{}}}", body.join(", "))
        }
        DocumentNode::Map(DocumentMap::GeneratedKeys(map)) => {
            let mut entries: Vec<_> = map
                .iter()
                .map(|(key, node)| (key.to_string(), format_node(node, parameters, snapshot, things, types)))
                .collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let body: Vec<_> = entries.iter().map(|(key, value)| format!("\"{key}\": {value}")).collect();
            format!("{{{}}}", body.join(", "))
        }
        DocumentNode::Leaf(DocumentLeaf::Empty) => "empty".to_string(),
        DocumentNode::Leaf(DocumentLeaf::Kind(kind)) => kind.to_string(),
        DocumentNode::Leaf(DocumentLeaf::Concept(concept)) => show_concept(concept, snapshot, things, types),
    }
}

fn show_concept<S: ReadableSnapshot>(concept: &Concept, snapshot: &S, things: &ThingManager, types: &TypeManager) -> String {
    match concept {
        Concept::Thing(thing) => show_thing(thing, snapshot, things, types),
        Concept::Type(type_) => show_type(type_, snapshot, types),
        Concept::Value(value) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use durability::vfs;

    use super::{clock, count_pages, probe, query, restore, roundtrip, save};

    fn empty_log() {
        vfs::clear_tree(Path::new("/typedb"));
    }

    fn memory_kv_files(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        fn walk(path: &Path, found: &mut Vec<PathBuf>) {
            let Ok(children) = vfs::read_dir(path) else { return };
            for child in children {
                if child.file_name().and_then(|name| name.to_str()) == Some("memory.kv") {
                    found.push(child);
                } else {
                    walk(&child, found);
                }
            }
        }
        walk(root, &mut found);
        found
    }

    #[test]
    fn the_clock_moves() {
        let reading = clock();
        assert!(reading.starts_with("unix_ms="), "{reading}");
        let millis: u128 = reading.split("unix_ms=").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(millis > 1_700_000_000_000, "{reading}");
    }

    #[test]
    fn a_database_opens_without_a_filesystem() {
        empty_log();
        let opened = probe("probe");
        assert_eq!(opened, "open", "{opened}");
    }

    #[test]
    fn define_insert_and_match_round_trip() {
        empty_log();
        let count = roundtrip();
        assert_eq!(count, "1", "{count}");
    }

    #[test]
    fn the_page_survives_a_saved_and_reloaded_log() {
        empty_log();
        let count = roundtrip();
        assert_eq!(count, "1", "{count}");
        let root = Path::new("/typedb/round");
        let saved = vfs::export_tree(root);
        assert!(!saved.is_empty(), "the log snapshot was empty");
        vfs::clear_tree(root);
        vfs::import_tree(&saved).expect("snapshot import");
        let again = count_pages("round").unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(again, 1);
    }

    #[test]
    fn a_reopen_uses_the_checkpoint_instead_of_the_whole_log() {
        empty_log();
        assert_eq!(roundtrip(), "1");
        assert_eq!(count_pages("round").unwrap(), 1);
        let root = Path::new("/typedb/round");
        let snapshots = memory_kv_files(root);
        assert!(snapshots.iter().any(|path| vfs::len_or_zero(path) > 0), "the checkpoint had no keyspace data");
        let saved = vfs::export_tree(root);
        vfs::clear_tree(root);
        vfs::import_tree(&saved).expect("snapshot import");
        let snapshots = memory_kv_files(root);
        assert!(!snapshots.is_empty(), "the saved log did not contain a checkpoint");
        for path in &snapshots {
            vfs::write_file(path, &[]).expect("wipe checkpoint");
        }
        let again = count_pages("round");
        assert!(again.as_ref().ok() != Some(&1), "reopen replayed the log instead of the checkpoint: {again:?}");
    }

    #[test]
    fn a_caller_can_run_its_own_typeql() {
        empty_log();
        let defined = query("notes", "define attribute title, value string; entity page, owns title;");
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query("notes", r#"insert $p isa page, has title "Hyperbase";"#);
        assert!(inserted.starts_with("$p="), "{inserted}");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(matched.contains("page"), "{matched}");
        assert!(matched.contains("\"Hyperbase\""), "{matched}");
        let again = query("notes", r#"insert $q isa page, has title "Notes";"#);
        assert!(again.starts_with("$q="), "{again}");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(matched.contains("\"Notes\""), "{matched}");
        assert_eq!(matched.lines().filter(|line| !line.is_empty()).count(), 2, "{matched}");
    }

    #[test]
    fn a_relation_links_two_pages() {
        empty_log();
        let defined = query(
            "notes",
            r#"define
attribute title, value string;
entity page, owns title, plays link:parent, plays link:child;
relation link, relates parent, relates child;"#,
        );
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query(
            "notes",
            r#"insert
$a isa page, has title "A";
$b isa page, has title "B";
link (parent: $a, child: $b);"#,
        );
        assert!(!inserted.starts_with("error"), "{inserted}");
        assert!(inserted.contains("$a="), "{inserted}");
        assert!(inserted.contains("$b="), "{inserted}");
        let matched = query(
            "notes",
            r#"match
$a isa page, has title $ta;
$b isa page, has title $tb;
link (parent: $a, child: $b);"#,
        );
        assert!(matched.contains("\"A\""), "{matched}");
        assert!(matched.contains("\"B\""), "{matched}");
        assert!(matched.contains("page"), "{matched}");
        let links = query("notes", "match $l isa link;");
        assert!(links.contains("link"), "{links}");
        assert_eq!(matched.lines().filter(|line| !line.is_empty()).count(), 1, "{matched}");
    }

    #[test]
    fn a_deleted_page_stays_deleted_after_restore() {
        empty_log();
        let defined = query("notes", "define attribute title, value string; entity page, owns title;");
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query("notes", r#"insert $a isa page, has title "A"; $b isa page, has title "B";"#);
        assert!(!inserted.starts_with("error"), "{inserted}");
        let deleted = query("notes", r#"match $p isa page, has title "A"; delete $p;"#);
        assert!(!deleted.starts_with("error"), "{deleted}");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(!matched.contains("\"A\""), "{matched}");
        assert!(matched.contains("\"B\""), "{matched}");
        assert_eq!(matched.lines().filter(|line| !line.is_empty()).count(), 1, "{matched}");
        let bytes = save("notes");
        empty_log();
        assert_eq!(restore(&bytes), "restored");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(!matched.contains("\"A\""), "{matched}");
        assert!(matched.contains("\"B\""), "{matched}");
    }

    #[test]
    fn an_updated_title_stays_updated_after_restore() {
        empty_log();
        let defined = query("notes", "define attribute title, value string; entity page, owns title @card(1);");
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query("notes", r#"insert $p isa page, has title "A";"#);
        assert!(!inserted.starts_with("error"), "{inserted}");
        let updated = query("notes", r#"match $p isa page, has title $t; $t == "A"; update $p has title "B";"#);
        assert!(!updated.starts_with("error"), "{updated}");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(matched.contains("\"B\""), "{matched}");
        assert!(!matched.contains("\"A\""), "{matched}");
        assert_eq!(matched.lines().filter(|line| !line.is_empty()).count(), 1, "{matched}");
        let bytes = save("notes");
        empty_log();
        assert_eq!(restore(&bytes), "restored");
        let matched = query("notes", "match $p isa page, has title $t;");
        assert!(matched.contains("\"B\""), "{matched}");
        assert!(!matched.contains("\"A\""), "{matched}");
    }

    #[test]
    fn reduce_counts_pages_and_fetch_returns_the_title() {
        empty_log();
        let defined = query("notes", "define attribute title, value string; entity page, owns title;");
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query("notes", r#"insert $a isa page, has title "A"; $b isa page, has title "B";"#);
        assert!(!inserted.starts_with("error"), "{inserted}");
        let counted = query("notes", "match $p isa page; reduce $n = count;");
        assert!(counted.contains('2'), "{counted}");
        let fetched = query("notes", r#"match $p isa page, has title $t; $t == "A"; fetch { "title": $t };"#);
        assert!(fetched.contains("\"title\""), "{fetched}");
        assert!(fetched.contains("\"A\""), "{fetched}");
        assert!(!fetched.contains("\"B\""), "{fetched}");
    }

    #[test]
    fn a_function_returns_the_title() {
        empty_log();
        let defined = query(
            "notes",
            r#"define
attribute title, value string;
entity page, owns title;
fun page_title($p: page) -> title:
    match $p has title $t;
    return first $t;"#,
        );
        assert_eq!(defined, "ok", "{defined}");
        let inserted = query("notes", r#"insert $p isa page, has title "A";"#);
        assert!(!inserted.starts_with("error"), "{inserted}");
        let called = query("notes", "match $p isa page; let $name = page_title($p);");
        assert!(called.contains("\"A\""), "{called}");
    }

    #[test]
    fn a_script_runs_until_the_first_error() {
        empty_log();
        let script = r#"define
attribute title, value string;
entity page, owns title;
end;

insert $p isa page, has title "A";
end;

match $p isa page, has title $t;"#;
        let answer = query("notes", script);
        assert!(answer.contains("ok"), "{answer}");
        assert!(answer.contains("title \"A\""), "{answer}");
        assert!(!answer.starts_with("error"), "{answer}");

        let stopped = query("notes", "insert $q isa page, has title \"B\";\nend;\nnot typeql;\nend;\nmatch $p isa page;");
        assert!(!stopped.starts_with("error"), "{stopped}");
        assert!(stopped.contains("error"), "{stopped}");
        assert!(stopped.contains("$q="), "{stopped}");
        let still = query("notes", "match $p isa page, has title $t;");
        assert!(still.contains("title \"A\""), "{still}");
        assert!(still.contains("title \"B\""), "{still}");
    }
}
