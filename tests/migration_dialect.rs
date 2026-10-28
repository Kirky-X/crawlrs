// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Migration dialect consistency tests (spec: R-multidb-schema-001).
//!
//! `migrations/{postgres,sqlite,mysql}/` must cover the same set of tables and
//! columns — they differ only in dialect syntax. The parser below reconstructs
//! the final schema per dialect (CREATE TABLE adds tables/columns,
//! ALTER TABLE ADD COLUMN adds columns, DROP TABLE removes tables) and asserts
//! the three reconstructions are identical, so a drift in one dialect fails
//! here instead of at deploy time.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Final schema state per dialect: table name → set of column names.
type Schema = BTreeMap<String, BTreeSet<String>>;

/// Extract `table -> columns` from one dialect directory.
///
/// Files are processed in lexicographic order (001_…, 002_…, …) so the final
/// state reflects the full evolution, including later ALTERs and DROPs.
fn parse_dialect(dir: &Path) -> Schema {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();

    let mut schema = Schema::new();
    for file in files {
        let sql =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        apply_sql(&mut schema, &sql);
    }
    schema
}

/// Apply one SQL file to the schema state.
fn apply_sql(schema: &mut Schema, sql: &str) {
    let mut statements = Vec::new();
    collect_statements(sql, &mut statements);
    for stmt in statements {
        let trimmed = stmt.trim();
        if let Some(rest) = strip_prefix_ci(trimmed, "CREATE TABLE") {
            if let Some((table, columns)) = parse_create_table(rest) {
                schema.entry(table).or_default().extend(columns);
            }
        } else if let Some(rest) = strip_prefix_ci(trimmed, "ALTER TABLE") {
            apply_alter(schema, rest);
        } else if let Some(rest) = strip_prefix_ci(trimmed, "DROP TABLE") {
            let rest = skip_if_exists(rest);
            if let Some(table) = first_identifier(rest) {
                schema.remove(&table);
            }
        }
    }
}

/// Handle `ALTER TABLE <t> ADD COLUMN [IF NOT EXISTS] <name> <type>` — the
/// only ALTER form used by the migration set.
fn apply_alter(schema: &mut Schema, rest: &str) {
    let Some((table, after_table)) = split_first_identifier(rest) else {
        return;
    };
    let after_table = after_table.trim_start();
    let Some(after_add) = strip_prefix_ci(after_table, "ADD COLUMN") else {
        return;
    };
    let after_add = skip_if_exists(after_add);
    if let Some(column) = first_identifier(after_add) {
        schema.entry(table).or_default().insert(column);
    }
}

/// Parse the parenthesized column list of a CREATE TABLE body.
fn parse_create_table(rest: &str) -> Option<(String, BTreeSet<String>)> {
    let rest = skip_if_exists(rest);
    let open = rest.find('(')?;
    let close = matching_paren(rest, open)?;
    let body = &rest[open + 1..close];
    let table = first_identifier(&rest[..open])?;

    let mut columns = BTreeSet::new();
    for item in split_top_level(body) {
        let item = item.trim();
        if item.is_empty() || is_table_constraint(item) {
            continue;
        }
        if let Some(name) = first_identifier(item) {
            columns.insert(name);
        }
    }
    Some((table, columns))
}

/// Constraint keywords that open a table-level (not column) definition.
fn is_table_constraint(item: &str) -> bool {
    ["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"]
        .iter()
        .any(|kw| strip_prefix_ci(item, kw).is_some())
}

/// Split on top-level commas (parenthesis depth 0), respecting string literals.
fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    for ch in body.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            '(' if !in_single && !in_double => {
                depth += 1;
                current.push(ch);
            }
            ')' if !in_single && !in_double => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 && !in_single && !in_double => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

/// Split SQL text into statements at top-level semicolons, skipping `--`
/// line comments and `$$ … $$` dollar-quoted bodies (PG functions/DO blocks).
fn collect_statements(sql: &str, out: &mut Vec<String>) {
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut in_line_comment = false;
    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                current.push(ch);
            }
            continue;
        }
        if ch == '-' && chars.peek() == Some(&'-') {
            in_line_comment = true;
            continue;
        }
        if ch == ';' {
            out.push(std::mem::take(&mut current));
            continue;
        }
        if ch == '$' {
            // Dollar-quoted body ($$…$$ or $tag$…$tag$): skip entirely —
            // PG-only function/DO bodies carry no cross-dialect schema.
            let mut tag = String::from("$");
            while let Some(&c) = chars.peek() {
                if c == '$' {
                    chars.next();
                    tag.push('$');
                    break;
                }
                if !c.is_ascii_alphanumeric() && c != '_' {
                    break;
                }
                tag.push(c);
                chars.next();
            }
            if tag.ends_with('$') && tag.len() >= 2 {
                skip_dollar_body(&mut chars, &tag);
                continue;
            }
            // Not a dollar quote: fall through as a normal character.
            current.push(ch);
            continue;
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
}

