use super::layout::to_u16;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayRun {
    pub(super) text: DisplayText,
    pub(super) style: EditorRenderSpan,
    pub(super) source: DisplaySource,
    pub(super) rich: DisplayRich,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DisplayText {
    Owned(String),
    Shared(Arc<str>),
    /// A zero-copy slice of `EditorViewModel::source`.
    SourceRange(EditorSourceRange),
}

impl DisplayText {
    pub(super) fn resolve<'a>(&'a self, source: Option<&'a str>) -> &'a str {
        match self {
            Self::Owned(text) => text,
            Self::Shared(text) => text,
            Self::SourceRange(range) => source
                .and_then(|source| source.get(range.start..range.end))
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplaySource {
    Unmapped,
    Range(EditorSourceRange),
    Virtual(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayRich {
    Unmapped,
    Range(RichRange),
    Virtual(RichPosition),
}

impl DisplayRun {
    pub(super) fn unmapped(text: impl Into<String>, style: EditorRenderSpan) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: DisplaySource::Unmapped,
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn virtual_text(
        text: impl Into<String>,
        style: EditorRenderSpan,
        byte_offset: Option<usize>,
    ) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: byte_offset
                .map(DisplaySource::Virtual)
                .unwrap_or(DisplaySource::Unmapped),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn source_range(range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::SourceRange(range),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn shared_source(text: Arc<str>, range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::Shared(text),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn with_virtual_rich(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Virtual(position);
        }
        self
    }

    /// Marks an otherwise zero-width display run as an editable Rich cursor
    /// boundary. This differs from a virtual decoration: the position is a
    /// real boundary in the document model even though it has no terminal
    /// cell of its own (for example, immediately before or after a soft
    /// break).
    pub(super) fn with_editable_rich_boundary(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Range(RichRange::between(position, position));
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayLine {
    pub(super) runs: Vec<DisplayRun>,
    pub(super) block_index: Option<usize>,
    pub(super) no_wrap: bool,
    /// Absolute display column of the first run. Non-zero only for a
    /// horizontally clipped Source viewport.
    pub(super) column_start: usize,
}

/// Returns whether the Rich document fits without a vertical scrollbar.
///
/// This mirrors the line-counting semantics of [`block_lines`] without
/// constructing `DisplayRun`s. Wrapped blocks stop as soon as the supplied
/// height is exceeded, so a large document normally measures only a bounded
/// prefix before being flattened once at the final canvas width.
pub(super) fn rich_document_fits_height(
    model: &EditorViewModel,
    width: usize,
    height: usize,
) -> bool {
    let blocks = model.render_blocks();
    if blocks.is_empty() {
        return height >= 1;
    }

    let width = width.max(1);
    let mut line_count = 0usize;
    for block in blocks {
        let remaining = height.saturating_sub(line_count);
        let Some(block_line_count) = rich_block_line_count_up_to(block, width, remaining) else {
            return false;
        };
        line_count = line_count.saturating_add(block_line_count);
    }
    line_count <= height
}

pub(super) fn rich_block_line_count_up_to(
    block: &EditorRenderBlock,
    width: usize,
    limit: usize,
) -> Option<usize> {
    match block {
        EditorRenderBlock::Paragraph(spans) | EditorRenderBlock::Heading { spans, .. } => {
            wrapped_line_count_up_to("", spans, width, limit)
        }
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::Footnote { label, spans } => {
            let prefix = format!("[^{label}] ");
            wrapped_line_count_up_to(&prefix, spans, width, limit)
        }
        EditorRenderBlock::CodeBlock { lines, .. } => {
            fixed_line_count_up_to(lines.len().saturating_add(2), limit)
        }
        EditorRenderBlock::Table { header, rows, .. }
        | EditorRenderBlock::RichTable { header, rows, .. } => {
            let has_columns = !header.is_empty() || rows.iter().any(|row| !row.is_empty());
            let line_count = if !has_columns {
                1
            } else {
                rows.len()
                    .saturating_add(2)
                    .saturating_add(usize::from(!header.is_empty()).saturating_mul(2))
            };
            fixed_line_count_up_to(line_count, limit)
        }
        EditorRenderBlock::HorizontalRule
        | EditorRenderBlock::RawHtml(_)
        | EditorRenderBlock::Image { .. }
        | EditorRenderBlock::Blank => fixed_line_count_up_to(1, limit),
    }
}

pub(super) fn fixed_line_count_up_to(line_count: usize, limit: usize) -> Option<usize> {
    (line_count <= limit).then_some(line_count)
}

/// Counts the lines produced by [`wrap_runs`] without creating one owned
/// `DisplayRun` per grapheme. `None` means the count exceeded `limit`.
pub(super) fn wrapped_line_count_up_to(
    prefix: &str,
    spans: &[EditorRenderSpan],
    width: usize,
    limit: usize,
) -> Option<usize> {
    let prefix_width = Span::raw(terminal_safe_text(prefix)).width();
    let mut measure = WrappedLineMeasure {
        width,
        limit,
        prefix_width,
        line_count: 0,
        current_width: prefix_width,
        current_has_runs: !prefix.is_empty(),
        continuation_has_runs: prefix_width > 0,
    };

    for span in spans {
        // `mapped_text_runs` emits one empty run for an empty span. It affects
        // whether a final empty visual line exists even though it has no width.
        if span.text.is_empty() {
            measure.current_has_runs = true;
            continue;
        }

        let bytes = span.text.as_bytes();
        let mut segment_start = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            let newline_len = match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
                b'\r' | b'\n' => 1,
                _ => {
                    index += 1;
                    continue;
                }
            };
            if !measure.push_segment(&span.text[segment_start..index]) {
                return None;
            }
            let mapped_boundary = span.source_range.is_some() || span.rich_range.is_some();
            if !measure.push_newline(mapped_boundary) {
                return None;
            }
            index += newline_len;
            segment_start = index;
        }
        if !measure.push_segment(&span.text[segment_start..]) {
            return None;
        }
    }

    measure.finish()
}

struct WrappedLineMeasure {
    width: usize,
    limit: usize,
    prefix_width: usize,
    line_count: usize,
    current_width: usize,
    current_has_runs: bool,
    continuation_has_runs: bool,
}

impl WrappedLineMeasure {
    pub(super) fn push_segment(&mut self, segment: &str) -> bool {
        if segment.is_empty() {
            return true;
        }
        let span = Span::raw(segment);
        for grapheme in span.styled_graphemes(Style::default()) {
            let run_width = Span::raw(terminal_safe_text(grapheme.symbol)).width();
            if self.current_width > self.prefix_width
                && self.current_width.saturating_add(run_width) > self.width
                && !self.push_line()
            {
                return false;
            }
            self.current_width = self.current_width.saturating_add(run_width);
            self.current_has_runs = true;
        }
        true
    }

    pub(super) fn push_newline(&mut self, mapped_boundary: bool) -> bool {
        if !self.push_line() {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_has_runs = self.continuation_has_runs || mapped_boundary;
        true
    }

    pub(super) fn push_line(&mut self) -> bool {
        self.line_count = self.line_count.saturating_add(1);
        if self.line_count > self.limit {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_has_runs = self.continuation_has_runs;
        true
    }

    pub(super) fn finish(mut self) -> Option<usize> {
        if (self.current_has_runs || self.line_count == 0) && !self.push_line() {
            return None;
        }
        Some(self.line_count)
    }
}

#[cfg(test)]
std::thread_local! {
    pub(super) static RICH_FLATTEN_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn flatten_rich_document(model: &EditorViewModel, width: usize) -> Vec<DisplayLine> {
    #[cfg(test)]
    RICH_FLATTEN_CALL_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let width = width.max(1);
    let mut output = Vec::new();
    for (block_index, block) in model.render_blocks().iter().enumerate() {
        let table_widths = model.table_widths_for_block(block_index, block.table_id());
        let lines = block_lines(
            block,
            block_index,
            width,
            model.block_sources.get(block_index).copied(),
            table_widths,
            model.source.as_deref(),
        );
        output.extend(lines);
    }
    if output.is_empty() {
        output.push(empty_display_line(None));
    }
    output
}

pub(super) fn block_lines(
    block: &EditorRenderBlock,
    block_index: usize,
    width: usize,
    block_source: Option<EditorBlockSourceMap>,
    table_widths: Option<&[usize]>,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let anchor = block_source.map(EditorBlockSourceMap::anchor);
    match block {
        EditorRenderBlock::Paragraph(spans) => {
            wrap_runs(Vec::new(), spans, width, block_index, false, source)
        }
        EditorRenderBlock::Heading { level, spans } => {
            let level = (*level).clamp(1, 6);
            let styled: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.bold = true;
                    span.color = EditorSpanColor::Accent;
                    span.underlined |= level == 1;
                    span.italic |= level >= 3;
                    span
                })
                .collect();
            wrap_runs(Vec::new(), &styled, width, block_index, false, source)
        }
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            let quote_spans: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.italic = true;
                    span
                })
                .collect();
            wrap_runs(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                &quote_spans,
                width,
                block_index,
                false,
                source,
            )
        }
        EditorRenderBlock::CodeBlock { language, lines } => {
            let language = language.as_deref().unwrap_or("code");
            let mut output = vec![DisplayLine {
                runs: vec![DisplayRun::virtual_text(
                    format!("┌─ {language} "),
                    accent_span(),
                    anchor,
                )],
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            }];
            let line_ranges = code_line_source_ranges(lines, block_source, source);
            output.extend(lines.iter().enumerate().map(|(index, line)| {
                let line_range = line_ranges.get(index).copied().flatten();
                let line_anchor = line_range.map(|range| range.start).or(anchor);
                let mut runs = vec![DisplayRun::virtual_text("│ ", accent_span(), line_anchor)];
                runs.extend(mapped_text_runs(
                    line,
                    EditorRenderSpan::code(""),
                    line_range,
                    source,
                ));
                DisplayLine {
                    runs,
                    block_index: Some(block_index),
                    no_wrap: true,
                    column_start: 0,
                }
            }));
            let end_anchor = block_source
                .and_then(|mapping| mapping.content_range)
                .map(|range| range.end)
                .or(anchor);
            output.push(DisplayLine {
                runs: vec![DisplayRun::virtual_text("└─", accent_span(), end_anchor)],
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            });
            output
        }
        EditorRenderBlock::Table {
            header,
            rows,
            alignments,
        }
        | EditorRenderBlock::RichTable {
            table_id: _,
            header,
            rows,
            alignments,
        } => table_lines(
            header,
            rows,
            alignments,
            table_widths,
            block_index,
            anchor,
            source,
        ),
        EditorRenderBlock::HorizontalRule => vec![DisplayLine {
            runs: vec![DisplayRun::virtual_text(
                "─".repeat(width),
                accent_span(),
                anchor,
            )],
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
        }],
        EditorRenderBlock::RawHtml(html) => {
            let mut runs = vec![DisplayRun::virtual_text("HTML ", warning_span(), anchor)];
            runs.extend(mapped_text_runs(
                html,
                warning_span(),
                block_source.map(|mapping| mapping.source_range),
                source,
            ));
            vec![DisplayLine {
                runs,
                block_index: Some(block_index),
                no_wrap: true,
                column_start: 0,
            }]
        }
        EditorRenderBlock::Image { markdown } => vec![DisplayLine {
            runs: mapped_text_runs(
                markdown,
                muted_span(),
                block_source.map(|mapping| mapping.source_range),
                source,
            ),
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
        }],
        EditorRenderBlock::Footnote { label, spans } => wrap_runs(
            vec![DisplayRun::virtual_text(
                format!("[^{label}] "),
                accent_span(),
                anchor,
            )],
            spans,
            width,
            block_index,
            false,
            source,
        ),
        EditorRenderBlock::Blank => vec![empty_display_line_at(Some(block_index), anchor)],
    }
}

pub(super) fn table_lines(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    alignments: &[EditorTableAlignment],
    requested_widths: Option<&[usize]>,
    block_index: usize,
    anchor: Option<usize>,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let columns = max(
        header.len(),
        rows.iter().map(Vec::len).max().unwrap_or_default(),
    );
    if columns == 0 {
        return vec![empty_display_line_at(Some(block_index), anchor)];
    }
    let rich_anchor = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .flat_map(|row| row.iter())
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start));
    let widths = table_column_widths(header, rows, requested_widths);
    let first_row = if header.is_empty() {
        rows.first().map(Vec::as_slice).unwrap_or_default()
    } else {
        header
    };
    let mut output = vec![table_border_line(
        '┌',
        '┬',
        '┐',
        &widths,
        first_row,
        block_index,
        false,
        anchor,
        rich_anchor,
    )];
    if !header.is_empty() {
        output.push(table_row_line(
            header,
            &widths,
            alignments,
            block_index,
            true,
            anchor,
            rich_anchor,
            source,
        ));
        output.push(table_border_line(
            '├',
            '┼',
            '┤',
            &widths,
            header,
            block_index,
            false,
            anchor,
            rich_anchor,
        ));
    }
    output.extend(rows.iter().map(|row| {
        table_row_line(
            row,
            &widths,
            alignments,
            block_index,
            false,
            anchor,
            rich_anchor,
            source,
        )
    }));
    let last_row = rows.last().map(Vec::as_slice).unwrap_or(first_row);
    output.push(table_border_line(
        '└',
        '┴',
        '┘',
        &widths,
        last_row,
        block_index,
        false,
        anchor,
        rich_anchor,
    ));
    output
}

pub(super) fn table_row_line(
    row: &[EditorTableCell],
    widths: &[usize],
    alignments: &[EditorTableAlignment],
    block_index: usize,
    header: bool,
    anchor: Option<usize>,
    rich_anchor: Option<RichPosition>,
    source: Option<&str>,
) -> DisplayLine {
    let row_anchor = row
        .iter()
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start))
        .or(rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text("│", table_span(header), anchor).with_virtual_rich(row_anchor),
    ];
    for (column, width) in widths.iter().enumerate() {
        let cell = row.get(column);
        let cell_range = cell.and_then(table_cell_source_range);
        let start = cell_range.map(|range| range.start);
        let end = cell_range.map(|range| range.end);
        let rich_range = cell.and_then(table_cell_rich_range);
        let rich_start = rich_range.map(|range| range.start).or(row_anchor);
        let rich_end = rich_range.map(|range| range.end).or(rich_start);
        let mut content = cell.map_or_else(Vec::new, |cell| {
            cell.spans
                .iter()
                .flat_map(|span| {
                    let mut style = span.clone();
                    style.color = EditorSpanColor::Accent;
                    style.bold |= header;
                    mapped_text_runs(&span.text, style, span.source_range, source)
                })
                .collect::<Vec<_>>()
        });
        let (fitted, used) = fit_table_content(
            std::mem::take(&mut content),
            *width,
            end.or(start).or(anchor),
            rich_end,
            header,
        );
        let remaining = width.saturating_sub(used);
        let (aligned_left, aligned_right) =
            match alignments.get(column).copied().unwrap_or_default() {
                EditorTableAlignment::Left => (0, remaining),
                EditorTableAlignment::Center => {
                    (remaining / 2, remaining.saturating_sub(remaining / 2))
                }
                EditorTableAlignment::Right => (remaining, 0),
            };
        runs.push(table_padding(
            " ".repeat(aligned_left.saturating_add(1)),
            start,
            anchor,
            rich_start,
            header,
        ));
        runs.extend(fitted);
        runs.push(table_padding(
            " ".repeat(aligned_right.saturating_add(1)),
            end.or(start),
            anchor,
            rich_end,
            header,
        ));
        runs.push(
            DisplayRun::virtual_text("│", table_span(header), end.or(start).or(anchor))
                .with_virtual_rich(rich_end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap: true,
        column_start: 0,
    }
}

pub(super) fn table_border_line(
    left: char,
    middle: char,
    right: char,
    widths: &[usize],
    cells: &[EditorTableCell],
    block_index: usize,
    header: bool,
    anchor: Option<usize>,
    rich_anchor: Option<RichPosition>,
) -> DisplayLine {
    let first_anchor = cells
        .first()
        .and_then(table_cell_rich_range)
        .map(|range| range.start)
        .or(rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text(left.to_string(), table_span(header), anchor)
            .with_virtual_rich(first_anchor),
    ];
    for (column, width) in widths.iter().copied().enumerate() {
        let cell_range = cells.get(column).and_then(table_cell_rich_range);
        let start = cell_range.map(|range| range.start).or(rich_anchor);
        let end = cell_range.map(|range| range.end).or(start);
        runs.push(
            DisplayRun::virtual_text(
                "─".repeat(width.saturating_add(2)),
                table_span(header),
                anchor,
            )
            .with_virtual_rich(start),
        );
        let delimiter = if column + 1 == widths.len() {
            right
        } else {
            middle
        };
        runs.push(
            DisplayRun::virtual_text(delimiter.to_string(), table_span(header), anchor)
                .with_virtual_rich(end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap: true,
        column_start: 0,
    }
}

pub(super) fn table_span(header: bool) -> EditorRenderSpan {
    let mut style = EditorRenderSpan::plain("");
    style.color = EditorSpanColor::Accent;
    style.bold = header;
    style
}

pub(super) fn table_padding(
    text: String,
    byte_offset: Option<usize>,
    fallback: Option<usize>,
    rich_position: Option<RichPosition>,
    header: bool,
) -> DisplayRun {
    match byte_offset {
        Some(byte_offset) => DisplayRun {
            text: DisplayText::Owned(text),
            style: table_span(header),
            source: DisplaySource::Range(EditorSourceRange::new(byte_offset, byte_offset)),
            rich: rich_position
                .map(DisplayRich::Virtual)
                .unwrap_or(DisplayRich::Unmapped),
        },
        None => DisplayRun::virtual_text(text, table_span(header), fallback)
            .with_virtual_rich(rich_position),
    }
}

pub(super) fn table_cell_rich_range(cell: &EditorTableCell) -> Option<RichRange> {
    let mut ranges = cell.spans.iter().filter_map(|span| span.rich_range);
    let first = ranges.next()?;
    let mut start = first.start;
    let mut end = first.end;
    for range in ranges {
        if range.start.container_id != start.container_id
            || range.end.container_id != start.container_id
        {
            return Some(RichRange::between(first.start, first.end));
        }
        start.grapheme_offset = start.grapheme_offset.min(range.start.grapheme_offset);
        end.grapheme_offset = end.grapheme_offset.max(range.end.grapheme_offset);
    }
    Some(RichRange::between(start, end))
}

pub(super) fn table_cell_source_range(cell: &EditorTableCell) -> Option<EditorSourceRange> {
    Some(EditorSourceRange::new(
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.start)
            .min()?,
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.end)
            .max()?,
    ))
}

pub(super) fn fit_table_content(
    runs: Vec<DisplayRun>,
    width: usize,
    overflow_anchor: Option<usize>,
    rich_overflow_anchor: Option<RichPosition>,
    header: bool,
) -> (Vec<DisplayRun>, usize) {
    let used = runs_width(&runs);
    if used <= width {
        return (runs, used);
    }
    if width == 0 {
        return (Vec::new(), 0);
    }
    let content_width = width.saturating_sub(1);
    let mut fitted = Vec::new();
    let mut fitted_width = 0usize;
    for run in runs {
        let run_width = display_run_width(&run);
        if fitted_width.saturating_add(run_width) > content_width {
            break;
        }
        fitted_width = fitted_width.saturating_add(run_width);
        fitted.push(run);
    }
    fitted.push(
        DisplayRun::virtual_text("…", table_span(header), overflow_anchor)
            .with_virtual_rich(rich_overflow_anchor),
    );
    (fitted, width)
}

pub(super) fn table_column_widths(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    requested: Option<&[usize]>,
) -> Vec<usize> {
    let columns = max(
        header.len(),
        rows.iter().map(Vec::len).max().unwrap_or_default(),
    );
    let mut widths = vec![1usize; columns];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(
                Span::raw(terminal_safe_text(&cell_text(cell)))
                    .width()
                    .min(24),
            );
        }
    }
    if let Some(requested) = requested {
        for (width, requested) in widths.iter_mut().zip(requested.iter().copied()) {
            if requested > 0 {
                *width = requested.clamp(1, 120);
            }
        }
    }
    widths
}

pub(super) fn table_resize_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableResizeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        let mut boundary_column = 0usize;
        for (column_index, width) in widths.into_iter().enumerate() {
            boundary_column = boundary_column.saturating_add(width.saturating_add(3));
            let Some(visible_column) = boundary_column.checked_sub(horizontal_scroll) else {
                continue;
            };
            if visible_column >= usize::from(canvas.width) {
                continue;
            }
            handles.push(EditorTableResizeHandle {
                table_id,
                block_index: block_area.block_index,
                column_index,
                width,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

pub(super) fn table_edge_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableEdgeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let source_range = model
            .block_sources
            .get(block_area.block_index)
            .map(|mapping| mapping.source_range);
        if table_id.is_none() && source_range.is_none() {
            continue;
        }
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        if widths.is_empty() {
            continue;
        }

        if horizontal_scroll == 0 && canvas.width > 0 {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Left,
                source_range,
                area: Rect::new(canvas.x, block_area.area.y, 1, block_area.area.height),
            });
        }

        let right_column = widths
            .iter()
            .fold(0usize, |total, width| total.saturating_add(width + 3));
        if let Some(visible_column) = right_column.checked_sub(horizontal_scroll)
            && visible_column < usize::from(canvas.width)
        {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Right,
                source_range,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

pub(super) fn wrap_runs(
    prefix: Vec<DisplayRun>,
    spans: &[EditorRenderSpan],
    width: usize,
    block_index: usize,
    no_wrap: bool,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    let rich_anchor = spans
        .iter()
        .find_map(|span| span.rich_range.map(|range| range.start));
    let mut prefix = prefix;
    for run in &mut prefix {
        if matches!(run.rich, DisplayRich::Unmapped)
            && let Some(position) = rich_anchor
        {
            run.rich = DisplayRich::Virtual(position);
        }
    }
    let prefix_width = runs_width(&prefix);
    let continuation_anchor = prefix.iter().find_map(display_run_anchor);
    let continuation_prefix = if prefix_width == 0 {
        Vec::new()
    } else {
        vec![
            DisplayRun::virtual_text(
                " ".repeat(prefix_width),
                EditorRenderSpan::plain(""),
                continuation_anchor,
            )
            .with_virtual_rich(rich_anchor),
        ]
    };
    let mut lines = Vec::new();
    let mut current = prefix.clone();
    let mut current_width = prefix_width;
    for span in spans {
        let span_range = span.source_range;
        for run in mapped_text_runs(&span.text, span.clone(), span_range, source) {
            if is_display_newline(run.text.resolve(source)) {
                let before = display_run_start(&run);
                let rich_before = display_run_rich_start(&run);
                if before.is_some() || rich_before.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), before)
                            .with_editable_rich_boundary(rich_before),
                    );
                }
                lines.push(DisplayLine {
                    runs: std::mem::take(&mut current),
                    block_index: Some(block_index),
                    no_wrap,
                    column_start: 0,
                });
                current = continuation_prefix.clone();
                let after = display_run_end(&run);
                let rich_after = display_run_rich_end(&run);
                if after.is_some() || rich_after.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), after)
                            .with_editable_rich_boundary(rich_after),
                    );
                }
                current_width = prefix_width;
                continue;
            }
            let run_width = display_run_width(&run);
            if current_width > prefix_width && current_width.saturating_add(run_width) > width {
                lines.push(DisplayLine {
                    runs: std::mem::take(&mut current),
                    block_index: Some(block_index),
                    no_wrap,
                    column_start: 0,
                });
                current = continuation_prefix.clone();
                current_width = prefix_width;
            }
            current_width = current_width.saturating_add(run_width);
            current.push(run);
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(DisplayLine {
            runs: current,
            block_index: Some(block_index),
            no_wrap,
            column_start: 0,
        });
    }
    lines
}

