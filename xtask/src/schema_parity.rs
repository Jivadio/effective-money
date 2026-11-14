// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo xtask schema-parity`: checks that the PostgreSQL and SQLite
//! migrations (`crates/storage/migrations/{postgres,sqlite}/`) define the
//! same schema.
//!
//! Both directories must hold the same file names. Replaying each side's
//! files in order gives a schema per backend, and the two must match:
//! - the same tables, with the same columns in the same order;
//! - each column's type mapped to its SQLite equivalent (`UUID`,
//!   `TIMESTAMPTZ` and `DATE` are `TEXT` in SQLite, `BIGINT` is `INTEGER`);
//! - everything after the type (`NOT NULL`, `PRIMARY KEY`, `REFERENCES`,
//!   `CHECK`, `DEFAULT`) and every table constraint, word for word;
//! - the same indexes, each with the same definition (unique flag, table,
//!   columns, partial `WHERE` clause).
//!
//! It parses the files rather than applying them, so it runs without a
//! database. The parser is deliberately strict: a statement it does not
//! understand is an error, never silently skipped (the contract tests in
//! `crates/storage` prove the SQL itself runs on both databases).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, bail};

const MIGRATIONS_DIR: &str = "crates/storage/migrations";

/// PostgreSQL column types and the SQLite type that must mirror them.
const TYPE_MAP: &[(&str, &str)] = &[
    ("UUID", "TEXT"),
    ("TIMESTAMPTZ", "TEXT"),
    ("DATE", "TEXT"),
    ("TEXT", "TEXT"),
    ("BIGINT", "INTEGER"),
    ("INTEGER", "INTEGER"),
    ("BOOLEAN", "INTEGER"),
];

#[derive(Debug, Default, PartialEq)]
struct Schema {
    tables: BTreeMap<String, Table>,
    /// Index name to (table, normalized definition).
    indexes: BTreeMap<String, (String, String)>,
}

#[derive(Debug, Default, PartialEq)]
struct Table {
    columns: Vec<Column>,
    constraints: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct Column {
    name: String,
    ty: String,
    /// Everything after the type, whitespace-normalized.
    rest: String,
}

pub fn run(workspace_root: &Path) -> anyhow::Result<()> {
    let dir = workspace_root.join(MIGRATIONS_DIR);
    let pg_files = migration_files(&dir.join("postgres"))?;
    let sqlite_files = migration_files(&dir.join("sqlite"))?;
    let pg_names: Vec<&String> = pg_files.keys().collect();
    let sqlite_names: Vec<&String> = sqlite_files.keys().collect();
    if pg_names != sqlite_names {
        bail!(
            "migration files differ: postgres has {pg_names:?}, sqlite has {sqlite_names:?} \
             (each migration needs a file with the same name on both sides)"
        );
    }
    let pg = build_schema(pg_files.iter()).context("postgres migrations")?;
    let sqlite = build_schema(sqlite_files.iter()).context("sqlite migrations")?;
    let problems = compare(&pg, &sqlite);
    if problems.is_empty() {
        println!(
            "schema-parity: ok ({} migration(s), {} tables, {} indexes)",
            pg_names.len(),
            pg.tables.len(),
            pg.indexes.len()
        );
        Ok(())
    } else {
        for problem in &problems {
            eprintln!("schema-parity: {problem}");
        }
        bail!(
            "{} schema difference(s) between postgres and sqlite",
            problems.len()
        )
    }
}

/// `*.sql` files of one backend, sorted by name, with their contents.
fn migration_files(dir: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sql") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("non UTF-8 migration file name")?
            .to_owned();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        files.insert(name, text);
    }
    if files.is_empty() {
        bail!("no migrations found in {}", dir.display());
    }
    Ok(files)
}

fn build_schema<'a>(
    files: impl Iterator<Item = (&'a String, &'a String)>,
) -> anyhow::Result<Schema> {
    let mut schema = Schema::default();
    for (name, text) in files {
        apply(&mut schema, text).with_context(|| format!("in {name}"))?;
    }
    Ok(schema)
}

