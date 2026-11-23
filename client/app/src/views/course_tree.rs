use crate::view_model::ClientViewModel;
use cynapse_client_core::{AdapterId, CourseTreeNodeView, UnitResetScope, UnitStatus, UnitView};
use cynapse_client_i18n::{t, t_with};
use cynapse_client_model::ProjectedCourseRow;
use gpui_kit::base::{ScrollBounce, Tree as BaseTree};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::collections::HashSet;
use std::path::PathBuf;

/// Course-tree button actions; presentation only, derived from Unit status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnitAction {
    Select,
    Unselect,
    Reset,
}

/// Course tree with per-Unit status, progress, and actions.
///
/// Owns only presentation state (tree expansion/rows); selection and progress
/// stay in [`ClientViewModel`]. Clicking a row never selects a Unit; selection
/// is an explicit row action.
pub struct CourseTreeView {
    view_model: Entity<ClientViewModel>,
    tree_state: Entity<TreeState>,
    last_content_revision: u64,
    expanded: HashSet<String>,
    known_folders: HashSet<String>,
    last_selected: Option<String>,
    _tree_subscription: Subscription,
    _subscription: Subscription,
}

impl CourseTreeView {
    pub fn new(view_model: Entity<ClientViewModel>, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let subscription = cx.observe(&view_model, |this, view_model, cx| {
            let (content_revision, selected) = {
                let view_model = view_model.read(cx);
                (
                    view_model.course_content_revision(),
                    view_model.selected_unit_path().map(str::to_string),
                )
            };
            if content_revision != this.last_content_revision {
                let tree = view_model.read(cx).course_tree().to_vec();
                this.refresh_structure(&tree, content_revision, cx);
            } else if selected != this.last_selected {
                let mut expanded_changed = false;
                if let Some(selected) = selected.as_deref() {
                    expanded_changed = expand_ancestors(selected, &mut this.expanded);
                }
                this.last_selected = selected;
                if expanded_changed {
                    let tree = view_model.read(cx).course_tree().to_vec();
                    let items = tree_items(&tree, &this.expanded);
                    this.tree_state
                        .update(cx, |state, cx| state.set_items(items, cx));
                }
            }
            cx.notify();
        });
        let tree_subscription =
            cx.subscribe(&tree_state, |this, _, event: &TreeEvent, _| match event {
                TreeEvent::Expanded(id) => {
                    this.expanded.insert(id.to_string());
                }
                TreeEvent::Collapsed(id) => {
                    this.expanded.remove(id.as_ref());
                }
            });
        let content_revision = view_model.read(cx).course_content_revision();
        let mut view = Self {
            view_model,
            tree_state,
            last_content_revision: 0,
            expanded: HashSet::new(),
            known_folders: HashSet::new(),
            last_selected: None,
            _tree_subscription: tree_subscription,
            _subscription: subscription,
        };
        let tree = view.view_model.read(cx).course_tree().to_vec();
        view.refresh_structure(&tree, content_revision, cx);
        view
    }

    fn refresh_structure(
        &mut self,
        tree: &[CourseTreeNodeView],
        content_revision: u64,
        cx: &mut Context<Self>,
    ) {
        self.last_content_revision = content_revision;
        sync_expanded_folders(tree, &mut self.expanded, &mut self.known_folders);
        let selected = self
            .view_model
            .read(cx)
            .selected_unit_path()
            .map(str::to_string);
        if selected != self.last_selected {
            if let Some(selected) = selected.as_deref() {
                expand_ancestors(selected, &mut self.expanded);
            }
            self.last_selected = selected;
        }
        let items = tree_items(tree, &self.expanded);
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
    }
}

fn sync_expanded_folders(
    nodes: &[CourseTreeNodeView],
    expanded: &mut HashSet<String>,
    known_folders: &mut HashSet<String>,
) {
    for node in nodes {
        if !node.children.is_empty() && known_folders.insert(node.path.clone()) {
            expanded.insert(node.path.clone());
        }
        sync_expanded_folders(&node.children, expanded, known_folders);
    }
}

