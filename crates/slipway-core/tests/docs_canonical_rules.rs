//! Docs-lint guard for the Phase-2 canonical-rule designation
//! (LLM-ergonomics roadmap Phase 2 item LE-M5 / Phase 5 item 2, Step 214).
//!
//! `docs/public/llm-contract-checklist.md` is the CANONICAL statement of the
//! cross-cutting authoring rules; every other public page carries at most a
//! one-line restatement plus a deference link. This test makes that
//! designation enforceable in the default suite:
//!
//! * the checklist must keep its canonical self-designation and the anchor
//!   sentence of each guarded rule (editing the canonical wording forces a
//!   deliberate update HERE, in the same change);
//! * each known non-canonical restatement site must keep its pinned one-line
//!   summary and (where designated) its link to the checklist — editing a
//!   copy divergently, without touching the canonical file, fails;
//! * contract-bearing phrases may only appear on the pages registered for
//!   them, so a NEW page restating a guarded rule fails until it is
//!   registered here with its own pinned summary.
//!
//! Maintenance: when a rule's canonical wording changes, update the checklist
//! AND the pinned copies AND this table in one commit. That is the point.

use std::fs;
use std::path::{Path, PathBuf};

const CANONICAL: &str = "llm-contract-checklist.md";

fn docs_public_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("public")
}

fn read_page(relative: &str) -> String {
    let path = docs_public_dir().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .replace("\r\n", "\n")
}

/// Anchor sentences the CANONICAL file must keep, per rule.
const CANONICAL_ANCHORS: &[(&str, &str)] = &[
    (
        "canonical self-designation",
        "This file is the CANONICAL statement of the cross-cutting authoring rules",
    ),
    (
        "facade/prelude rule",
        "Do not start with `use slipway::*`. The facade root exposes low-level extension",
    ),
    ("app-shape rule", "## Required App Shape"),
    (
        "declaration rule",
        "Painting a shape or text is never enough to make it interactive.",
    ),
    (
        "scroll inducement rule",
        "ask the scroll question explicitly: does any content",
    ),
    (
        "style rule",
        "Backend theme/defaults are not Slipway style authority.",
    ),
    (
        "backend-input-proof rule",
        "Do not construct or import `BackendInputEvent` in ordinary app authoring.",
    ),
    (
        "admission-refusal routing rule",
        "an admission refusal is not a doc gap",
    ),
    (
        "gap landing-zone rule",
        "records every gap in a `GAPS.md` file at its own project root",
    ),
];

/// The five role files of the canonical app shape, pinned wherever the shape
/// is restated so a divergent copy (renamed/absent role) fails.
const APP_SHAPE_FILES: [&str; 5] = [
    "ssot.rs",
    "internal_logic.rs",
    "communication.rs",
    "view.rs",
    "app_runner.rs",
];

struct PinnedCopy {
    page: &'static str,
    rule: &'static str,
    /// Exact sentence fragments this non-canonical copy must keep.
    pinned: &'static [&'static str],
    /// Whether the page must carry a deference link to the checklist.
    must_link_canonical: bool,
}

const PINNED_COPIES: &[PinnedCopy] = &[
    PinnedCopy {
        page: "api/core.md",
        rule: "facade/prelude rule",
        pinned: &[
            "Do not use `use slipway::*` as the ordinary authoring surface.",
            "(the canonical\nstatement of this rule)",
        ],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "api/core.md",
        rule: "backend-input-proof rule",
        pinned: &["`BackendInputEvent` is intentionally not part of `slipway::prelude::*`."],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "llm-entry.md",
        rule: "facade/prelude rule",
        pinned: &["For ordinary app authoring, import `slipway::prelude::*`. Do not use"],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "llm-entry.md",
        rule: "backend-input-proof rule",
        pinned: &["Do not import or construct `BackendInputEvent` in ordinary app authoring."],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "llm-entry.md",
        rule: "app-shape rule",
        pinned: &["## Required Output Shape"],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "quickstart-authoring.md",
        rule: "facade/prelude rule",
        pinned: &["use slipway::prelude::*;"],
        must_link_canonical: false,
    },
    PinnedCopy {
        page: "quickstart-authoring.md",
        rule: "declaration rule",
        pinned: &["A painted shape is not interactive by itself."],
        must_link_canonical: false,
    },
    PinnedCopy {
        page: "quickstart-authoring.md",
        rule: "scroll inducement rule",
        pinned: &["does any content exceed its container or the window?"],
        must_link_canonical: false,
    },
    PinnedCopy {
        page: "api/README.md",
        rule: "facade/prelude rule",
        pinned: &["LLM workers should read [LLM contract checklist](../llm-contract-checklist.md)"],
        must_link_canonical: true,
    },
    PinnedCopy {
        page: "llm-entry.md",
        rule: "gap landing-zone rule",
        pinned: &["`GAPS.md` at your own project root"],
        must_link_canonical: true,
    },
];