pub(super) fn cell_text(cell: &EditorTableCell) -> String {
    cell.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

pub(super) fn fit_text(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut chars = text.chars();
    let mut fitted = chars.by_ref().take(width).collect::<String>();
    if chars.next().is_some() && width > 1 {
        fitted.pop();
        fitted.push('…');
    }
    fitted
}

pub(super) fn runs_width(runs: &[DisplayRun]) -> usize {
    runs.iter().map(display_run_width).sum()
}

pub(super) fn display_run_width(run: &DisplayRun) -> usize {
    Span::raw(terminal_safe_text(run.text.resolve(None))).width()
}

pub(super) fn display_run_anchor(run: &DisplayRun) -> Option<usize> {
    match run.source {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn display_run_start(run: &DisplayRun) -> Option<usize> {
    display_source_start(run.source)
}

pub(super) fn display_run_end(run: &DisplayRun) -> Option<usize> {
    display_source_end(run.source)
}

pub(super) fn display_source_start(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn display_source_end(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.end),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn is_display_newline(value: &str) -> bool {
    matches!(value, "\n" | "\r" | "\r\n")
}

pub(super) fn mapped_text_runs(
    text: &str,
    style: EditorRenderSpan,
    source_range: Option<EditorSourceRange>,
    source: Option<&str>,
) -> Vec<DisplayRun> {
    let exact = source_range.filter(|range| {
        range.start <= range.end
            && source.and_then(|source| source.get(range.start..range.end)) == Some(text)
    });
    let mut runs = Vec::new();
    let mut segment_start = 0usize;
    let mut grapheme_offset = 0usize;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let newline_len = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => {
                index += 1;
                continue;
            }
        };
        append_mapped_grapheme_runs(
            &mut runs,
            &text[segment_start..index],
            segment_start,
            &style,
            exact,
            source_range,
            &mut grapheme_offset,
        );
        push_mapped_text_run(
            &mut runs,
            &text[index..index + newline_len],
            index,
            index + newline_len,
            &style,
            exact,
            source_range,
            grapheme_offset,
            grapheme_offset.saturating_add(1),
        );
        grapheme_offset = grapheme_offset.saturating_add(1);
        index += newline_len;
        segment_start = index;
    }
    append_mapped_grapheme_runs(
        &mut runs,
        &text[segment_start..],
        segment_start,
        &style,
        exact,
        source_range,
        &mut grapheme_offset,
    );
    if runs.is_empty() {
        let rich = empty_rich_mapping(style.rich_range);
        runs.push(DisplayRun {
            text: DisplayText::Owned(String::new()),
            style,
            source: exact
                .map(DisplaySource::Range)
                .or_else(|| source_range.map(|range| DisplaySource::Virtual(range.start)))
                .unwrap_or(DisplaySource::Unmapped),
            rich,
        });
    }
    runs
}

pub(super) fn append_mapped_grapheme_runs(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    base: usize,
    style: &EditorRenderSpan,
    exact: Option<EditorSourceRange>,
    source_range: Option<EditorSourceRange>,
    grapheme_offset: &mut usize,
) {
    let span = Span::raw(text.to_owned());
    let mut relative = 0usize;
    for grapheme in span.styled_graphemes(Style::default()) {
        let start = base.saturating_add(relative);
        relative = relative.saturating_add(grapheme.symbol.len());
        push_mapped_text_run(
            runs,
            grapheme.symbol,
            start,
            base.saturating_add(relative),
            style,
            exact,
            source_range,
            *grapheme_offset,
            grapheme_offset.saturating_add(1),
        );
        *grapheme_offset = grapheme_offset.saturating_add(1);
    }
}

pub(super) fn push_mapped_text_run(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    start: usize,
    end: usize,
    style: &EditorRenderSpan,
    exact: Option<EditorSourceRange>,
    source_range: Option<EditorSourceRange>,
    grapheme_start: usize,
    grapheme_end: usize,
) {
    let source = match (exact, source_range) {
        (Some(range), _) => DisplaySource::Range(EditorSourceRange::new(
            range.start.saturating_add(start).min(range.end),
            range.start.saturating_add(end).min(range.end),
        )),
        // Decoded entities, generated labels and other non-exact text must
        // never pretend that every visible grapheme covers the whole source.
        (None, Some(range)) => DisplaySource::Virtual(range.start),
        (None, None) => DisplaySource::Unmapped,
    };
    runs.push(DisplayRun {
        text: DisplayText::Owned(text.to_owned()),
        style: style.clone(),
        source,
        rich: rich_mapping(style.rich_range, grapheme_start, grapheme_end),
    });
}

pub(super) fn empty_rich_mapping(range: Option<RichRange>) -> DisplayRich {
    match range {
        Some(range) if range.start.container_id == range.end.container_id => {
            DisplayRich::Range(range)
        }
        Some(range) => DisplayRich::Virtual(range.start),
        None => DisplayRich::Unmapped,
    }
}

pub(super) fn rich_mapping(
    range: Option<RichRange>,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    let Some(range) = range else {
        return DisplayRich::Unmapped;
    };
    if range.start.container_id != range.end.container_id {
        return DisplayRich::Virtual(range.start);
    }
    let start = range
        .start
        .grapheme_offset
        .saturating_add(relative_start)
        .min(range.end.grapheme_offset);
    let end = range
        .start
        .grapheme_offset
        .saturating_add(relative_end)
        .min(range.end.grapheme_offset);
    DisplayRich::Range(RichRange::between(
        RichPosition::in_node(range.start.container_id, start),
        RichPosition::in_node(range.start.container_id, end),
    ))
}

pub(super) fn display_source_for_segment(
    mapping: DisplaySource,
    text_len: usize,
    relative_start: usize,
    relative_end: usize,
) -> DisplaySource {
    match mapping {
        DisplaySource::Range(range)
            if range.end.saturating_sub(range.start) == text_len
                && relative_start <= relative_end =>
        {
            DisplaySource::Range(EditorSourceRange::new(
                range.start.saturating_add(relative_start).min(range.end),
                range.start.saturating_add(relative_end).min(range.end),
            ))
        }
        mapping => mapping,
    }
}

pub(super) fn display_rich_for_grapheme(
    mapping: DisplayRich,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    match mapping {
        DisplayRich::Range(range)
            if range.start.container_id == range.end.container_id
                && range
                    .end
                    .grapheme_offset
                    .saturating_sub(range.start.grapheme_offset)
                    >= relative_end =>
        {
            DisplayRich::Range(RichRange::between(
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_start),
                ),
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_end),
                ),
            ))
        }
        mapping => mapping,
    }
}

