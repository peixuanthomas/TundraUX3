//! Optional, per-frame UI metadata collected by the same code that paints controls.

use std::cell::RefCell;

use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
pub use shell_control::{Node, Snapshot};

thread_local! {
    static CAPTURE: RefCell<Option<Collector>> = const { RefCell::new(None) };
}

struct Collector {
    bounds: Rect,
    page: String,
    nodes: Vec<Node>,
    parent: String,
    blocked: bool,
    transform: Projection,
}

#[derive(Clone, Copy)]
struct Projection {
    x: i32,
    y: i32,
    clip: Rect,
}

/// Starts collection only for frames the host intends to export.
pub struct FrameCapture;

impl FrameCapture {
    pub fn begin(bounds: Rect, page: impl Into<String>) -> Self {
        let page = page.into();
        let mut root = node("screen", "screen", &page, bounds);
        root.actionable = false;
        CAPTURE.with_borrow_mut(|capture| {
            *capture = Some(Collector {
                bounds,
                page,
                nodes: vec![root],
                parent: "screen".into(),
                blocked: false,
                transform: Projection {
                    x: 0,
                    y: 0,
                    clip: bounds,
                },
            });
        });
        Self
    }

    /// Changes the compositor layer. Blocked layers retain their visible controls.
    pub fn set_group(&self, id: &str, role: &str, area: Rect, blocked: bool) {
        CAPTURE.with_borrow_mut(|capture| {
            let Some(capture) = capture else { return };
            let mut group = node(id, role, "", area.intersection(capture.bounds));
            group.parent = Some("screen".into());
            group.actionable = false;
            capture.nodes.push(group);
            capture.parent = id.into();
            capture.blocked = blocked;
            capture.transform = Projection {
                x: 0,
                y: 0,
                clip: capture.bounds,
            };
        });
    }

    /// Uses the final painted buffer. The host supplies session and frame numbers.
    pub fn finish(self, buffer: &Buffer) -> Snapshot {
        let mut capture = CAPTURE
            .with_borrow_mut(Option::take)
            .expect("active frame capture");
        let sensitive: Vec<_> = capture
            .nodes
            .iter()
            .filter(|node| node.sensitive)
            .map(|node| area(node.rect))
            .collect();
        let lines = buffer_lines(buffer, capture.bounds, &sensitive);
        let full_text = lines.join("\n");
        for node in &mut capture.nodes {
            node.text = if node.sensitive {
                "••••".into()
            } else if node.role == "heading" {
                node.name.clone()
            } else if area(node.rect) == capture.bounds {
                full_text.clone()
            } else {
                buffer_lines(buffer, area(node.rect), &sensitive).join("\n")
            };
        }
        attach_containers(&mut capture.nodes);
        capture_visible_text(&mut capture.nodes, buffer, capture.bounds, &sensitive);
        Snapshot {
            session: String::new(),
            number: 0,
            page: capture.page,
            width: capture.bounds.width,
            height: capture.bounds.height,
            animation_running: false,
            background_tasks_running: false,
            nodes: capture.nodes,
            lines,
        }
    }
}

impl Drop for FrameCapture {
    fn drop(&mut self) {
        CAPTURE.with_borrow_mut(|capture| *capture = None);
    }
}

pub fn enabled() -> bool {
    CAPTURE.with_borrow(Option::is_some)
}

pub fn node(id: impl Into<String>, role: &str, name: impl Into<String>, rect: Rect) -> Node {
    Node::new(
        id,
        role,
        name,
        shell_control::Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        },
    )
}

fn area(rect: shell_control::Rect) -> Rect {
    Rect::new(rect.x, rect.y, rect.width, rect.height)
}

