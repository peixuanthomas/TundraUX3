//! Debug-only composition of existing components. Runtime and animation state
//! live in Shell; this module owns the shared layout used for drawing and input.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};

use crate::{
    RenderContext,
    components::{Button, Dialog, List, ProgressGauge, Surface, TextInput},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiStyleVersion {
    Glacier,
    Tea,
    Spring,
}

impl UiStyleVersion {
    pub const ALL: [Self; 3] = [Self::Glacier, Self::Tea, Self::Spring];

    pub const fn number(self) -> u8 {
        match self {
            Self::Glacier => 1,
            Self::Tea => 2,
            Self::Spring => 3,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Glacier => "Glacier / native widgets",
            Self::Tea => "Tea / minimal flow",
            Self::Spring => "Spring / animated cards",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Glacier => "Bordered panels, stepped progress, tachyonfx sweep.",
            Self::Tea => "Borderless list and form, message-driven cubic progress.",
            Self::Spring => "Raised cards, retargetable spring progress, soft reveal.",
        }
    }

    pub const fn technique(self) -> &'static str {
        match self {
            Self::Glacier => {
                "Existing List + TextInput + Button + Dialog; Ratatui Gauge; tachyonfx after widget rendering."
            }
            Self::Tea => {
                "Bubble Tea-inspired Model / Message / Update / View. Targets change immediately; the displayed value eases over 450 ms."
            }
            Self::Spring => {
                "Position + velocity + target persist across frames. A damped spring follows new targets without restarting its velocity."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PreviewLayout {
    pub usable: bool,
    pub header: Rect,
    pub list: Rect,
    pub details: Rect,
    pub input: Rect,
    pub progress: Rect,
    pub replay: Rect,
    pub inspect: Rect,
    pub footer: Rect,
    pub dialog: Rect,
}

pub fn preview_layout(area: Rect, version: UiStyleVersion) -> PreviewLayout {
    if area.width < 60 || area.height < 24 {
        return PreviewLayout::default();
    }
    let width = match version {
        UiStyleVersion::Tea => 78,
        _ => 112,
    };
    let [body] = Layout::horizontal([Constraint::Max(width)])
        .flex(Flex::Center)
        .margin(1)
        .areas(area);
    let [header, content, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(15),
        Constraint::Length(3),
    ])
    .areas(body);
    let (mut list, detail) = if version == UiStyleVersion::Tea && area.height >= 30 {
        let [list, detail] =
            Layout::vertical([Constraint::Length(5), Constraint::Min(10)]).areas(content);
        (list, detail)
    } else {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                .spacing(2)
                .areas(content);
        (list, detail)
    };
    let [mut details, mut input, mut progress, mut actions] = Layout::vertical([
        if version == UiStyleVersion::Tea {
            Constraint::Length(3)
        } else {
            Constraint::Min(3)
        },
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .spacing(1)
    .flex(Flex::Start)
    .areas(detail);
    if version == UiStyleVersion::Spring && area.height >= 30 && area.width >= 80 {
        let [cards, field, meter, buttons] = Layout::vertical([
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .spacing(1)
        .flex(Flex::Start)
        .areas(content);
        [list, details] =
            Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                .spacing(2)
                .areas(cards);
        input = field;
        progress = meter;
        actions = buttons;
    }
    let [replay, inspect] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)])
        .spacing(2)
        .areas(actions);
    let [dialog_column] = Layout::horizontal([Constraint::Max(58)])
        .flex(Flex::Center)
        .areas(area);
    let [dialog] = Layout::vertical([Constraint::Length(12)])
        .flex(Flex::Center)
        .areas(dialog_column);
    PreviewLayout {
        usable: true,
        header,
        list,
        details,
        input,
        progress,
        replay,
        inspect,
        footer,
        dialog,
    }
}

pub struct PreviewView<'a> {
    pub version: UiStyleVersion,
    pub list: &'a List,
    pub input: &'a TextInput,
    pub replay: &'a Button,
    pub inspect: &'a Button,
    pub dialog: &'a Dialog,
    pub displayed: f64,
    pub target: f64,
    pub running: bool,
}

pub fn render_preview(frame: &mut Frame<'_>, view: &PreviewView<'_>, context: &RenderContext) {
    let area = frame.area();
    let theme = context.compatibility_theme();
    let tokens = context.theme;
    Surface::new().render_frame(frame, area, context);
    let layout = preview_layout(area, view.version);
    if !layout.usable {
        frame.render_widget(Paragraph::new("UI style preview needs at least 60 x 24 cells.\nResize to continue. Esc / Ctrl-C exits.")
            .style(theme.body_style()).wrap(Wrap { trim: false }), area);
        return;
    }
    let [title, subtitle] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(layout.header);
    frame.render_widget(
        Paragraph::new(format!(
            "{} / 03    {}",
            view.version.number(),
            view.version.title()
        ))
        .style(
            Style::default()
                .fg(tokens.accent)
                .add_modifier(Modifier::BOLD),
        ),
        title,
    );
    frame.render_widget(
        Paragraph::new(view.version.description())
            .style(theme.muted_style())
            .wrap(Wrap { trim: false }),
        subtitle,
    );

    if view.version == UiStyleVersion::Tea {
        view.list
            .render_borderless_frame(frame, layout.list, &theme);
        view.input
            .render_borderless_frame(frame, layout.input, &theme);
        view.replay
            .render_borderless_frame(frame, layout.replay, &theme);
        view.inspect
            .render_borderless_frame(frame, layout.inspect, &theme);
    } else {
        view.list.render_frame(frame, layout.list, &theme);
        view.input.render_frame(frame, layout.input, &theme);
        view.replay.render_frame(frame, layout.replay, &theme);
        view.inspect.render_frame(frame, layout.inspect, &theme);
    }

    let selected = view
        .list
        .selected_item()
        .map(|item| item.label.as_str())
        .unwrap_or("Preview");
    let surface = Surface::new()
        .titled(format!(" {selected} "))
        .bordered(view.version != UiStyleVersion::Tea)
        .raised(view.version == UiStyleVersion::Spring);
    surface.render_frame(frame, layout.details, context);
    frame.render_widget(
        Paragraph::new(view.version.technique())
            .style(theme.body_style())
            .wrap(Wrap { trim: false }),
        surface.inner(layout.details),
    );

    let meter = Surface::new()
        .titled(" Simulated task ")
        .bordered(view.version != UiStyleVersion::Tea)
        .raised(view.version == UiStyleVersion::Spring);
    meter.render_frame(frame, layout.progress, context);
    let status = if view.running { "Running" } else { "Settled" };
    let progress_area = if view.version == UiStyleVersion::Tea {
        Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .areas::<1>(layout.progress)[0]
    } else {
        meter.inner(layout.progress)
    };
    frame.render_widget(
        ProgressGauge::new(
            format!(
                "{status}  {:.0}%  / target {:.0}%",
                view.displayed * 100.0,
                view.target * 100.0
            ),
            view.displayed,
            tokens.accent,
            tokens.raised,
            &tokens,
        ),
        progress_area,
    );

    let motion = if context.motion.reduced_motion {
        "off"
    } else {
        "on"
    };
    frame.render_widget(Paragraph::new(format!(
        "F1-F3 style | F4 motion {motion} | F5 replay | Esc back/exit\nTab/Shift-Tab focus | Arrows/mouse select | Enter activate\nType in the field. Demo only; no preferences are saved."
    )).style(theme.muted_style()), layout.footer);
    view.dialog.render_frame(frame, layout.dialog, &theme);
}