pub(super) fn display_line_source_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorSourceBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut column = line.column_start;
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        let text = run.text.resolve(source);
        if text.is_empty() {
            if let Some(offset) = display_run_start(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            if let Some(offset) = display_run_end(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_byte = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let relative_start = relative_byte;
            relative_byte = relative_byte.saturating_add(grapheme.symbol.len());
            let mapping =
                display_source_for_segment(run.source, text.len(), relative_start, relative_byte);
            let safe = terminal_safe_text(grapheme.symbol);
            let next_column = column.saturating_add(Span::raw(safe).width().max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(offset) = display_source_end(mapping) {
                fallback_boundary = Some(EditorSourceBoundary {
                    column: next_column,
                    byte_offset: offset,
                    editable: matches!(mapping, DisplaySource::Range(_)),
                });
            }
            if intersects && let Some(offset) = display_source_start(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            column = next_column;
            if intersects && let Some(offset) = display_source_end(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            if column > horizontal_end {
                break;
            }
        }
        if column > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

pub(super) fn display_line_rich_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorRichBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut column = line.column_start;
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        let text = run.text.resolve(source);
        if text.is_empty() {
            if let Some(position) = display_run_rich_start(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            if let Some(position) = display_run_rich_end(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_grapheme = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let mapping = display_rich_for_grapheme(
                run.rich,
                relative_grapheme,
                relative_grapheme.saturating_add(1),
            );
            relative_grapheme = relative_grapheme.saturating_add(1);
            let safe = terminal_safe_text(grapheme.symbol);
            let next_column = column.saturating_add(Span::raw(safe).width().max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(position) = display_rich_end(mapping) {
                fallback_boundary = Some(EditorRichBoundary {
                    column: next_column,
                    position,
                    editable: matches!(mapping, DisplayRich::Range(_)),
                });
            }
            if intersects && let Some(position) = display_rich_start(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            column = next_column;
            if intersects && let Some(position) = display_rich_end(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            if column > horizontal_end {
                break;
            }
        }
        if column > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

pub(super) fn display_run_rich_start(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_start(run.rich)
}

pub(super) fn display_rich_start(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.start),
        DisplayRich::Virtual(position) => Some(position),
    }
}

pub(super) fn display_run_rich_end(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_end(run.rich)
}

pub(super) fn display_rich_end(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.end),
        DisplayRich::Virtual(position) => Some(position),
    }
}

pub(super) fn push_rich_boundary(
    boundaries: &mut Vec<EditorRichBoundary>,
    column: usize,
    position: RichPosition,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.position == position)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorRichBoundary {
        column,
        position,
        editable,
    });
}

pub(super) fn push_source_boundary(
    boundaries: &mut Vec<EditorSourceBoundary>,
    column: usize,
    byte_offset: usize,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.byte_offset == byte_offset)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorSourceBoundary {
        column,
        byte_offset,
        editable,
    });
}

pub(super) fn nearest_source_boundary(
    boundaries: &[EditorSourceBoundary],
    column: usize,
) -> Option<EditorSourceBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

pub(super) fn nearest_rich_boundary(
    boundaries: &[EditorRichBoundary],
    column: usize,
) -> Option<EditorRichBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

pub(super) fn nearest_editable_rich_boundary(
    lines: &[EditorRichLineMap],
    visual: EditorTextPosition,
    position: RichPosition,
) -> Option<EditorRichBoundary> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| {
                    boundary.editable && boundary.position.container_id == position.container_id
                })
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            line.document_line.abs_diff(visual.line),
                            boundary.column.abs_diff(visual.column),
                        ),
                        *boundary,
                    )
                })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, boundary)| boundary)
}