/// Records a control using its existing stable component ID and painted rectangle.
pub fn record(mut node: Node) {
    CAPTURE.with_borrow_mut(|capture| {
        let Some(capture) = capture else { return };
        let original = area(node.rect);
        let projection = capture.transform;
        let x = i32::from(original.x) + projection.x;
        let y = i32::from(original.y) + projection.y;
        let left = x.max(i32::from(projection.clip.x));
        let top = y.max(i32::from(projection.clip.y));
        let right = (x + i32::from(original.width)).min(i32::from(projection.clip.right()));
        let bottom = (y + i32::from(original.height)).min(i32::from(projection.clip.bottom()));
        let clipped = Rect::new(
            left.max(0) as u16,
            top.max(0) as u16,
            (right - left).max(0) as u16,
            (bottom - top).max(0) as u16,
        );
        let base = if node.id.is_empty() {
            format!("{}/{}", capture.parent, node.role)
        } else {
            format!("{}/{}", capture.parent, node.id)
        };
        let mut id = base.clone();
        let mut suffix = 1;
        while capture.nodes.iter().any(|node| node.id == id) {
            suffix += 1;
            id = format!("{base}.{suffix}");
        }
        node.id = id;
        node.parent = Some(capture.parent.clone());
        node.rect = shell_control::Rect {
            x: clipped.x,
            y: clipped.y,
            width: clipped.width,
            height: clipped.height,
        };
        node.focused &= node.enabled && !capture.blocked;
        node.actionable &= node.enabled
            && !capture.blocked
            && clipped.width == original.width
            && clipped.height == original.height
            && !clipped.is_empty();
        if !node.actionable {
            node.actions.clear();
        }
        if node.sensitive {
            node.text = "••••".into();
        }
        if !clipped.is_empty() {
            capture.nodes.push(node);
        }
    });
}

pub(crate) fn control(
    id: &str,
    role: &str,
    name: &str,
    rect: Rect,
    state: crate::components::ComponentState,
    actions: &[&str],
) {
    if !enabled() {
        return;
    }
    let mut entry = node(id, role, name, rect);
    entry.focused = state.focused;
    entry.selected = state.selected;
    entry.enabled = !state.disabled;
    entry.actionable = !actions.is_empty();
    entry.actions = actions.iter().map(|action| (*action).into()).collect();
    record(entry);
}

/// Keeps children under a compound control without changing its render layout.
pub fn scope(id: &str, role: &str, name: &str, rect: Rect) -> CaptureScope {
    if !enabled() {
        return CaptureScope(None);
    }
    let previous = CAPTURE.with_borrow(|capture| {
        let capture = capture.as_ref().unwrap();
        (capture.parent.clone(), capture.nodes.len())
    });
    let mut group = node(id, role, name, rect);
    group.actionable = false;
    record(group);
    CAPTURE.with_borrow_mut(|capture| {
        let capture = capture.as_mut().unwrap();
        if capture.nodes.len() > previous.1 {
            capture.parent = capture.nodes.last().unwrap().id.clone();
        }
    });
    CaptureScope(Some(previous.0))
}

pub struct CaptureScope(Option<String>);

impl Drop for CaptureScope {
    fn drop(&mut self) {
        if let Some(parent) = self.0.take() {
            CAPTURE.with_borrow_mut(|capture| {
                if let Some(capture) = capture {
                    capture.parent = parent;
                }
            });
        }
    }
}

/// Applies the same translation and clipping as an off-screen render canvas.
pub fn transform(x: i32, y: i32, clip: Rect) -> CaptureTransform {
    let previous = CAPTURE.with_borrow_mut(|capture| {
        let capture = capture.as_mut()?;
        let previous = capture.transform;
        capture.transform = Projection {
            x: previous.x + x,
            y: previous.y + y,
            clip: clip.intersection(previous.clip),
        };
        Some(previous)
    });
    CaptureTransform(previous)
}

pub struct CaptureTransform(Option<Projection>);

impl Drop for CaptureTransform {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            CAPTURE.with_borrow_mut(|capture| {
                if let Some(capture) = capture {
                    capture.transform = previous;
                }
            });
        }
    }
}

fn is_container(node: &Node) -> bool {
    matches!(
        node.role.as_str(),
        "screen"
            | "page"
            | "group"
            | "surface"
            | "dialog"
            | "menu"
            | "list"
            | "table"
            | "row"
            | "tablist"
            | "navigation"
    )
}

fn contains(outer: shell_control::Rect, inner: shell_control::Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x.saturating_add(inner.width) <= outer.x.saturating_add(outer.width)
        && inner.y.saturating_add(inner.height) <= outer.y.saturating_add(outer.height)
}

