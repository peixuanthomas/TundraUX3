use super::{
    ScreenKeyboardAction as Action, ScreenKeyboardLayout, ScreenKeyboardSection, append_row,
};
use ratatui::layout::{Constraint, Layout, Rect};

type KeyRow = Vec<(Action, u16)>;

fn letters() -> [KeyRow; 5] {
    use Action::*;
    let mut numbers = "`1234567890-="
        .chars()
        .map(|c| (Character(c), 4))
        .collect::<KeyRow>();
    numbers.push((Backspace, 8));
    let mut qwerty = vec![(Tab, 6)];
    qwerty.extend("qwertyuiop[]".chars().map(|c| {
        (
            if c.is_ascii_alphabetic() {
                Letter(c)
            } else {
                Character(c)
            },
            4,
        )
    }));
    qwerty.push((Character('\\'), 6));
    let mut home = vec![(CapsLock, 7)];
    home.extend("asdfghjkl".chars().map(|c| (Letter(c), 4)));
    home.extend([(Character(';'), 4), (Character('\''), 4), (Enter, 9)]);
    let mut shift = vec![(Shift, 9)];
    shift.extend("zxcvbnm".chars().map(|c| (Letter(c), 4)));
    shift.extend([
        (Character(','), 4),
        (Character('.'), 4),
        (Character('/'), 4),
        (RightShift, 11),
    ]);
    [
        numbers,
        qwerty,
        home,
        shift,
        vec![
            (LeftCtrl, 6),
            (Alt, 6),
            (Space, 30),
            (RightAlt, 6),
            (RightCtrl, 6),
        ],
    ]
}

fn required_width(keys: &[(Action, u16)], bordered: bool) -> u16 {
    keys.iter()
        .map(|(key, _)| key.minimum_width(bordered))
        .sum::<u16>()
        + keys.len().saturating_sub(1) as u16
}

pub(super) fn append_keys(
    layout: &mut ScreenKeyboardLayout,
    bottom: u16,
    heights: [u16; 6],
    input: bool,
    section: ScreenKeyboardSection,
) {
    use Action::*;
    let mut letters = letters();
    if input {
        letters[4].insert(0, (Paste, 6));
    }
    let bordered = layout.bordered_buttons;
    let nav_width = [
        Insert, Delete, Home, End, PageUp, PageDown, Up, Down, Left, Right,
    ]
    .into_iter()
    .map(|key| key.minimum_width(bordered))
    .max()
    .unwrap()
        * 3
        + 2;
    let hide_width = if input {
        ToggleKeyboard.minimum_width(bordered) + 1
    } else {
        0
    };
    let mut functions = vec![(Escape, 2)];
    functions.extend((1..=12).map(|number| (Function(number), 1)));
    let main_width = letters
        .iter()
        .chain(std::iter::once(&functions))
        .map(|row| required_width(row, bordered))
        .max()
        .unwrap()
        .max(104);
    layout.compact = layout.keyboard.width < main_width + nav_width + hide_width + 2;
    let compact_functions = layout.compact && section == ScreenKeyboardSection::Functions;
    let row_count = if layout.compact && !compact_functions {
        5
    } else {
        6
    };
    let area = layout.keyboard;
    let spare = bottom
        .saturating_sub(area.y)
        .saturating_sub(heights[..row_count].iter().sum());
    let mut preceding = 0_u32;
    let rows = (0..row_count)
        .map(|index| {
            let y = u32::from(area.y)
                + preceding
                + index as u32 * u32::from(spare) / (row_count - 1) as u32;
            preceding += u32::from(heights[index]);
            Rect::new(
                area.x,
                y.min(u32::from(bottom)) as u16,
                area.width,
                heights[index],
            )
            .intersection(Rect::new(
                area.x,
                area.y,
                area.width,
                bottom.saturating_sub(area.y),
            ))
        })
        .collect::<Vec<_>>();
    if layout.compact {
        letters[4].push((
            if compact_functions {
                Letters
            } else {
                Functions
            },
            6,
        ));
        if input {
            letters[4].push((ToggleKeyboard, 6));
        }
        if compact_functions {
            append_functions(layout, rows[0]);
            let width = nav_width.min(area.width);
            let nav = Rect::new(area.right() - width, area.y, width, area.height);
            append_navigation(layout, nav, &rows, 3, 4);
            append_row(&mut layout.buttons, rows[5], &letters[4], bordered);
        } else {
            for (row, keys) in rows.iter().zip(&letters) {
                append_row(&mut layout.buttons, *row, keys, bordered);
            }
        }
    } else {
        let main = Rect::new(
            area.x,
            area.y,
            area.width - nav_width - hide_width - 2,
            area.height,
        );
        let nav = Rect::new(main.right() + 2, area.y, nav_width, area.height);
        append_functions(layout, Rect::new(main.x, rows[0].y, main.width, rows[0].height));
        for (row, keys) in rows[1..].iter().zip(&letters) {
            append_row(
                &mut layout.buttons,
                Rect::new(main.x, row.y, main.width, row.height),
                keys,
                bordered,
            );
        }
        append_navigation(layout, nav, &rows, 4, 5);
        if input {
            let row = rows[5];
            append_row(
                &mut layout.buttons,
                Rect::new(nav.right() + 1, row.y, hide_width - 1, row.height),
                &[(ToggleKeyboard, 1)],
                bordered,
            );
        }
    }
}

fn append_functions(layout: &mut ScreenKeyboardLayout, area: Rect) {
    let groups = Layout::horizontal([
        Constraint::Fill(2), Constraint::Length(1),
        Constraint::Fill(4), Constraint::Length(1),
        Constraint::Fill(4), Constraint::Length(1), Constraint::Fill(4),
    ]).split(area);
    append_row(&mut layout.buttons, groups[0], &[(Action::Escape, 1)], layout.bordered_buttons);
    for (area, first) in [groups[2], groups[4], groups[6]].into_iter().zip([1, 5, 9]) {
        let keys = (first..first + 4).map(|number| (Action::Function(number), 1)).collect::<KeyRow>();
        append_row(&mut layout.buttons, area, &keys, layout.bordered_buttons);
    }
}

fn append_navigation(
    layout: &mut ScreenKeyboardLayout,
    area: Rect,
    rows: &[Rect],
    up_row: usize,
    arrows_row: usize,
) {
    use Action::*;
    let columns = Layout::horizontal([Constraint::Fill(1); 3])
        .spacing(1)
        .split(area);
    for (index, keys) in [
        (1, [Insert, Home, PageUp]),
        (2, [Delete, End, PageDown]),
        (arrows_row, [Left, Down, Right]),
    ] {
        for (column, key) in columns.iter().zip(keys) {
            let row = rows[index];
            append_row(
                &mut layout.buttons,
                Rect::new(column.x, row.y, column.width, row.height),
                &[(key, 1)],
                layout.bordered_buttons,
            );
        }
    }
    let row = rows[up_row];
    append_row(
        &mut layout.buttons,
        Rect::new(columns[1].x, row.y, columns[1].width, row.height),
        &[(Up, 1)],
        layout.bordered_buttons,
    );
}