/// Consume characters until the closing `$tag$`.
fn skip_dollar_body(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, tag: &str) {
    let mut window = String::new();
    for ch in chars.by_ref() {
        window.push(ch);
        if window.len() > tag.len() {
            window.remove(0);
        }
        if window == tag {
            return;
        }
    }
}

/// Case-insensitive prefix strip that requires a word boundary after `prefix`.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    if !s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = &s[prefix.len()..];
    // `CREATE TABLEX` must not match `CREATE TABLE`.
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(rest)
}

/// Drop a leading `IF NOT EXISTS` (case-insensitive).
fn skip_if_exists(s: &str) -> &str {
    let trimmed = s.trim_start();
    match strip_prefix_ci(trimmed, "IF NOT EXISTS") {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    }
}

/// First `(identifier)` or `(quoted identifier)` in `s`.
fn first_identifier(s: &str) -> Option<String> {
    let start = s.char_indices().find(|(_, c)| !c.is_whitespace())?.0;
    let rest = &s[start..];
    let first = rest.chars().next()?;
    if first == '"' || first == '`' {
        let close = rest[first.len_utf8()..].find(first)?;
        return Some(rest[first.len_utf8()..first.len_utf8() + close].to_string());
    }
    if !first.is_ascii_alphanumeric() && first != '_' {
        return None;
    }
    let end = first.len_utf8()
        + rest[first.len_utf8()..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len() - first.len_utf8());
    Some(rest[..end].to_string())
}

/// `(head, tail-after-identifier)`.
fn split_first_identifier(s: &str) -> Option<(String, &str)> {
    let start = s.char_indices().find(|(_, c)| !c.is_whitespace())?.0;
    let id = first_identifier(s)?;
    Some((id.clone(), &s[start + id.len()..]))
}

/// Index of the `)` matching the `(` at `open`.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn migrations_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

use std::path::PathBuf;

/// All three dialect directories must exist with at least one migration.
#[test]
fn all_three_dialect_dirs_are_present() {
    for dialect in ["postgres", "sqlite", "mysql"] {
        let dir = migrations_root().join(dialect);
        assert!(
            dir.is_dir(),
            "migrations/{dialect}/ must exist (multi-dialect schema supply)"
        );
        let count = fs::read_dir(&dir)
            .expect("read migrations dir")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
            .count();
        assert!(count > 0, "migrations/{dialect}/ has no .sql files");
    }
}

/// R-multidb-schema-001: the three dialects must cover the same tables and
/// columns after applying the full migration sequence.
#[test]
fn three_dialects_cover_same_tables_and_columns() {
    let postgres = parse_dialect(&migrations_root().join("postgres"));
    let sqlite = parse_dialect(&migrations_root().join("sqlite"));
    let mysql = parse_dialect(&migrations_root().join("mysql"));

    assert!(!postgres.is_empty(), "postgres schema must not be empty");

    for (label, other) in [("sqlite", &sqlite), ("mysql", &mysql)] {
        let missing_tables: Vec<_> = postgres
            .keys()
            .filter(|t| !other.contains_key(*t))
            .collect();
        let extra_tables: Vec<_> = other
            .keys()
            .filter(|t| !postgres.contains_key(*t))
            .collect();
        assert!(
            missing_tables.is_empty() && extra_tables.is_empty(),
            "{label} table set diverges from postgres: missing={missing_tables:?} extra={extra_tables:?}"
        );

        for (table, pg_columns) in &postgres {
            let other_columns = &other[table];
            let missing: Vec<_> = pg_columns.difference(other_columns).collect();
            let extra: Vec<_> = other_columns.difference(pg_columns).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{label}.{table} column set diverges from postgres: missing={missing:?} extra={extra:?}"
            );
        }
    }
}

/// R-multidb-schema-001: SQLite/MySQL DDL must not contain PostgreSQL-only
/// syntax (SERIAL/JSONB/TIMESTAMPTZ per spec; plus the PG-only identifiers
/// that would silently no-op or error on the other engines).
#[test]
fn sqlite_and_mysql_ddl_contain_no_postgres_only_syntax() {
    const FORBIDDEN: &[&str] = &[
        "BIGSERIAL",
        "SERIAL",
        "JSONB",
        "TIMESTAMPTZ",
        "gen_random_uuid",
        "uuid-ossp",
        "plpgsql",
        "::jsonb",
        "NULLS FIRST",
        "NULLS LAST",
        "EXECUTE FUNCTION",
        "DO $$",
        "COMMENT ON",
    ];
    for dialect in ["sqlite", "mysql"] {
        let dir = migrations_root().join(dialect);
        for entry in fs::read_dir(&dir)
            .expect("read migrations dir")
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "sql") {
                continue;
            }
            let sql = fs::read_to_string(&path).expect("read migration file");
            for token in FORBIDDEN {
                // Comment lines may legitimately mention PG syntax when
                // documenting a translation decision; only flag live SQL.
                let offending = sql.lines().find(|line| {
                    let code = line.split("--").next().unwrap_or("");
                    code.contains(token)
                });
                assert!(
                    offending.is_none(),
                    "migrations/{dialect}/{} contains PostgreSQL-only syntax {token:?} in: {offending:?}",
                    path.display()
                );
            }
        }
    }
}
