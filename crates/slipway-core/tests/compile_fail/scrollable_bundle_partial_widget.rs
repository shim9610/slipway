//! The audit's P1 probe shape (2026-07-11, LE-H7): a quickstart-level
//! PARTIALLY-implemented scrollable widget. The author implemented the
//! widget trio (Ssot/Logic/View) plus the two scroll policies, then asks
//! for the scrollable-container bundle. The pinned stderr must carry the
//! bundle-level `on_unimplemented` triage in every E0277 block and must
//! NOT contain the misleading `SlipwayAppWidget<A>` wrapper suggestion.

use slipway_core::*;

struct PartialListWidget;
struct ProbeExternal;
struct ProbeLocal;
enum ProbeMessage {}

impl SlipwayWidgetTypes for PartialListWidget {
    type ExternalState = ProbeExternal;
    type LocalState = ProbeLocal;
    type AppMessage = ProbeMessage;
}

impl SlipwaySsot for PartialListWidget {
    fn id(&self) -> WidgetId {
        WidgetId::from("probe.list")
    }

    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode::leaf(self.id())
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

impl SlipwayLogic for PartialListWidget {
    fn handle_event(
        &self,
        _external: &Self::ExternalState,
        _local: &mut Self::LocalState,
        _event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        EventOutcome::ignored()
    }
}

impl SlipwayView for PartialListWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        ProbeLocal
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: LayoutOutputBuilder,
    ) -> LayoutOutput {
        output.finish(input.viewport)
    }

    fn paint(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        Vec::new()
    }

    fn observe_state(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        Vec::new()
    }
}

impl SlipwayScrollBehaviorPolicy for PartialListWidget {
    fn scroll_behavior_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration {
        let viewport = input.viewport.into_rect();
        ScrollBehaviorPolicyDeclaration {
            target: self.id(),
            region_id: None,
            address: None,
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            extent: Size {
                width: viewport.size.width,
                height: viewport.size.height * 2.0,
            },
            viewport: input.viewport,
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: viewport.size.width,
                    height: viewport.size.height * 2.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayWheelRoutingPolicy for PartialListWidget {
    fn wheel_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration {
        WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::NearestScrollable,
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}

fn requires_scrollable_bundle<T: SlipwayScrollableContainerCapability>(_widget: &T) {}

fn main() {
    requires_scrollable_bundle(&PartialListWidget);
}