/// Contract-bearing phrases and the ONLY public pages allowed to carry them.
/// A new page restating one of these rules fails until it is registered here
/// (and pinned above if it restates the rule).
const PHRASE_ALLOWLIST: &[(&str, &[&str])] = &[
    (
        // facade-root rule statements
        "slipway::*",
        &["llm-contract-checklist.md", "llm-entry.md", "api/core.md"],
    ),
    (
        // backend-input-proof rule statements
        "BackendInputEvent",
        &[
            "llm-contract-checklist.md",
            "llm-entry.md",
            "api/core.md",
            "api/backends.md",
        ],
    ),
    (
        // style-authority rule statements
        "style authority",
        &["llm-contract-checklist.md", "api/core.md"],
    ),
    (
        // app-shape restatements (any page naming the first role file)
        "ssot.rs",
        &[
            "llm-contract-checklist.md",
            "llm-entry.md",
            "quickstart-authoring.md",
            "authoring-layout.md",
            "tasks/mirror-web-ui.md",
        ],
    ),
    (
        // scroll-inducement rule statements (roadmap Phase 6 item 2, NC-13)
        "exceed its container or the window",
        &["llm-contract-checklist.md", "quickstart-authoring.md"],
    ),
    (
        // the overflow advisory's code: the pages allowed to teach it
        "content_overflow_without_scroll_region",
        &[
            "llm-contract-checklist.md",
            "quickstart-authoring.md",
            "api/diagnostics.md",
            "api/routing-and-scroll.md",
        ],
    ),
    (
        // gap landing-zone rule statements (roadmap Phase 6 item 6a, NC-12)
        "GAPS.md",
        &["llm-contract-checklist.md", "llm-entry.md"],
    ),
];

#[test]
fn checklist_keeps_every_canonical_anchor() {
    let checklist = read_page(CANONICAL);
    for (rule, anchor) in CANONICAL_ANCHORS {
        assert!(
            checklist.contains(anchor),
            "llm-contract-checklist.md lost the canonical anchor for the \
             {rule}: {anchor:?}. If the canonical wording changed on purpose, \
             update the pinned copies and this test in the same commit."
        );
    }
    for file in APP_SHAPE_FILES {
        assert!(
            checklist.contains(&format!("`{file}`")),
            "llm-contract-checklist.md app-shape rule lost role file `{file}`"
        );
    }
}

#[test]
fn non_canonical_copies_keep_their_pinned_restatements_and_defer() {
    for copy in PINNED_COPIES {
        let content = read_page(copy.page);
        for pinned in copy.pinned {
            assert!(
                content.contains(pinned),
                "docs/public/{} diverged from the canonical {} \
                 (llm-contract-checklist.md): pinned restatement missing: \
                 {:?}. Non-canonical pages carry a one-line restatement plus \
                 a link; restate the rule only in the canonical file.",
                copy.page,
                copy.rule,
                pinned
            );
        }
        if copy.must_link_canonical {
            assert!(
                content.contains(CANONICAL),
                "docs/public/{} restates the {} but no longer links the \
                 canonical file {CANONICAL}",
                copy.page,
                copy.rule
            );
        }
    }
}

#[test]
fn app_shape_restatements_name_all_five_role_files() {
    // Pages that restate the app shape must list the SAME five roles; a
    // divergent copy (renamed or dropped role) fails here.
    for page in [
        "llm-contract-checklist.md",
        "llm-entry.md",
        "quickstart-authoring.md",
    ] {
        let content = read_page(page);
        for file in APP_SHAPE_FILES {
            assert!(
                content.contains(file),
                "docs/public/{page} restates the app shape but no longer \
                 names role file {file} — it diverged from the canonical \
                 shape in llm-contract-checklist.md"
            );
        }
    }
}

#[test]
fn contract_bearing_phrases_stay_on_registered_pages() {
    let root = docs_public_dir();
    let mut pages = Vec::new();
    collect_md_pages(&root, &root, &mut pages);
    assert!(
        pages.len() >= 10,
        "docs/public scan looks broken: only {} pages found",
        pages.len()
    );
    for (phrase, allowed) in PHRASE_ALLOWLIST {
        for (relative, content) in &pages {
            if content.contains(phrase) {
                assert!(
                    allowed.contains(&relative.as_str()),
                    "docs/public/{relative} carries the contract-bearing \
                     phrase {phrase:?} but is not registered for it. The \
                     canonical statement lives in {CANONICAL}; either remove \
                     the restatement or register the page (with a pinned \
                     one-line summary) in \
                     crates/slipway-core/tests/docs_canonical_rules.rs."
                );
            }
        }
    }
}

fn collect_md_pages(root: &Path, dir: &Path, pages: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_pages(root, &path, pages);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            let relative = path
                .strip_prefix(root)
                .expect("page under docs/public")
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
                .replace("\r\n", "\n");
            pages.push((relative, content));
        }
    }
}