fn expand_ancestors(path: &str, expanded: &mut HashSet<String>) -> bool {
    let mut current = path;
    let mut changed = false;
    while let Some((parent, _)) = current.rsplit_once('/') {
        changed |= expanded.insert(parent.to_string());
        current = parent;
    }
    changed
}

impl Render for CourseTreeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view_model.read(cx).course_rows().is_empty() {
            let recent = self.view_model.read(cx).recent_courses();
            if recent.is_empty() {
                return div().into_any_element();
            }
            return div()
                .size_full()
                .flex()
                .flex_col()
                .pt_4()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t("course_tree.recent")),
                        )
                        .child({
                            let view_model = self.view_model.clone();
                            Button::new("clear-recent-courses")
                                .small()
                                .label(t("course_tree.clear_recent"))
                                .on_click(move |_, window, cx| {
                                    let view_model = view_model.clone();
                                    window.open_alert_dialog(cx, move |alert, _, _| {
                                        let view_model = view_model.clone();
                                        alert
                                            .title(t("course_tree.clear_recent.title"))
                                            .description(t("course_tree.clear_recent.message"))
                                            .show_cancel(true)
                                            .on_ok(move |_, _, cx| {
                                                view_model.update(cx, |view_model, cx| {
                                                    view_model.clear_recent_courses(cx)
                                                });
                                                true
                                            })
                                    });
                                })
                        }),
                )
                .children(
                    recent
                        .into_iter()
                        .take(5)
                        .enumerate()
                        .map(|(index, course)| {
                            let view_model = self.view_model.clone();
                            let path = course.path.clone();
                            Button::new(format!("recent-course-{index}"))
                                .ghost()
                                .label(course.title)
                                .tooltip(course.path)
                                .on_click(move |_, _, cx| {
                                    view_model.update(cx, |view_model, cx| {
                                        view_model.open_course(PathBuf::from(path.clone()), cx)
                                    });
                                })
                        }),
                )
                .into_any_element();
        }

        let view_model = self.view_model.read(cx);
        let actions_available = view_model.tree_actions_available();
        let transitioning = view_model.transition_blocks_controls();
        let actions_disabled = view_model.reset_pending()
            || view_model.challenge_pending()
            || view_model.advance_pending()
            || transitioning
            || !actions_available;

        let rows = self.view_model.read(cx).course_rows();
        let view_model = self.view_model.clone();

        let scroll_handle = self.tree_state.read(cx).scroll_handle().clone();
        div()
            .size_full()
            .child(
                BaseTree::new(&self.tree_state)
                    .list_style(StyleRefinement::default().flex_grow_1().size_full())
                    .item(move |_ix, entry, _entry_state, _, cx| {
                        let path = entry.item().id.to_string();
                        let row = rows.get(&path);
                        let unit = row.and_then(|row| row.unit.as_ref());
                        let progress = row.map_or(NodeProgress::Idle, row_progress);
                        let selected =
                            view_model.read(cx).selected_unit_path() == Some(path.as_str());
                        let disclosure: AnyElement = if entry.is_folder() {
                            Icon::new(if entry.is_expanded() {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .small()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element()
                        } else {
                            div().size(px(14.)).into_any_element()
                        };
                        let tooltip_title = entry.item().label.to_string();
                        let label_color = if selected {
                            cx.theme().foreground
                        } else {
                            cx.theme().muted_foreground
                        };
                        let progress_label = unit.and_then(unit_progress_label);
                        let actions = unit
                            .map(|unit| unit_actions(unit, selected))
                            .unwrap_or_default();
                        let title = tooltip_title.clone();

                        let mut content = div()
                            .flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(disclosure)
                            .child(
                                div()
                                    .id(format!("course-label-{path}"))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(label_color)
                                    .when(selected, |style| {
                                        style.font_weight(gpui::FontWeight::SEMIBOLD)
                                    })
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tooltip_title.clone()).build(window, cx)
                                    })
                                    .child(entry.item().label.clone()),
                            );

                        let mut trailing = div().flex().items_center();
                        let mut has_trailing = false;
                        if actions_available {
                            for action in actions {
                                let (label, icon) = action_presentation(action);
                                let model = view_model.clone();
                                let path = path.clone();
                                let title = title.clone();
                                has_trailing = true;
                                trailing = trailing.child(
                                    Button::new(format!("unit-{}-{path}", action_key(action)))
                                        .ghost()
                                        .small()
                                        .icon(icon)
                                        .tooltip(t_with(
                                            "unit.action.tooltip",
                                            &[("action", &label), ("title", &title)],
                                        ))
                                        .accessibility_label(label)
                                        .disabled(actions_disabled)
                                        .flex_shrink_0()
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            let model = model.clone();
                                            let path = path.clone();
                                            let title = title.clone();
                                            match action {
                                                UnitAction::Select => {
                                                    model.update(cx, |model, cx| {
                                                        model.select_unit(path, cx)
                                                    });
                                                }
                                                UnitAction::Unselect => {
                                                    model.update(cx, |model, cx| {
                                                        model.clear_selection(cx)
                                                    });
                                                }
                                                UnitAction::Reset => {
                                                    let Some(identity) =
                                                        model.read(cx).course_identity()
                                                    else {
                                                        return;
                                                    };
                                                    let jupyterlab =
                                                        model.read(cx).units().iter().any(|unit| {
                                                            unit.path == path
                                                                && unit.adapters.contains(
                                                                    &AdapterId::Jupyterlab,
                                                                )
                                                        });
                                                    let description = model
                                                        .read(cx)
                                                        .session()
                                                        .unit_reset_scope(&path)
                                                        .map(|scope| {
                                                            unit_reset_description(
                                                                &scope, jupyterlab,
                                                            )
                                                        })
                                                        .unwrap_or_else(|_| {
                                                            t("unit.reset.description")
                                                        });
                                                    window.open_alert_dialog(
                                                        cx,
                                                        move |alert, _, _| {
                                                            let model = model.clone();
                                                            let path = path.clone();
                                                            let identity = identity.clone();
                                                            alert
                                                                .title(t_with(
                                                                    "unit.reset.title",
                                                                    &[("title", &title)],
                                                                ))
                                                                .description(description.clone())
                                                                .show_cancel(true)
                                                                .on_ok(move |_, _, cx| {
                                                                    let identity = identity.clone();
                                                                    model.update(cx, |model, cx| {
                                                                model.reset_unit_if_current(
                                                                    identity,
                                                                    path.clone(),
                                                                    cx,
                                                                )
                                                            });
                                                                    true
                                                                })
                                                        },
                                                    );
                                                }
                                            }
                                        }),
                                );
                            }
                        }

                        if let Some(label) = progress_label {
                            has_trailing = true;
                            trailing = trailing.child(
                                div()
                                    .id(format!("unit-progress-{path}"))
                                    .test_support()
                                    .w(px(PROGRESS_LABEL_WIDTH))
                                    .flex_shrink_0()
                                    .text_right()
                                    .text_xs()
                                    .text_color(progress_text_color(progress, cx))
                                    .child(label),
                            );
                        }
                        if has_trailing {
                            content = content.child(trailing);
                        }

                        ListItem::new(entry.item().id.clone())
                            .pl(px(entry.depth() as f32 * 16.0))
                            // The Tree scrollbar is a 16px overlay; keep row
                            // controls out of its lane instead of adding an
                            // outer scroll container.
                            .pr(px(16.))
                            .h(px(TREE_ROW_HEIGHT))
                            .accessibility_label(entry.item().label.clone())
                            .child(content)
                            .into_any_element()
                    })
                    .relative()
                    .size_full()
                    .map(|tree| ScrollBounce::new("tree-scroll-bounce", &scroll_handle, tree)),
            )
            .vertical_scrollbar(&scroll_handle)
            .into_any_element()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeProgress {
    Idle,
    Active,
    PendingFinish,
    Done,
}