/// Replays one migration file onto `schema`.
fn apply(schema: &mut Schema, text: &str) -> anyhow::Result<()> {
    for statement in strip_comments(text).split(';') {
        let statement = normalize(statement);
        if statement.is_empty() {
            continue;
        }
        let upper = statement.to_ascii_uppercase();
        if let Some(rest) = upper.strip_prefix("CREATE TABLE ") {
            let name_len = rest.find([' ', '(']).unwrap_or(rest.len());
            let name =
                statement["CREATE TABLE ".len().."CREATE TABLE ".len() + name_len].to_owned();
            let open = statement.find('(').context("CREATE TABLE without a body")?;
            let close = statement
                .rfind(')')
                .context("CREATE TABLE without a body")?;
            let table = parse_table(&statement[open + 1..close])
                .with_context(|| format!("table {name}"))?;
            if schema.tables.insert(name.clone(), table).is_some() {
                bail!("table {name} is created twice");
            }
        } else if upper.starts_with("CREATE INDEX ") || upper.starts_with("CREATE UNIQUE INDEX ") {
            let words: Vec<&str> = statement.split(' ').collect();
            let name_at = if upper.starts_with("CREATE UNIQUE") {
                3
            } else {
                2
            };
            let name = words
                .get(name_at)
                .context("index without a name")?
                .to_string();
            let on_at = upper.find(" ON ").context("index without ON")?;
            let table = statement[on_at + 4..]
                .split([' ', '('])
                .next()
                .unwrap_or_default()
                .to_owned();
            if schema
                .indexes
                .insert(name.clone(), (table, statement.clone()))
                .is_some()
            {
                bail!("index {name} is created twice");
            }
        } else if let Some(rest) = upper.strip_prefix("ALTER TABLE ") {
            let table = statement
                ["ALTER TABLE ".len().."ALTER TABLE ".len() + rest.find(' ').unwrap_or(rest.len())]
                .to_owned();
            let add = " ADD COLUMN ";
            let at = upper
                .find(add)
                .context("only ALTER TABLE ... ADD COLUMN is supported")?;
            let column = parse_column(&statement[at + add.len()..])?;
            schema
                .tables
                .get_mut(&table)
                .with_context(|| format!("ALTER TABLE on unknown table {table}"))?
                .columns
                .push(column);
        } else if let Some(rest) = upper.strip_prefix("DROP INDEX ") {
            let name = &statement[statement.len() - rest.len()..];
            let name = name.trim_start_matches("IF EXISTS ").trim();
            schema.indexes.remove(name);
        } else {
            let head: String = statement.chars().take(40).collect();
            bail!("unsupported statement `{head}...` (extend xtask schema-parity)");
        }
    }
    Ok(())
}

fn parse_table(body: &str) -> anyhow::Result<Table> {
    let mut table = Table::default();
    for item in split_top_level(body) {
        let upper = item.to_ascii_uppercase();
        let is_constraint = [
            "PRIMARY KEY",
            "UNIQUE",
            "CHECK",
            "FOREIGN KEY",
            "CONSTRAINT",
        ]
        .iter()
        .any(|keyword| upper.starts_with(keyword));
        if is_constraint {
            table.constraints.push(item);
        } else {
            table.columns.push(parse_column(&item)?);
        }
    }
    Ok(table)
}

fn parse_column(item: &str) -> anyhow::Result<Column> {
    let mut words = item.splitn(3, ' ');
    let name = words.next().unwrap_or_default().to_owned();
    let ty = words
        .next()
        .with_context(|| format!("column {name} has no type"))?
        .to_ascii_uppercase();
    let rest = words.next().unwrap_or_default().to_owned();
    Ok(Column { name, ty, rest })
}

/// Splits a table body on commas outside parentheses and quotes.
fn split_top_level(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut current = String::new();
    for c in body.chars() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                items.push(normalize(&current));
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    items.push(normalize(&current));
    items.retain(|item| !item.is_empty());
    items
}

/// Drops `-- ...` comments (outside quotes).
fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut quoted = false;
        let mut cut = line.len();
        let bytes = line.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\'' {
                quoted = !quoted;
            } else if !quoted && b == b'-' && bytes.get(i + 1) == Some(&b'-') {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// Collapses whitespace, and removes it just inside parentheses.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace(" )", ")")
}

/// Every difference between the two schemas, as readable lines.
fn compare(pg: &Schema, sqlite: &Schema) -> Vec<String> {
    let mut problems = Vec::new();
    for name in pg.tables.keys() {
        if !sqlite.tables.contains_key(name) {
            problems.push(format!("table {name} is missing in sqlite"));
        }
    }
    for name in sqlite.tables.keys() {
        if !pg.tables.contains_key(name) {
            problems.push(format!("table {name} is missing in postgres"));
        }
    }
    for (name, pg_table) in &pg.tables {
        let Some(sqlite_table) = sqlite.tables.get(name) else {
            continue;
        };
        compare_table(name, pg_table, sqlite_table, &mut problems);
    }
    for (name, (table, definition)) in &pg.indexes {
        match sqlite.indexes.get(name) {
            None => problems.push(format!("index {name} (on {table}) is missing in sqlite")),
            Some((_, other)) if other != definition => problems.push(format!(
                "index {name} differs:\n  postgres: {definition}\n  sqlite:   {other}"
            )),
            Some(_) => {}
        }
    }
    for (name, (table, _)) in &sqlite.indexes {
        if !pg.indexes.contains_key(name) {
            problems.push(format!("index {name} (on {table}) is missing in postgres"));
        }
    }
    problems
}

