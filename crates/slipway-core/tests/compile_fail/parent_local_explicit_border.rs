use slipway_core::{BoxSpacing, ChildLayoutPlan, ChildLayoutSeed, ParentLocalRect};

fn reject(wrong_space: ParentLocalRect) {
    let _ = ChildLayoutPlan::explicit_border(
        ChildLayoutSeed {
            child: "child".into(),
            local_state_slot: None,
        },
        wrong_space,
        BoxSpacing::default(),
    );
}

fn main() {}
