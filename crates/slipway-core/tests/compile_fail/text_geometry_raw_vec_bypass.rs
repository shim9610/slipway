use slipway_core::{
    CaretGeometryEvidence, CaretSet, Point, Rect, Size, TextSelectionPolicyDeclaration, WidgetId,
};

fn main() {
    let target = WidgetId::from("text");

    let _selection = TextSelectionPolicyDeclaration {
        target: target.clone(),
        selection: None,
        carets: CaretSet {
            carets: Vec::new(),
            primary: 0,
        },
        editable: true,
        diagnostics: Vec::new(),
    };

    let _caret = CaretGeometryEvidence {
        target,
        caret_bounds: vec![Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 1.0,
                height: 16.0,
            },
        }],
        selection_bounds: Vec::new(),
        measurement_request_ids: Vec::new(),
        diagnostics: Vec::new(),
    };
}