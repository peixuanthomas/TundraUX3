use ratatui::layout::Rect;

use crate::input::{InputEvent, MouseEvent, Point, RoutedEvent, UiId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HitKind {
    Button,
    ListItem(usize),
    TextInput,
    Dialog,
    ContextMenu,
    Tab(usize),
    Backdrop,
    Custom(String),
}

impl HitKind {
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }
}

/// Semantic plane used to resolve overlapping UI hit targets.
///
/// Higher layers always receive input before lower layers. Within a layer,
/// HitTarget::z_index and registration order retain their existing behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HitLayer {
    #[default]
    AppContent,
    AppOverlay,
    ShellChrome,
    ShellModal,
}

impl HitLayer {
    pub const fn z_index(self) -> i32 {
        match self {
            Self::AppContent => 0,
            Self::AppOverlay => 1,
            Self::ShellChrome => 2,
            Self::ShellModal => 3,
        }
    }
}

pub type HitTargetKind = HitKind;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitTarget {
    pub id: UiId,
    pub rect: Rect,
    pub kind: HitKind,
    pub layer: HitLayer,
    pub z_index: i32,
    pub enabled: bool,
}

impl HitTarget {
    pub fn new(id: impl Into<UiId>, rect: Rect, kind: HitKind) -> Self {
        Self {
            id: id.into(),
            rect,
            kind,
            layer: HitLayer::default(),
            z_index: 0,
            enabled: true,
        }
    }

    pub fn with_layer(mut self, layer: HitLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn contains(&self, point: Point) -> bool {
        rect_contains(self.rect, point)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HitEntry {
    target: HitTarget,
    order: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HitMap {
    entries: Vec<HitEntry>,
    next_order: usize,
}

impl HitMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, target: HitTarget) {
        self.insert(target);
    }

    pub fn insert(&mut self, target: HitTarget) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.target.id == target.id)
        {
            entry.target = target;
            return;
        }

        self.entries.push(HitEntry {
            target,
            order: self.next_order,
        });
        self.next_order = self.next_order.saturating_add(1);
    }

    pub fn remove(&mut self, id: &UiId) -> bool {
        let initial_len = self.entries.len();
        self.entries.retain(|entry| &entry.target.id != id);
        self.entries.len() != initial_len
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_order = 0;
    }

    pub fn hit(&self, point: Point) -> Option<&HitTarget> {
        self.entries
            .iter()
            .filter(|entry| entry.target.enabled && entry.target.contains(point))
            .max_by(|left, right| {
                left.target
                    .layer
                    .cmp(&right.target.layer)
                    .then(left.target.z_index.cmp(&right.target.z_index))
                    .then(left.order.cmp(&right.order))
            })
            .map(|entry| &entry.target)
    }

    pub fn hit_test(&self, column: u16, row: u16) -> Option<&HitTarget> {
        self.hit(Point::new(column, row))
    }

    pub fn route_mouse(&self, mouse: MouseEvent) -> RoutedEvent {
        let event = InputEvent::Mouse(mouse);
        self.hit(mouse.position)
            .map(|target| RoutedEvent::hit(event.clone(), target.id.clone()))
            .unwrap_or_else(|| RoutedEvent::unmatched(event))
    }

    pub fn route_input(&self, event: InputEvent) -> RoutedEvent {
        match event {
            InputEvent::Mouse(mouse) => self.route_mouse(mouse),
            other => RoutedEvent::unmatched(other),
        }
    }

    pub fn targets(&self) -> impl Iterator<Item = &HitTarget> {
        self.entries.iter().map(|entry| &entry.target)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn rect_contains(rect: Rect, point: Point) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);

    point.column >= rect.x && point.column < right && point.row >= rect.y && point.row < bottom
}