/// The tree only needs to fit two-digit Step counters (`[99/99]`).
const PROGRESS_LABEL_WIDTH: f32 = 44.;
/// `uniform_list` measures the first row; every row must use this height.
const TREE_ROW_HEIGHT: f32 = 32.;

fn row_progress(row: &ProjectedCourseRow) -> NodeProgress {
    if row.completed {
        NodeProgress::Done
    } else if row.unit.as_ref().is_some_and(|unit| unit.completed) {
        NodeProgress::PendingFinish
    } else if row.has_progress {
        NodeProgress::Active
    } else {
        NodeProgress::Idle
    }
}

#[cfg(test)]
fn node_progress(node: &CourseTreeNodeView) -> NodeProgress {
    if node.completed {
        NodeProgress::Done
    } else if node.unit.as_ref().is_some_and(|unit| unit.completed) {
        NodeProgress::PendingFinish
    } else if node_has_progress(node) {
        NodeProgress::Active
    } else {
        NodeProgress::Idle
    }
}

#[cfg(test)]
fn node_has_progress(node: &CourseTreeNodeView) -> bool {
    node.completed
        || node
            .unit
            .as_ref()
            .is_some_and(|unit| unit.completed || unit.status != UnitStatus::NotStarted)
        || node.children.iter().any(node_has_progress)
}

