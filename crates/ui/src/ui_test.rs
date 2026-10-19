//! A standalone catalogue of the public shared widgets, using only sample data.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::{
    InputEvent, Key, MouseButton, MouseEventKind, RenderContext, SystemStatusWidgetKind,
    SystemStatusWidgetSize, SystemStatusWidgetState, SystemStatusWidgetViewModel, components::*,
};

const PAGE_KEYS: [&str; 6] = [
    "ui-test-basic",
    "ui-test-selection",
    "ui-test-navigation",
    "ui-test-feedback",
    "ui-test-popups",
    "ui-test-progress",
];

pub struct UiTestGallery {
    pub page: usize,
    focus: usize,
    input: TextInput,
    list: List,
    table: DataTable,
    tabs: Tabs,
    picker: Picker,
    rail: NavRail,
    workspace_selected: usize,
    dialog: Dialog,
    menu: ContextMenu,
    palette: CommandPalette,
    notice: String,
}

pub struct UiTestLayout {
    pub usable: bool,
    body: Rect,
    samples: Vec<Rect>,
    footer: Vec<Rect>,
    controls: Vec<Rect>,
}

impl Default for UiTestGallery {
    fn default() -> Self {
        let labels = sample_labels();
        let mut input = TextInput::new("ui-test.input").with_placeholder(i18n::tr!("ui-test-type"));
        input.set_value(i18n::tr!("ui-test-input-value"));
        let mut gallery = Self {
            page: 0,
            focus: 7,
            input,
            list: List::new(
                "ui-test.list",
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| ListItem::new(format!("sample.{i}"), label).disabled(i == 2))
                    .collect(),
            ),
            table: DataTable::new(
                "ui-test.table",
                [i18n::tr!("ui-test-name"), i18n::tr!("ui-test-value")],
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| vec![label.clone(), (i * 25).to_string()]),
            ),
            tabs: Tabs::new(
                "ui-test.tabs",
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| TabItem::new(format!("tab.{i}"), label).disabled(i == 2))
                    .collect(),
            ),
            picker: Picker::new("ui-test.picker", labels.clone()),
            rail: NavRail::new(
                "ui-test.rail",
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| NavRailItem::new(format!("rail.{i}"), label))
                    .collect(),
            ),
            workspace_selected: 0,
            dialog: Dialog::new(
                "ui-test.dialog",
                i18n::tr!("ui-test-dialog"),
                i18n::tr!("ui-test-safe"),
                vec![
                    DialogAction::new("confirm", i18n::tr!("ui-test-confirm")),
                    DialogAction::new("cancel", i18n::tr!("ui-test-cancel")),
                ],
            ),
            menu: ContextMenu::new(
                "ui-test.menu",
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        ContextMenuItem::new(format!("menu.{i}"), label).disabled(i == 2)
                    })
                    .collect(),
            )
            .titled(i18n::tr!("ui-test-context-menu")),
            palette: CommandPalette::new(
                "ui-test.palette",
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        CommandPaletteCommand::new(format!("command.{i}"), label).disabled(i == 2)
                    })
                    .collect(),
            ),
            notice: i18n::tr!("ui-test-safe"),
        };
        gallery.dialog.open();
        gallery.menu.open();
        gallery.palette.open();
        gallery
    }
}

