use slipway_core::{CaretSet, TextSelectionPolicyDeclaration, WidgetId};

fn main() {
    let target = WidgetId::from("text");

    let _carets = CaretSet {
        carets: Vec::new(),
        primary: 0,
    };

    let _selection = TextSelectionPolicyDeclaration {
        target,
        selection: None,
        carets: CaretSet::single(0),
        editable: true,
        diagnostics: Vec::new(),
    };
}