fn compare_table(name: &str, pg: &Table, sqlite: &Table, problems: &mut Vec<String>) {
    let pg_names: Vec<&str> = pg.columns.iter().map(|c| c.name.as_str()).collect();
    let sqlite_names: Vec<&str> = sqlite.columns.iter().map(|c| c.name.as_str()).collect();
    for column in &pg_names {
        if !sqlite_names.contains(column) {
            problems.push(format!("column {name}.{column} is missing in sqlite"));
        }
    }
    for column in &sqlite_names {
        if !pg_names.contains(column) {
            problems.push(format!("column {name}.{column} is missing in postgres"));
        }
    }
    let common_pg: Vec<&str> = pg_names
        .iter()
        .copied()
        .filter(|c| sqlite_names.contains(c))
        .collect();
    let common_sqlite: Vec<&str> = sqlite_names
        .iter()
        .copied()
        .filter(|c| pg_names.contains(c))
        .collect();
    if common_pg != common_sqlite {
        problems.push(format!(
            "columns of {name} are in a different order: postgres {common_pg:?}, sqlite {common_sqlite:?}"
        ));
    }
    for pg_column in &pg.columns {
        let Some(sqlite_column) = sqlite.columns.iter().find(|c| c.name == pg_column.name) else {
            continue;
        };
        let column = format!("{name}.{}", pg_column.name);
        match TYPE_MAP.iter().find(|(pg_ty, _)| *pg_ty == pg_column.ty) {
            None => problems.push(format!(
                "column {column}: postgres type {} has no known SQLite mirror (extend TYPE_MAP)",
                pg_column.ty
            )),
            Some((_, expected)) if *expected != sqlite_column.ty => problems.push(format!(
                "column {column}: postgres {} must be {expected} in sqlite, found {}",
                pg_column.ty, sqlite_column.ty
            )),
            Some(_) => {}
        }
        if pg_column.rest != sqlite_column.rest {
            problems.push(format!(
                "column {column} differs:\n  postgres: {}\n  sqlite:   {}",
                pg_column.rest, sqlite_column.rest
            ));
        }
    }
    if pg.constraints != sqlite.constraints {
        problems.push(format!(
            "table constraints of {name} differ:\n  postgres: {:?}\n  sqlite:   {:?}",
            pg.constraints, sqlite.constraints
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: &str = "
        -- comment with a ; inside
        CREATE TABLE things (
            id UUID PRIMARY KEY,
            owner_id UUID NOT NULL REFERENCES owners (id) ON DELETE CASCADE,
            amount_minor BIGINT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('a', 'b')),
            booked_on DATE,
            PRIMARY KEY (id, owner_id)
        );
        CREATE UNIQUE INDEX things_key ON things (owner_id, kind) WHERE booked_on IS NOT NULL;
    ";

    fn sqlite_from(pg: &str) -> String {
        pg.replace("UUID", "TEXT")
            .replace("BIGINT", "INTEGER")
            .replace("DATE,", "TEXT,")
    }

    fn diff(pg: &str, sqlite: &str) -> Vec<String> {
        let mut pg_schema = Schema::default();
        apply(&mut pg_schema, pg).expect("pg parses");
        let mut sqlite_schema = Schema::default();
        apply(&mut sqlite_schema, sqlite).expect("sqlite parses");
        compare(&pg_schema, &sqlite_schema)
    }

    #[test]
    fn mirrored_schemas_match() {
        assert_eq!(diff(PG, &sqlite_from(PG)), Vec::<String>::new());
    }

    #[test]
    fn missing_column_is_caught() {
        let sqlite = sqlite_from(PG).replace("amount_minor INTEGER NOT NULL,", "");
        let problems = diff(PG, &sqlite);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("things.amount_minor is missing in sqlite")),
            "{problems:?}"
        );
    }

    #[test]
    fn type_constraint_and_index_drift_is_caught() {
        let wrong_type = sqlite_from(PG).replace("amount_minor INTEGER", "amount_minor TEXT");
        assert!(
            diff(PG, &wrong_type)
                .iter()
                .any(|p| p.contains("must be INTEGER"))
        );

        let wrong_check = sqlite_from(PG).replace("('a', 'b')", "('a')");
        assert!(
            diff(PG, &wrong_check)
                .iter()
                .any(|p| p.contains("things.kind differs"))
        );

        let not_partial = sqlite_from(PG).replace(" WHERE booked_on IS NOT NULL", "");
        assert!(
            diff(PG, &not_partial)
                .iter()
                .any(|p| p.contains("index things_key differs"))
        );

        let no_index = sqlite_from(PG).replace(
            "CREATE UNIQUE INDEX things_key ON things (owner_id, kind) WHERE booked_on IS NOT NULL;",
            "",
        );
        assert!(
            diff(PG, &no_index)
                .iter()
                .any(|p| p.contains("missing in sqlite"))
        );
    }

    #[test]
    fn alter_table_add_column_is_replayed() {
        let pg = format!("{PG} ALTER TABLE things ADD COLUMN note TEXT;");
        let sqlite = sqlite_from(PG);
        assert!(
            diff(&pg, &sqlite)
                .iter()
                .any(|p| p.contains("things.note is missing in sqlite"))
        );
    }

    #[test]
    fn unknown_statements_are_errors() {
        let mut schema = Schema::default();
        assert!(apply(&mut schema, "CREATE VIEW v AS SELECT 1;").is_err());
    }

    #[test]
    fn repository_migrations_match() {
        run(crate::check_deps::workspace_root()).expect("storage migrations are in parity");
    }
}