impl UiTestGallery {
    pub fn layout(&self, area: Rect) -> UiTestLayout {
        let body = Rect::new(
            area.x + 1,
            area.y + 3,
            area.width.saturating_sub(2),
            area.height.saturating_sub(6),
        );
        let footer: Vec<_> = Layout::horizontal([Constraint::Fill(1); 7])
            .split(Rect::new(
                area.x,
                area.bottom().saturating_sub(1),
                area.width,
                1,
            ))
            .to_vec();
        let columns = Layout::horizontal([Constraint::Fill(1); 2])
            .spacing(1)
            .split(body);
        let mut samples = Vec::new();
        for column in columns.iter() {
            samples.extend(
                Layout::vertical([Constraint::Fill(1); 2])
                    .spacing(1)
                    .split(*column)
                    .iter()
                    .copied(),
            );
        }
        let inner = |i: usize| samples[i].inner(ratatui::layout::Margin::new(1, 1));
        let controls = match self.page {
            0 => {
                let column = Layout::horizontal([Constraint::Fill(1); 2]).split(inner(0))[0];
                vec![Rect::new(column.x, column.y, column.width, 3), inner(1)]
            }
            1 => (0..4).map(inner).collect(),
            2 => {
                samples = columns.to_vec();
                let workspace =
                    SidebarWorkspaceLayout::new(inner_area(samples[1]), &sample_labels());
                let (list, _) = SidebarWorkspaceLayout::panes(workspace.content, false);
                let mut controls = vec![inner_area(samples[0])];
                controls.extend(workspace.navigation);
                controls.push(list);
                controls
            }
            4 => {
                samples[2] = columns[1];
                vec![
                    inner_area(samples[0]),
                    inner_area(samples[1]),
                    inner_area(samples[2]),
                ]
            }
            _ => Vec::new(),
        };
        UiTestLayout {
            usable: area.width >= 88 && area.height >= 30,
            body,
            samples,
            footer,
            controls,
        }
    }

