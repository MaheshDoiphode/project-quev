use anyhow::Result;
use gpui::{
    App, AsyncApp, Bounds, ClickEvent, Context, CursorStyle, Entity, EventEmitter, FocusHandle,
    Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, Size, Task, Window, canvas, fill, point, px,
};
use std::sync::Arc;
use workspace::ui::prelude::*;
use workspace::ui::{Button, ButtonStyle, Icon, IconName, Label, LabelSize};
use workspace::{AppState, Item};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 3.0;
const PAN_LINE_MULTIPLIER: f32 = 48.0;
const GRID_MINOR_WORLD_STEP: f32 = 64.0;
const GRID_MAJOR_MULTIPLIER: i32 = 5;

#[derive(Clone)]
struct CanvasNode {
    id: usize,
    title: SharedString,
    subtitle: SharedString,
    position: Point<Pixels>,
    size: Size<Pixels>,
}

#[derive(Clone, Copy)]
enum DragState {
    Panning {
        last_mouse_position: Point<Pixels>,
    },
    DraggingNode {
        node_id: usize,
        last_mouse_position: Point<Pixels>,
    },
}

pub async fn open_canvas_mode(app_state: Arc<AppState>, mut cx: AsyncApp) -> Result<()> {
    let multi_workspace = workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

    multi_workspace.update(&mut cx, |multi_workspace, window, cx| {
        let workspace = multi_workspace.workspace().clone();
        workspace.update(cx, |workspace, cx| {
            if let Some(existing) = workspace.item_of_type::<QuevCanvasView>(cx) {
                workspace.activate_item(&existing, true, true, window, cx);
                return;
            }

            let canvas_view = cx.new(|cx| QuevCanvasView::new(cx));
            workspace.add_item_to_active_pane(Box::new(canvas_view), None, true, window, cx);
        });
    })?;

    Ok(())
}

pub struct QuevCanvasView {
    focus_handle: FocusHandle,
    zoom_level: f32,
    pan_offset: Point<Pixels>,
    drag_state: Option<DragState>,
    nodes: Vec<CanvasNode>,
    next_node_id: usize,
}