fn attach_containers(nodes: &mut [Node]) {
    let parents: Vec<_> = nodes.iter().map(|node| node.parent.clone()).collect();
    for index in 1..nodes.len() {
        let parent = parents[index].as_deref();
        let container = nodes[..index]
            .iter()
            .enumerate()
            .filter(|(candidate_index, candidate)| {
                is_container(candidate)
                    && Some(candidate.id.as_str()) != parent
                    && parents[*candidate_index].as_deref() == parent
                    && contains(candidate.rect, nodes[index].rect)
            })
            .min_by_key(|(candidate_index, candidate)| {
                (
                    u32::from(candidate.rect.width) * u32::from(candidate.rect.height),
                    std::cmp::Reverse(*candidate_index),
                )
            });
        if let Some((_, container)) = container {
            nodes[index].parent = Some(container.id.clone());
        }
    }
}

fn buffer_lines(buffer: &Buffer, rect: Rect, sensitive: &[Rect]) -> Vec<String> {
    let rect = rect.intersection(buffer.area);
    (rect.y..rect.bottom())
        .map(|y| {
            let mut line = String::with_capacity(usize::from(rect.width));
            let mut x = rect.x;
            while x < rect.right() {
                let cell = &buffer[(x, y)];
                let symbol = cell.symbol();
                let width = cell.cell_width().max(1);
                if sensitive.iter().any(|area| area.contains((x, y).into())) {
                    line.push_str(&"*".repeat(usize::from(width.min(rect.right() - x))));
                } else if symbol.contains('\u{1b}') {
                    line.push_str(&" ".repeat(usize::from(width.min(rect.right() - x))));
                } else if x.saturating_add(width) <= rect.right() {
                    line.push_str(symbol);
                }
                // Ratatui reserves trailing cells of a wide glyph; they are not spaces.
                x = x.saturating_add(width);
            }
            line.truncate(line.trim_end().len());
            line
        })
        .collect()
}

fn capture_visible_text(nodes: &mut Vec<Node>, buffer: &Buffer, bounds: Rect, sensitive: &[Rect]) {
    let original_len = nodes.len();
    let row_width = usize::from(bounds.width);
    let index = |x: u16, y: u16| usize::from(y - bounds.y) * row_width + usize::from(x - bounds.x);
    let mut covered = vec![false; row_width * usize::from(bounds.height)];
    // Mark control coverage once instead of searching every control for each cell.
    for rect in nodes
        .iter()
        .filter(|node| !is_container(node))
        .map(|node| area(node.rect).intersection(bounds))
        .filter(|rect| !rect.is_empty())
    {
        for y in rect.y..rect.bottom() {
            let start = index(rect.x, y);
            covered[start..start + usize::from(rect.width)].fill(true);
        }
    }
    for y in bounds.y..bounds.bottom() {
        let mut x = bounds.x;
        while x < bounds.right() {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            let width = cell.cell_width().max(1);
            if covered[index(x, y)] || symbol.trim().is_empty() || symbol.contains('\u{1b}') {
                x = x.saturating_add(width);
                continue;
            }
            let start = x;
            let parent_index = nodes[..original_len]
                .iter()
                .enumerate()
                .filter(|(_, node)| is_container(node) && node.rect.contains(x, y))
                .min_by_key(|(index, node)| {
                    (
                        u32::from(node.rect.width) * u32::from(node.rect.height),
                        std::cmp::Reverse(*index),
                    )
                })
                .map(|(index, _)| index)
                .unwrap_or(0);
            let parent = &nodes[parent_index];
            let end = parent
                .rect
                .x
                .saturating_add(parent.rect.width)
                .min(bounds.right());
            x = x.saturating_add(width);
            while x < end && !covered[index(x, y)] {
                x = x.saturating_add(buffer[(x, y)].cell_width().max(1));
            }
            let rect = Rect::new(start, y, x.saturating_sub(start), 1);
            let text = buffer_lines(buffer, rect, sensitive).join("");
            if text
                .chars()
                .all(|c| c.is_whitespace() || "─│┌┐└┘╭╮╰╯├┤┬┴┼═║╔╗╚╝".contains(c))
            {
                continue;
            }
            let prefix = format!("{}/text.{}.", parent.id, y - parent.rect.y);
            let index = nodes[original_len..]
                .iter()
                .filter(|node| node.id.starts_with(&prefix))
                .count();
            let mut label = node(format!("{prefix}{index}"), "text", &text, rect);
            label.parent = Some(parent.id.clone());
            label.text = text;
            label.enabled = parent.enabled;
            label.actionable = false;
            nodes.push(label);
        }
    }
}