    pub fn handle_event(&mut self, input: InputEvent, layout: &UiTestLayout) -> bool {
        if let InputEvent::Key(key) = &input {
            if !key.is_press_like() {
                return false;
            }
            match key.key {
                Key::Escape => return true,
                Key::Char('c') if key.modifiers.control || key.modifiers.ctrl => return true,
                Key::F(n @ 1..=6) => {
                    self.select_page(usize::from(n - 1));
                    return false;
                }
                Key::PageUp => {
                    self.select_page((self.page + 5) % 6);
                    return false;
                }
                Key::PageDown => {
                    self.select_page((self.page + 1) % 6);
                    return false;
                }
                Key::Tab | Key::BackTab => {
                    let count = 7 + layout.controls.len();
                    let backwards = key.key == Key::BackTab || key.modifiers.shift;
                    self.focus = (self.focus + if backwards { count - 1 } else { 1 }) % count;
                    return false;
                }
                Key::Enter | Key::Space if self.focus < 7 => {
                    if self.focus == 6 {
                        return true;
                    }
                    self.select_page(self.focus);
                    return false;
                }
                _ => {}
            }
        }
        if let InputEvent::Mouse(mouse) = &input {
            if matches!(mouse.kind, MouseEventKind::Click(MouseButton::Left)) {
                if let Some(index) = layout
                    .footer
                    .iter()
                    .position(|area| contains_point(*area, mouse.column(), mouse.row()))
                {
                    if index == 6 {
                        return true;
                    }
                    self.select_page(index);
                    return false;
                }
            }
            if layout.usable
                && matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::Click(MouseButton::Left)
                )
            {
                if let Some(index) = layout
                    .controls
                    .iter()
                    .position(|area| contains_point(*area, mouse.column(), mouse.row()))
                {
                    self.focus = 7 + index;
                }
            }
        }
        if !layout.usable {
            return false;
        }
        let Some(index) = self
            .focus
            .checked_sub(7)
            .filter(|index| *index < layout.controls.len())
        else {
            return false;
        };
        let area = layout.controls[index];
        self.input.set_focused(self.page == 0 && index == 1);
        self.list.set_focused(true);
        self.table.state.focused = true;
        self.tabs.set_focused(true);
        self.picker.state.focused = true;
        self.rail.state.focused = true;
        let result = match (self.page, index) {
            (0, 0) => {
                let mut button = Button::new("ui-test.normal", i18n::tr!("ui-test-normal"));
                button.set_focused(true);
                button.handle_event(input, area)
            }
            (0, 1) => self.input.handle_event(input, area),
            (1, 0) => self.list.handle_event(input, area),
            (1, 1) => self.table.handle_event(input, area),
            (1, 2) => self.tabs.handle_event(input, area),
            (1, 3) => self.picker.handle_event(input, area),
            (2, 0) => self.rail.handle_event(input, area),
            (2, 1..=3) => {
                if matches!(input, InputEvent::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Click(MouseButton::Left)))
                    || matches!(&input, InputEvent::Key(key) if matches!(key.key, Key::Enter | Key::Space))
                {
                    self.workspace_selected = index - 1;
                }
                ComponentEvent::None
            }
            (2, 4) => self.list.handle_event(input, area),
            (4, 0) => self.dialog.handle_event(input, area),
            (4, 1) => self.menu.handle_event(input, area),
            (4, 2) => self.palette.handle_event(input, area),
            _ => ComponentEvent::None,
        };
        if matches!(result, ComponentEvent::Activated(_)) {
            self.notice = i18n::tr!("ui-test-activated");
        }
        // Popup specimens stay visible after activation so their appearance can be compared.
        self.dialog.open();
        self.menu.open();
        self.palette.open();
        false
    }

    fn select_page(&mut self, page: usize) {
        self.page = page;
        self.focus = if matches!(page, 3 | 5) { page } else { 7 };
    }

    pub fn render(&self, frame: &mut Frame<'_>, context: &RenderContext) -> UiTestLayout {
        let area = frame.area();
        let layout = self.layout(area);
        let theme = context.compatibility_theme();
        Surface::new().render_frame(frame, area, context);
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-test-title")).style(theme.title_style()),
            Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
        );
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-test-keys"))
                .style(theme.muted_style())
                .wrap(Wrap { trim: false }),
            Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), 2),
        );
        if layout.usable {
            match self.page {
                0 => self.render_basic(frame, &layout, context),
                1 => self.render_selection(frame, &layout, context),
                2 => self.render_navigation(frame, &layout, context),
                3 => self.render_feedback(frame, &layout, context),
                4 => {
                    let mut dialog = self.dialog.clone();
                    dialog.state.focused = self.focus == 7;
                    dialog.render_frame(frame, layout.controls[0], &theme);
                    let mut menu = self.menu.clone();
                    menu.state.focused = self.focus == 8;
                    menu.render_frame(frame, layout.controls[1], &theme);
                    let mut palette = self.palette.clone();
                    palette.state.focused = self.focus == 9;
                    palette.render_frame(frame, layout.controls[2], &theme);
                }
                5 => self.render_progress(frame, &layout, context),
                _ => unreachable!(),
            }
            frame.render_widget(
                Paragraph::new(self.notice.as_str()).style(theme.muted_style()),
                Rect::new(
                    area.x + 1,
                    area.bottom().saturating_sub(2),
                    area.width.saturating_sub(2),
                    1,
                ),
            );
        } else {
            EmptyState::new(i18n::tr!("ui-test-small")).render_frame(frame, layout.body, context);
        }
        for (i, rect) in layout.footer.iter().enumerate() {
            let label = if i == 6 {
                i18n::tr!("ui-test-exit")
            } else {
                format!("{} F{}", i18n::tr!(PAGE_KEYS[i]), i + 1)
            };
            let mut button =
                Button::new(format!("ui-test.page.{i}"), label).with_bracketed_label(false);
            button.state.selected = i == self.page;
            button.set_focused(i == self.focus);
            button.render_borderless_frame(frame, *rect, &theme);
        }
        layout
    }

    fn render_basic(&self, frame: &mut Frame<'_>, layout: &UiTestLayout, context: &RenderContext) {
        let theme = context.compatibility_theme();
        let area = caption(frame, layout.samples[0], "ui-test-buttons", context);
        let columns = Layout::horizontal([Constraint::Fill(1); 2]).split(area);
        let states = [
            "ui-test-normal",
            "ui-test-focused",
            "ui-test-hovered",
            "ui-test-pressed",
            "ui-test-selected",
            "ui-test-disabled",
        ];
        for (i, key) in states.iter().enumerate() {
            let rect = Rect::new(
                columns[i % 2].x,
                area.y + (i / 2) as u16 * 3,
                columns[i % 2].width,
                3,
            )
            .intersection(area);
            let mut button = Button::new(
                if i == 0 {
                    "ui-test.normal".to_owned()
                } else {
                    format!("ui-test.state.{i}")
                },
                i18n::tr!(*key),
            );
            button.state = ComponentState {
                focused: i == 1,
                hovered: i == 2,
                active: i == 3,
                selected: i == 4,
                disabled: i == 5,
            };
            if i == 0 {
                button.set_focused(self.focus == 7);
                button.render_frame(frame, rect, &theme);
            } else {
                let mut specimen_theme = theme.clone();
                specimen_theme.buttons = None;
                button.render_frame(frame, rect, &specimen_theme);
            }
        }
        let area = caption(frame, layout.samples[1], "ui-test-text-input", context);
        let mut input = self.input.clone();
        input.set_focused(self.focus == 8);
        input.render_frame(frame, area, &theme);
        let surface = Surface::new()
            .titled(i18n::tr!("ui-test-surface"))
            .bordered(true)
            .raised(true);
        surface.render_frame(frame, layout.samples[2], context);
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-test-surface-detail"))
                .style(theme.body_style())
                .wrap(Wrap { trim: false }),
            surface.inner(layout.samples[2]),
        );
        let panel = Panel::new(i18n::tr!("ui-test-panel"));
        panel.render_frame(frame, layout.samples[3], context);
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-test-wide-text"))
                .style(theme.body_style())
                .wrap(Wrap { trim: false }),
            panel.inner(layout.samples[3]),
        );
    }

    fn render_selection(
        &self,
        frame: &mut Frame<'_>,
        layout: &UiTestLayout,
        context: &RenderContext,
    ) {
        let theme = context.compatibility_theme();
        for (i, key) in [
            "ui-test-list",
            "ui-test-table",
            "ui-test-tabs",
            "ui-test-picker",
        ]
        .iter()
        .enumerate()
        {
            caption(frame, layout.samples[i], key, context);
        }
        let mut list = self.list.clone();
        list.set_focused(self.focus == 7);
        list.render_frame(frame, layout.controls[0], &theme);
        let mut table = self.table.clone();
        table.state.focused = self.focus == 8;
        table.render_frame(frame, layout.controls[1], context);
        let mut tabs = self.tabs.clone();
        tabs.set_focused(self.focus == 9);
        tabs.render_frame(frame, layout.controls[2], &theme);
        let mut picker = self.picker.clone();
        picker.state.focused = self.focus == 10;
        picker.render_frame(frame, layout.controls[3], context);
        let area = layout.samples[0];
        Scrollbar::new(20, 5, self.list.selected_index().unwrap_or(0)).render_frame(
            frame,
            Rect::new(area.right() - 1, area.y + 1, 1, area.height - 2),
            context,
        );
        Scrollbar::new(20, 5, self.list.selected_index().unwrap_or(0))
            .orientation(ScrollbarOrientation::HorizontalBottom)
            .render_frame(
                frame,
                Rect::new(area.x + 1, area.bottom() - 1, area.width - 2, 1),
                context,
            );
    }

    fn render_navigation(
        &self,
        frame: &mut Frame<'_>,
        layout: &UiTestLayout,
        context: &RenderContext,
    ) {
        caption(frame, layout.samples[0], "ui-test-nav-rail", context);
        let mut rail = self.rail.clone();
        rail.state.focused = self.focus == 7;
        rail.render_frame(frame, layout.controls[0], context);
        let area = caption(frame, layout.samples[1], "ui-test-workspace", context);
        let labels = sample_labels();
        let workspace = SidebarWorkspaceLayout::new(area, &labels);
        let buttons: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let mut button = Button::new(format!("ui-test.workspace.{i}"), label);
                button.set_focused(self.focus == 8 + i);
                button
            })
            .collect();
        workspace.render_navigation(frame, &buttons, self.workspace_selected, context);
        let (list_area, details) = SidebarWorkspaceLayout::panes(workspace.content, false);
        let mut list = self.list.clone();
        list.set_focused(self.focus == 11);
        list.render_frame(frame, list_area, &context.compatibility_theme());
        EmptyState::new(labels[self.workspace_selected].clone())
            .detail(i18n::tr!("ui-test-safe"))
            .render_frame(frame, details, context);
    }

    fn render_feedback(
        &self,
        frame: &mut Frame<'_>,
        layout: &UiTestLayout,
        context: &RenderContext,
    ) {
        let theme = context.compatibility_theme();
        let area = caption(frame, layout.samples[0], "ui-test-empty", context);
        EmptyState::new(i18n::tr!("ui-test-no-items"))
            .detail(i18n::tr!("ui-test-safe"))
            .render_frame(frame, area, context);
        let area = caption(frame, layout.samples[1], "ui-test-skeleton", context);
        Skeleton.render_frame(
            frame,
            Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2)),
            context,
        );
        let spans = [
            CommandStatus::Pending,
            CommandStatus::Succeeded,
            CommandStatus::Failed,
        ]
        .into_iter()
        .zip(["ui-test-pending", "ui-test-succeeded", "ui-test-failed"])
        .flat_map(|(status, key)| {
            [
                status.span(&theme),
                Span::raw(format!(" {}  ", i18n::tr!(key))),
            ]
        })
        .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
        for (i, (tone, key)) in [
            (ToastTone::Info, "ui-test-info"),
            (ToastTone::Success, "ui-test-success"),
            (ToastTone::Warning, "ui-test-warning"),
            (ToastTone::Danger, "ui-test-danger"),
        ]
        .iter()
        .enumerate()
        {
            let area = layout.samples[2 + i / 2];
            let mut still = context.clone();
            still.motion.reduced_motion = true;
            Toast::new(i18n::tr!(*key), *tone, still.motion).render_frame(
                frame,
                Rect::new(area.x, area.y + (i % 2) as u16 * 3, area.width, 3),
                &still,
            );
        }
    }

    fn render_progress(
        &self,
        frame: &mut Frame<'_>,
        layout: &UiTestLayout,
        context: &RenderContext,
    ) {
        let area = caption(frame, layout.samples[0], "ui-test-gauge", context);
        for (i, ratio) in [0.0, 0.45, 1.0].iter().enumerate() {
            frame.render_widget(
                ProgressGauge::new(
                    format!("{}%", (ratio * 100.0) as u16),
                    *ratio,
                    context.theme.accent,
                    context.theme.surface,
                    &context.theme,
                ),
                Rect::new(area.x, area.y + i as u16 * 2, area.width, 1),
            );
        }
        let model = SystemStatusWidgetViewModel {
            kind: SystemStatusWidgetKind::Cpu,
            size: SystemStatusWidgetSize::Small,
            state: SystemStatusWidgetState::Ready,
            primary: "45%".into(),
            secondary: vec![i18n::tr!("ui-test-sample-data")],
            progress_percent: Some(45),
            ..Default::default()
        };
        let area = caption(frame, layout.samples[1], "ui-test-metric", context);
        MetricCard::new(&model).render_frame(frame, area, context);
        let model = UpdateActivityViewModel {
            download: UpdateMeterViewModel {
                percent: Some(75),
                display_basis_points: None,
                label: i18n::tr!("ui-test-download").into(),
            },
            compilation: UpdateMeterViewModel {
                percent: Some(30),
                display_basis_points: None,
                label: i18n::tr!("ui-test-build").into(),
            },
            output: vec![i18n::tr!("ui-test-sample-data"), i18n::tr!("ui-test-safe")],
        };
        let area = caption(
            frame,
            layout.samples[2].union(layout.samples[3]),
            "ui-test-update",
            context,
        );
        UpdateActivity::new(&model).render_scrolled(frame, area, i32::from(area.y), context);
    }
}

fn sample_labels() -> Vec<String> {
    ["ui-test-first", "ui-test-second", "ui-test-third"]
        .iter()
        .map(|key| i18n::tr!(*key))
        .collect()
}

fn inner_area(area: Rect) -> Rect {
    area.inner(ratatui::layout::Margin::new(1, 1))
}

fn caption(frame: &mut Frame<'_>, area: Rect, key: &str, context: &RenderContext) -> Rect {
    let surface = Surface::new().titled(i18n::tr!(key)).bordered(true);
    surface.render_frame(frame, area, context);
    surface.inner(area)
}