impl QuevCanvasView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            zoom_level: 1.0,
            pan_offset: point(px(0.0), px(0.0)),
            drag_state: None,
            nodes: vec![
                CanvasNode {
                    id: 1,
                    title: "Canvas Node A".into(),
                    subtitle: "Drop future editors/agents here".into(),
                    position: point(px(120.0), px(100.0)),
                    size: Size {
                        width: px(300.0),
                        height: px(120.0),
                    },
                },
                CanvasNode {
                    id: 2,
                    title: "Canvas Node B".into(),
                    subtitle: "Wire up Claude/OpenCode flows".into(),
                    position: point(px(760.0), px(360.0)),
                    size: Size {
                        width: px(320.0),
                        height: px(120.0),
                    },
                },
                CanvasNode {
                    id: 3,
                    title: "Canvas Node C".into(),
                    subtitle: "Phase 1: pan, zoom, mode-switch".into(),
                    position: point(px(1320.0), px(840.0)),
                    size: Size {
                        width: px(340.0),
                        height: px(120.0),
                    },
                },
            ],
            next_node_id: 4,
        }
    }

    fn px_to_f32(value: Pixels) -> f32 {
        value.into()
    }

    fn world_to_screen_point(&self, world: Point<Pixels>) -> Point<Pixels> {
        let world_x = Self::px_to_f32(world.x);
        let world_y = Self::px_to_f32(world.y);
        let pan_x = Self::px_to_f32(self.pan_offset.x);
        let pan_y = Self::px_to_f32(self.pan_offset.y);

        point(
            px(pan_x + world_x * self.zoom_level),
            px(pan_y + world_y * self.zoom_level),
        )
    }

    fn screen_to_world_point(&self, screen: Point<Pixels>) -> Point<Pixels> {
        let screen_x = Self::px_to_f32(screen.x);
        let screen_y = Self::px_to_f32(screen.y);
        let pan_x = Self::px_to_f32(self.pan_offset.x);
        let pan_y = Self::px_to_f32(self.pan_offset.y);

        point(
            px((screen_x - pan_x) / self.zoom_level),
            px((screen_y - pan_y) / self.zoom_level),
        )
    }

    fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    fn reset_view(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_level = 1.0;
        self.pan_offset = point(px(0.0), px(0.0));
        self.drag_state = None;
        cx.notify();
    }

    fn begin_node_drag(&mut self, node_id: usize, cursor: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag_state = Some(DragState::DraggingNode {
            node_id,
            last_mouse_position: cursor,
        });
        cx.notify();
    }

    fn begin_pan(&mut self, cursor: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag_state = Some(DragState::Panning {
            last_mouse_position: cursor,
        });
        cx.notify();
    }

    fn handle_surface_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left || event.button == MouseButton::Middle {
            self.begin_pan(event.position, cx);
        }
    }

    fn apply_zoom_around_cursor(
        &mut self,
        requested_zoom: f32,
        cursor_position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let old_zoom = self.zoom_level;
        let new_zoom = requested_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (new_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }

        let cursor_x = Self::px_to_f32(cursor_position.x);
        let cursor_y = Self::px_to_f32(cursor_position.y);
        let old_pan_x = Self::px_to_f32(self.pan_offset.x);
        let old_pan_y = Self::px_to_f32(self.pan_offset.y);

        let world_x = (cursor_x - old_pan_x) / old_zoom;
        let world_y = (cursor_y - old_pan_y) / old_zoom;

        self.zoom_level = new_zoom;
        self.pan_offset = point(
            px(cursor_x - world_x * new_zoom),
            px(cursor_y - world_y * new_zoom),
        );

        cx.notify();
    }

    fn handle_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.control || event.modifiers.platform {
            let delta: f32 = match event.delta {
                ScrollDelta::Pixels(pixels) => pixels.y.into(),
                ScrollDelta::Lines(lines) => lines.y * PAN_LINE_MULTIPLIER,
            };

            let zoom_factor = if delta > 0.0 {
                1.0 + delta.abs() * 0.01
            } else {
                1.0 / (1.0 + delta.abs() * 0.01)
            };

            self.apply_zoom_around_cursor(self.zoom_level * zoom_factor, event.position, cx);
        } else {
            let delta = match event.delta {
                ScrollDelta::Pixels(pixels) => pixels,
                ScrollDelta::Lines(lines) => lines.map(|d| px(d * PAN_LINE_MULTIPLIER)),
            };

            self.pan_offset += delta;
            cx.notify();
        }
    }

    fn handle_surface_click(
        &mut self,
        event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() < 2 {
            return;
        }

        let world = self.screen_to_world_point(event.position());
        let node_id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(CanvasNode {
            id: node_id,
            title: format!("Node {}", node_id).into(),
            subtitle: "Double-click spawned node".into(),
            position: world,
            size: Size {
                width: px(260.0),
                height: px(110.0),
            },
        });
        cx.notify();
    }

    fn handle_surface_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_state = None;
        cx.notify();
    }

    fn handle_surface_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag_state) = self.drag_state else {
            return;
        };

        match drag_state {
            DragState::Panning {
                last_mouse_position,
            } => {
                let delta = event.position - last_mouse_position;
                self.pan_offset += delta;
                self.drag_state = Some(DragState::Panning {
                    last_mouse_position: event.position,
                });
                cx.notify();
            }
            DragState::DraggingNode {
                node_id,
                last_mouse_position,
            } => {
                let delta = event.position - last_mouse_position;
                let dx = Self::px_to_f32(delta.x) / self.zoom_level;
                let dy = Self::px_to_f32(delta.y) / self.zoom_level;

                if let Some(node) = self.nodes.iter_mut().find(|node| node.id == node_id) {
                    node.position = point(
                        node.position.x + px(dx),
                        node.position.y + px(dy),
                    );
                }

                self.drag_state = Some(DragState::DraggingNode {
                    node_id,
                    last_mouse_position: event.position,
                });
                cx.notify();
            }
        }
    }

    fn render_grid(
        bounds: Bounds<Pixels>,
        pan_offset: Point<Pixels>,
        zoom_level: f32,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_quad(fill(bounds, cx.theme().colors().editor_background));

        let minor_world_step = GRID_MINOR_WORLD_STEP;
        let major_stride = GRID_MAJOR_MULTIPLIER;
        let zoom = zoom_level;
        let pan_x = Self::px_to_f32(pan_offset.x);
        let pan_y = Self::px_to_f32(pan_offset.y);

        let left = Self::px_to_f32(bounds.left());
        let right = Self::px_to_f32(bounds.right());
        let top = Self::px_to_f32(bounds.top());
        let bottom = Self::px_to_f32(bounds.bottom());

        let world_left = (left - pan_x) / zoom;
        let world_right = (right - pan_x) / zoom;
        let world_top = (top - pan_y) / zoom;
        let world_bottom = (bottom - pan_y) / zoom;

        let first_col = (world_left / minor_world_step).floor() as i32;
        let last_col = (world_right / minor_world_step).ceil() as i32;
        let first_row = (world_top / minor_world_step).floor() as i32;
        let last_row = (world_bottom / minor_world_step).ceil() as i32;

        let minor_color = cx.theme().colors().border_variant.opacity(0.25);
        let major_color = cx.theme().colors().border.opacity(0.55);

        for col in first_col..=last_col {
            let x = pan_x + col as f32 * minor_world_step * zoom;
            let x_px = px(x);
            let color = if col.rem_euclid(major_stride) == 0 {
                major_color
            } else {
                minor_color
            };

            window.paint_quad(fill(
                Bounds::from_corners(point(x_px, bounds.top()), point(x_px + px(1.0), bounds.bottom())),
                color,
            ));
        }

        for row in first_row..=last_row {
            let y = pan_y + row as f32 * minor_world_step * zoom;
            let y_px = px(y);
            let color = if row.rem_euclid(major_stride) == 0 {
                major_color
            } else {
                minor_color
            };

            window.paint_quad(fill(
                Bounds::from_corners(point(bounds.left(), y_px), point(bounds.right(), y_px + px(1.0))),
                color,
            ));
        }
    }

    fn render_node(&self, node: &CanvasNode, cx: &mut Context<Self>) -> impl IntoElement {
        let screen_pos = self.world_to_screen_point(node.position);
        let width = px(Self::px_to_f32(node.size.width) * self.zoom_level);
        let height = px(Self::px_to_f32(node.size.height) * self.zoom_level);
        let node_id = node.id;

        v_flex()
            .id(("quev-canvas-node", node.id as u64))
            .absolute()
            .left(screen_pos.x)
            .top(screen_pos.y)
            .w(width)
            .h(height)
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border)
            .bg(cx.theme().colors().element_active)
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.begin_node_drag(node_id, event.position, cx);
                cx.stop_propagation();
            }))
            .child(
                Label::new(node.title.clone())
                    .size(LabelSize::Small)
                    .color(Color::Default),
            )
            .child(
                Label::new(node.subtitle.clone())
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }
}

