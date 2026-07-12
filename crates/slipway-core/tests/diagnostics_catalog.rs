//! Drift test for the public diagnostics catalog
//! (`docs/public/api/diagnostics.md`, LLM-ergonomics roadmap Phase 2,
//! LE-H1/LE-M22).
//!
//! Every diagnostic-code string literal in the workspace crate sources that
//! belongs to a cataloged family must have a row in the catalog, and every
//! cataloged code must still exist in the sources. Adding a diagnostic code
//! without documenting it fails `every_diagnostic_code_in_source_is_documented`;
//! deleting or renaming one without updating the doc fails
//! `every_documented_code_still_exists_in_source`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Dotted code families cataloged in `docs/public/api/diagnostics.md`.
const CODE_FAMILIES: [&str; 5] = [
    "view_contract.",
    "backend_input.",
    "event_declaration.",
    "event_equivalence.",
    "probe.",
];

/// Family-less (kebab-case) diagnostic codes cataloged in the doc. These
/// cannot be discovered by prefix, so they are pinned by exact literal.
const KEBAB_CODES: [&str; 4] = [
    "probe-kind-unsupported",
    "resize-unsupported",
    "missing-child-layout",
    "app-font-resolution-refused",
];

fn workspace_crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("crates dir resolves")
}

fn catalog_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("public")
        .join("api")
        .join("diagnostics.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read catalog doc {}: {error}", path.display()))
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Every `src/**/*.rs` file of every workspace crate. Test directories are
/// excluded deliberately: probe metadata ids in fixtures (for example
/// `"probe.list"`) are not diagnostic codes.
fn workspace_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(workspace_crates_dir())
        .expect("list crates dir")
        .flatten()
    {
        let src = entry.path().join("src");
        if src.is_dir() {
            collect_rs_files(&src, &mut files);
        }
    }
    assert!(
        files.len() > 5,
        "workspace source scan looks broken: only {} files found",
        files.len()
    );
    files
}

fn code_suffix_char(character: char) -> bool {
    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
}

/// Extracts family codes that appear in `content` as `opening` + family +
/// suffix + `closing` (a quoted string literal in source, a backticked code
/// span in the doc). The suffix must be non-empty so pure prefix strings
/// like `starts_with("view_contract.")` do not count as codes.
fn extract_family_codes(content: &str, opening: char, closing: char) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for family in CODE_FAMILIES {
        for (index, _) in content.match_indices(family) {
            if content[..index].chars().next_back() != Some(opening) {
                continue;
            }
            let rest = &content[index + family.len()..];
            let suffix: String = rest.chars().take_while(|c| code_suffix_char(*c)).collect();
            if suffix.is_empty() {
                continue;
            }
            if rest[suffix.len()..].chars().next() != Some(closing) {
                continue;
            }
            codes.insert(format!("{family}{suffix}"));
        }
    }
    codes
}

fn source_codes() -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for file in workspace_source_files() {
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
        codes.extend(extract_family_codes(&content, '"', '"'));
        for kebab in KEBAB_CODES {
            if content.contains(&format!("\"{kebab}\"")) {
                codes.insert(kebab.to_string());
            }
        }
    }
    codes
}

#[test]
fn every_diagnostic_code_in_source_is_documented() {
    let doc = catalog_doc();
    let codes = source_codes();
    assert!(
        codes.iter().any(|code| code.starts_with("view_contract.")),
        "source scan found no view_contract codes; the extractor is broken"
    );
    let undocumented: Vec<&String> = codes.iter().filter(|code| !doc.contains(*code)).collect();
    assert!(
        undocumented.is_empty(),
        "diagnostic codes exist in crate sources but have no row in \
         docs/public/api/diagnostics.md: {undocumented:?}"
    );
}

#[test]
fn every_documented_code_still_exists_in_source() {
    let doc = catalog_doc();
    let codes = source_codes();
    let mut documented = extract_family_codes(&doc, '`', '`');
    for kebab in KEBAB_CODES {
        if doc.contains(&format!("`{kebab}`")) {
            documented.insert(kebab.to_string());
        }
    }
    assert!(
        documented.len() >= 60,
        "doc scan found only {} codes; the doc-side extractor is broken",
        documented.len()
    );
    let stale: Vec<&String> = documented
        .iter()
        .filter(|code| !codes.contains(*code))
        .collect();
    assert!(
        stale.is_empty(),
        "docs/public/api/diagnostics.md catalogs codes that no longer exist \
         in any crate source: {stale:?}"
    );
}
