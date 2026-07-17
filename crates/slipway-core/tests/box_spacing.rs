use slipway_core::*;

fn size(width: f32, height: f32) -> Size {
    Size { width, height }
}
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        origin: Point { x, y },
        size: size(width, height),
    }
}

#[test]
fn constructors_and_asymmetric_equations_are_exact() {
    assert_eq!(EdgeInsets::default(), EdgeInsets::ZERO);
    assert_eq!(
        EdgeInsets::symmetric(3.0, 5.0),
        EdgeInsets::trbl(3.0, 5.0, 3.0, 5.0)
    );
    let spacing = BoxSpacing::new(
        EdgeInsets::trbl(8.0, 24.0, 12.0, 4.0),
        EdgeInsets::trbl(6.0, 28.0, 18.0, 10.0),
    );
    let target = derive_target_box(size(100.0, 60.0), spacing).unwrap();
    assert_eq!(target.border.into_rect(), rect(0.0, 0.0, 100.0, 60.0));
    assert_eq!(target.content.into_rect(), rect(10.0, 6.0, 62.0, 36.0));
    assert_eq!(target.default_clip, target.border);
    assert_eq!(target.default_hit_bounds, target.border);
}

#[test]
fn every_spacing_edge_is_validated_and_finite_excess_is_allowed() {
    for edge in 0..8 {
        let mut values = [0.0; 8];
        values[edge] = -1.0;
        let spacing = BoxSpacing::new(
            EdgeInsets::trbl(values[0], values[1], values[2], values[3]),
            EdgeInsets::trbl(values[4], values[5], values[6], values[7]),
        );
        assert!(derive_target_box(size(10.0, 10.0), spacing).is_err());
        values[edge] = f32::NAN;
        let spacing = BoxSpacing::new(
            EdgeInsets::trbl(values[0], values[1], values[2], values[3]),
            EdgeInsets::trbl(values[4], values[5], values[6], values[7]),
        );
        assert!(derive_target_box(size(10.0, 10.0), spacing).is_err());
    }
    let target = derive_target_box(
        size(20.0, 10.0),
        BoxSpacing::ZERO.with_padding(EdgeInsets::trbl(30.0, 40.0, 50.0, 60.0)),
    )
    .unwrap();
    assert_eq!(target.content.into_rect(), rect(20.0, 10.0, 0.0, 0.0));
}

#[test]
fn requested_layout_subtracts_margin_before_exposing_content() {
    let request = ChildLayoutPlan::requested_outer(
        ChildLayoutSeed {
            child: WidgetId::from("child"),
            local_state_slot: None,
        },
        ContentLocalRect::new(rect(7.0, 11.0, 100.0, 80.0)),
        LayoutConstraints {
            min: size(60.0, 50.0),
            max: size(90.0, 70.0),
        },
        BoxSpacing::new(
            EdgeInsets::trbl(3.0, 7.0, 5.0, 11.0),
            EdgeInsets::trbl(2.0, 13.0, 17.0, 19.0),
        ),
    );
    let prepared = prepare_child_layout(&request).unwrap();
    assert_eq!(
        prepared.input.viewport.into_rect(),
        rect(0.0, 0.0, 82.0, 72.0)
    );
    assert_eq!(
        prepared.input.content.into_rect(),
        rect(19.0, 2.0, 50.0, 53.0)
    );
    assert_eq!(
        prepared.input.constraints,
        LayoutConstraints {
            min: size(42.0, 42.0),
            max: size(72.0, 62.0)
        }
    );
}

#[test]
fn explicit_border_and_effective_clip_have_single_composition() {
    let plan = ChildLayoutPlan::explicit_border(
        ChildLayoutSeed {
            child: WidgetId::from("child"),
            local_state_slot: None,
        },
        ContentLocalRect::new(rect(20.0, 30.0, 40.0, 50.0)),
        BoxSpacing::ZERO.with_margin(EdgeInsets::all(8.0)),
    );
    assert!(matches!(
        plan.request.geometry,
        ChildLayoutGeometry::ExplicitBorder(_)
    ));
    let default = TargetLocalRect::new(rect(0.0, 0.0, 40.0, 50.0));
    let overflow = TargetLocalRect::new(rect(-5.0, -6.0, 52.0, 64.0));
    assert_eq!(
        effective_clip(
            default,
            Some(overflow),
            Some(rect(18.0, 25.0, 45.0, 50.0)),
            Translation { x: 20.0, y: 30.0 }
        ),
        rect(18.0, 25.0, 45.0, 50.0)
    );
}

#[test]
fn zero_spacing_preserves_border_content_and_hit_convenience() {
    let target = derive_target_box(size(32.0, 24.0), BoxSpacing::ZERO).unwrap();
    assert_eq!(target.border, target.content);
    let input = LayoutInput {
        viewport: target.border,
        content: target.content,
        constraints: LayoutConstraints {
            min: size(0.0, 0.0),
            max: size(32.0, 24.0),
        },
    };
    assert_eq!(full_border_hit_bounds(&input), input.viewport);
}