impl EventEmitter<()> for QuevCanvasView {}

impl Focusable for QuevCanvasView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for QuevCanvasView {
    type Event = ();

    fn to_item_events(_: &Self::Event, _: &mut dyn FnMut(workspace::item::ItemEvent)) {}

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Canvas".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Blocks).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Quev Canvas Opened")
    }

    fn can_split(&self) -> bool {
        true
    }

    fn clone_on_split(
        &self,
        _workspace_id: Option<workspace::WorkspaceId>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<Entity<Self>>>
    where
        Self: Sized,
    {
        let snapshot_nodes = self.nodes.clone();
        let snapshot_zoom = self.zoom_level;
        let snapshot_pan = self.pan_offset;
        let snapshot_next_id = self.next_node_id;

        Task::ready(Some(cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            zoom_level: snapshot_zoom,
            pan_offset: snapshot_pan,
            drag_state: None,
            nodes: snapshot_nodes,
            next_node_id: snapshot_next_id,
        })))
    }
}

impl Render for QuevCanvasView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom_label = format!("{:.0}%", self.zoom_level * 100.0);
        let mut node_elements = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            node_elements.push(self.render_node(node, cx).into_any_element());
        }

        v_flex()
            .id("quev-canvas-view")
            .key_context("QuevCanvasView")
            .size_full()
            .track_focus(&self.focus_handle(cx))
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
            }))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .bg(cx.theme().colors().elevated_surface_background)
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(Label::new("Quev Canvas (Phase 1)").size(LabelSize::Small))
                            .child(
                                Label::new(
                                    "Drag to pan, Ctrl/Cmd + wheel to zoom, wheel to nudge",
                                )
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Label::new(zoom_label)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Button::new("canvas-reset-view", "Reset view")
                                    .style(ButtonStyle::Outlined)
                                    .on_click(cx.listener(Self::reset_view)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("quev-canvas-surface")
                    .relative()
                    .flex_1()
                    .overflow_hidden()
                    .cursor(if self.is_dragging() {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    })
                    .on_scroll_wheel(cx.listener(Self::handle_scroll_wheel))
                    .on_click(cx.listener(Self::handle_surface_click))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_surface_mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::handle_surface_mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_surface_mouse_up))
                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::handle_surface_mouse_up))
                    .on_mouse_move(cx.listener(Self::handle_surface_mouse_move))
                    .child(
                        canvas(
                            |_bounds, _window, _cx| {},
                            {
                                let zoom_level = self.zoom_level;
                                let pan_offset = self.pan_offset;
                                move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
                                    QuevCanvasView::render_grid(
                                        bounds,
                                        pan_offset,
                                        zoom_level,
                                        window,
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .children(node_elements)
                    .child(
                        h_flex()
                            .absolute()
                            .right_3()
                            .bottom_3()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(cx.theme().colors().elevated_surface_background)
                            .border_1()
                            .border_color(cx.theme().colors().border_variant)
                            .child(
                                Label::new("Double click canvas to create a node")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    ),
            )
    }
}
