use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn production_source(relative: &str) -> String {
    let source = fs::read_to_string(workspace_root().join(relative))
        .unwrap()
        .replace("\r\n", "\n");
    let marker = "#[cfg(test)]\nmod tests {";
    source
        .find(marker)
        .map_or(source.clone(), |index| source[..index].to_string())
}

fn layout_bodies(source: &str) -> Vec<&str> {
    let mut bodies = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = source[cursor..].find("fn layout(") {
        let start = cursor + relative;
        let tail = &source[start..];
        let Some(open_relative) = tail.find('{') else {
            break;
        };
        if tail.find(';').is_some_and(|semi| semi < open_relative) {
            cursor = start + open_relative;
            continue;
        }
        if !tail[..open_relative].contains("LayoutOutputBuilder") {
            cursor = start + open_relative;
            continue;
        }
        let open = start + open_relative;
        let mut depth = 0usize;
        for (offset, byte) in source.as_bytes()[open..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        bodies.push(&source[open + 1..open + offset]);
                        cursor = open + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    bodies
}

fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let open = source[start..].find('{').unwrap() + start;
    let mut depth = 0usize;
    for (offset, byte) in source.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}");
}

#[test]
fn public_layout_helpers_cannot_create_replacement_builders() {
    let core = production_source("crates/slipway-core/src/lib.rs");
    for signature in [
        "pub fn prepare_leaf_layout(",
        "pub fn prepare_resolved_layout(",
        "pub fn layout_view_definition<",
    ] {
        assert!(!function_body(&core, signature).contains("LayoutOutputBuilder::for_input"));
    }
    assert!(function_body(&core, "pub fn prepare_leaf_layout(").contains("output.finish(bounds)"));
    assert!(
        function_body(&core, "pub fn prepare_resolved_layout(").contains("output.push_resolved")
    );
}

#[test]
fn custom_view_definitions_do_not_replace_the_injected_builder() {
    for relative in [
        "crates/slipway-core/src/lib.rs",
        "crates/slipway/src/lib.rs",
        "crates/slipway-example-admission/src/main.rs",
        "crates/slipway-example-authored/src/view.rs",
        "crates/slipway-backend-egui/src/lib.rs",
        "crates/slipway-backend-iced/src/lib.rs",
    ] {
        let source = production_source(relative);
        assert!(
            !source.contains("layout_view(self, external, local, input.layout_input"),
            "{relative} rebuilds layout instead of transferring ViewDefinitionInput"
        );
        assert!(
            !source.contains("_output: LayoutOutputBuilder")
                && !source.contains("_output: slipway_core::LayoutOutputBuilder"),
            "{relative} has a layout implementation that explicitly ignores its builder"
        );
        for body in layout_bodies(&source) {
            assert!(
                body.contains("output"),
                "{relative} has a production layout implementation that does not consume output"
            );
        }
    }
}

#[test]
fn builder_creation_is_limited_to_named_core_boundaries() {
    let core = production_source("crates/slipway-core/src/lib.rs");
    assert_eq!(core.matches("LayoutOutputBuilder::for_input").count(), 5);
    for relative in [
        "crates/slipway/src/lib.rs",
        "crates/slipway-example-admission/src/main.rs",
        "crates/slipway-example-authored/src/view.rs",
        "crates/slipway-backend-egui/src/lib.rs",
        "crates/slipway-backend-iced/src/lib.rs",
    ] {
        assert!(!production_source(relative).contains("LayoutOutputBuilder::for_input"));
    }

    let transfer = function_body(&core, "pub fn into_layout_parts(");
    assert!(transfer.contains("self.output"));
    assert!(!transfer.contains("for_input"));
}