fn progress_text_color(progress: NodeProgress, cx: &App) -> Hsla {
    match progress {
        NodeProgress::Done => cx.theme().success,
        NodeProgress::PendingFinish => cx.theme().warning,
        NodeProgress::Active => cx.theme().primary,
        NodeProgress::Idle => cx.theme().muted_foreground.opacity(0.7),
    }
}

fn unit_actions(unit: &UnitView, selected: bool) -> Vec<UnitAction> {
    match unit.status {
        UnitStatus::Completed => vec![UnitAction::Reset],
        UnitStatus::NotStarted if selected => vec![UnitAction::Unselect],
        UnitStatus::NotStarted => vec![UnitAction::Select],
        UnitStatus::InProgress | UnitStatus::Paused if selected => {
            vec![UnitAction::Reset, UnitAction::Unselect]
        }
        UnitStatus::InProgress | UnitStatus::Paused => {
            vec![UnitAction::Reset, UnitAction::Select]
        }
    }
}

fn action_presentation(action: UnitAction) -> (String, IconName) {
    match action {
        UnitAction::Select => (t("unit.action.start"), IconName::Play),
        UnitAction::Reset => (t("unit.action.reset"), IconName::RotateCw),
        UnitAction::Unselect => (t("unit.action.cancel"), IconName::Pause),
    }
}

fn action_key(action: UnitAction) -> &'static str {
    match action {
        UnitAction::Select => "select",
        UnitAction::Reset => "reset",
        UnitAction::Unselect => "unselect",
    }
}

fn unit_progress_label(unit: &UnitView) -> Option<String> {
    let total = unit.steps.len();
    if total == 0 {
        return None;
    }
    if unit.status == UnitStatus::NotStarted {
        return Some(format!("[0/{total}]"));
    }
    if unit.completed {
        return Some(format!("[{total}/{total}]"));
    }
    let current = unit
        .current_step_id
        .as_ref()
        .and_then(|step_id| unit.steps.iter().position(|step| &step.step_id == step_id));
    Some(format!(
        "[{}/{total}]",
        current.map_or(0, |index| index + 1)
    ))
}

fn unit_reset_description(scope: &UnitResetScope, jupyterlab: bool) -> String {
    let mut description = t("unit.reset.description_short");
    if !scope.workspace_paths.is_empty() {
        description.push_str("\n\n");
        description.push_str(&t("unit.reset.workspace_files"));
        for path in &scope.workspace_paths {
            description.push_str("\n- workspace/");
            description.push_str(path);
        }
    }
    if jupyterlab {
        description.push_str("\n\n");
        description.push_str(&t("unit.reset.jupyterlab_warning"));
    }
    description
}

fn tree_items(nodes: &[CourseTreeNodeView], expanded: &HashSet<String>) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| {
            let item = TreeItem::new(node.path.clone(), node.title.clone())
                .expanded(!node.children.is_empty() && expanded.contains(&node.path));
            item.children(tree_items(&node.children, expanded))
        })
        .collect()
}

#[cfg(test)]
mod tests;