pub(super) fn nearest_visual_position(
    lines: &[EditorSourceLineMap],
    byte_offset: usize,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries.iter().map(move |boundary| {
                (
                    (
                        boundary.byte_offset.abs_diff(byte_offset),
                        usize::from(!boundary.editable),
                    ),
                    EditorTextPosition::new(line.document_line, boundary.column),
                )
            })
        })
        .min_by_key(|(distance, position)| (*distance, position.line, position.column))
        .map(|(_, position)| position)
}

pub(super) fn nearest_visual_position_for_rich(
    lines: &[EditorRichLineMap],
    position: RichPosition,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| boundary.position.container_id == position.container_id)
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            usize::from(!boundary.editable),
                        ),
                        EditorTextPosition::new(line.document_line, boundary.column),
                    )
                })
        })
        .min_by_key(|(distance, visual)| (*distance, visual.line, visual.column))
        .map(|(_, visual)| visual)
}

pub(super) fn code_line_source_ranges(
    lines: &[String],
    block_source: Option<EditorBlockSourceMap>,
    source: Option<&str>,
) -> Vec<Option<EditorSourceRange>> {
    let Some(content) = block_source.and_then(|mapping| mapping.content_range) else {
        return vec![None; lines.len()];
    };
    let Some(source) = source else {
        return vec![None; lines.len()];
    };
    let Some(content_source) = source.get(content.start..content.end) else {
        return vec![None; lines.len()];
    };
    let mut cursor = 0usize;
    let mut ranges = Vec::with_capacity(lines.len());
    for line in lines {
        if !content_source[cursor.min(content_source.len())..].starts_with(line) {
            return vec![None; lines.len()];
        }
        let start = content.start.saturating_add(cursor);
        cursor = cursor.saturating_add(line.len()).min(content_source.len());
        ranges.push(Some(EditorSourceRange::new(
            start,
            content.start.saturating_add(cursor),
        )));
        let remaining = &content_source[cursor..];
        if remaining.starts_with("\r\n") {
            cursor += 2;
        } else if remaining.starts_with(['\r', '\n']) {
            cursor += 1;
        }
    }
    ranges
}

/// Replaces bytes that a terminal could interpret as control traffic with
/// visible, inert Unicode glyphs. This must be applied at the final rendering
/// boundary as documents, file names, and status messages are all untrusted.
///
/// C0 controls use the Unicode Control Pictures block, DEL uses its dedicated
/// symbol, and C1/bidirectional formatting controls use the replacement glyph.
/// Newlines are laid out structurally before this boundary; if one reaches this
/// function it is shown visibly instead of being emitted to the terminal.
pub(super) fn accent_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Accent,
        bold: true,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn muted_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Muted,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn warning_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Warning,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn empty_display_line(block_index: Option<usize>) -> DisplayLine {
    empty_display_line_at(block_index, None)
}

pub(super) fn empty_display_line_at(
    block_index: Option<usize>,
    byte_offset: Option<usize>,
) -> DisplayLine {
    DisplayLine {
        runs: vec![DisplayRun::virtual_text(
            "",
            EditorRenderSpan::default(),
            byte_offset,
        )],
        block_index,
        no_wrap: false,
        column_start: 0,
    }
}
