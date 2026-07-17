use slipway_core::{
    BoxSpacing, ChildLayoutPlan, ChildLayoutSeed, LayoutConstraints, ParentLocalRect, Size,
};

fn reject(wrong_space: ParentLocalRect) {
    let _ = ChildLayoutPlan::requested_outer(
        ChildLayoutSeed {
            child: "child".into(),
            local_state_slot: None,
        },
        wrong_space,
        LayoutConstraints {
            min: Size { width: 0.0, height: 0.0 },
            max: Size { width: 1.0, height: 1.0 },
        },
        BoxSpacing::default(),
    );
}

fn main() {}
