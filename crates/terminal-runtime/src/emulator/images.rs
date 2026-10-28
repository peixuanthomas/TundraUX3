//! Image fragments retain source pixels and placement within the child terminal.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use wezterm_cell::image::{ImageCell, ImageDataType};

const IMAGE_CACHE_BYTES: usize = 80_000_000;
static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct DecodedImage {
    id: u64,
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

impl Emulator {
    pub(super) fn snapshot_images(&mut self) -> Vec<ui::TerminalImageFragment> {
        let (rows, columns) = self.size();
        let start = self
            .terminal
            .screen()
            .scrollback_rows()
            .saturating_sub(usize::from(rows))
            .saturating_sub(self.scrollback);
        let mut image_cells = Vec::new();
        self.terminal
            .screen()
            .with_phys_lines(start..start + usize::from(rows), |lines| {
                for (y, line) in lines.iter().enumerate() {
                    for cell in line.visible_cells() {
                        if cell.cell_index() >= usize::from(columns) {
                            break;
                        }
                        if let Some(images) = cell.attrs().images() {
                            for image in images {
                                image_cells.push((cell.cell_index() as u16, y as u16, image));
                            }
                        }
                    }
                }
            });
        let mut fragments = Vec::new();
        for (x, y, image) in image_cells {
            if let Some(fragment) = self.image_fragment(x, y, image) {
                fragments.push(fragment);
            }
        }
        merge_image_fragments(fragments)
    }

    fn image_fragment(
        &mut self,
        x: u16,
        y: u16,
        image: ImageCell,
    ) -> Option<ui::TerminalImageFragment> {
        let hash = image.image_data().hash();
        if !self.images.contains_key(&hash) {
            let decoded = image.image_data().data().clone().decode();
            let (width, height, data) = match decoded {
                ImageDataType::Rgba8 {
                    width,
                    height,
                    data,
                    ..
                } => (width, height, data),
                ImageDataType::AnimRgba8 {
                    width,
                    height,
                    mut frames,
                    ..
                } if !frames.is_empty() => (width, height, frames.remove(0)),
                _ => return None,
            };
            if u64::from(width) * u64::from(height) > ui::EDITOR_IMAGE_MAX_PIXELS {
                return None;
            }
            if self.image_bytes.saturating_add(data.len()) > IMAGE_CACHE_BYTES {
                self.images.clear();
                self.image_bytes = 0;
            }
            self.image_bytes += data.len();
            self.images.insert(
                hash,
                DecodedImage {
                    id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed),
                    width,
                    height,
                    rgba: data.into(),
                },
            );
        }
        let data = self.images.get(&hash)?;
        let top = image.top_left();
        let bottom = image.bottom_right();
        let source_x = (top.x.into_inner() * data.width as f32)
            .round()
            .clamp(0., data.width.saturating_sub(1) as f32) as u32;
        let source_y = (top.y.into_inner() * data.height as f32)
            .round()
            .clamp(0., data.height.saturating_sub(1) as f32) as u32;
        let end_x = (bottom.x.into_inner() * data.width as f32)
            .round()
            .clamp((source_x + 1) as f32, data.width as f32) as u32;
        let end_y = (bottom.y.into_inner() * data.height as f32)
            .round()
            .clamp((source_y + 1) as f32, data.height as f32) as u32;
        let (left, top, right, bottom) = image.padding();
        let cell_width = u32::from(self.cell_size.0);
        let cell_height = u32::from(self.cell_size.1);
        let draw_width = cell_width.saturating_sub(u32::from(right));
        let draw_height = cell_height.saturating_sub(u32::from(bottom));
        let columns = (u32::from(left) + draw_width).div_ceil(cell_width) as u16;
        let rows = (u32::from(top) + draw_height).div_ceil(cell_height) as u16;
        Some(ui::TerminalImageFragment {
            image_id: data.id,
            width: data.width,
            height: data.height,
            rgba: data.rgba.clone(),
            source: [source_x, source_y, end_x - source_x, end_y - source_y],
            area: ratatui::layout::Rect::new(x, y, columns, rows),
            z_index: image.z_index(),
            padding: [
                left,
                top,
                (u32::from(columns) * cell_width - u32::from(left) - draw_width) as u16,
                (u32::from(rows) * cell_height - u32::from(top) - draw_height) as u16,
            ],
            cell_size: [self.cell_size.0, self.cell_size.1],
        })
    }
}

fn merge_image_fragments(
    mut fragments: Vec<ui::TerminalImageFragment>,
) -> Vec<ui::TerminalImageFragment> {
    fragments.sort_by_key(|image| (image.z_index, image.image_id, image.area.y, image.area.x));
    let mut rows: Vec<ui::TerminalImageFragment> = Vec::new();
    for image in fragments {
        if let Some(previous) = rows.last_mut() {
            if previous.image_id == image.image_id
                && previous.z_index == image.z_index
                && content_bounds(previous)[1] == content_bounds(&image)[1]
                && content_bounds(previous)[3] == content_bounds(&image)[3]
                && content_bounds(previous)[2] == content_bounds(&image)[0]
                && previous.source[1] == image.source[1]
                && previous.source[3] == image.source[3]
                && previous.source[0] + previous.source[2] == image.source[0]
            {
                let end = content_bounds(&image)[2];
                previous.area.width =
                    previous.area.right().max(image.area.right()) - previous.area.x;
                previous.source[2] += image.source[2];
                previous.padding[2] = (u32::from(previous.area.right())
                    * u32::from(previous.cell_size[0])
                    - end) as u16;
                continue;
            }
        }
        rows.push(image);
    }
    let mut merged: Vec<ui::TerminalImageFragment> = Vec::new();
    for image in rows {
        if let Some(previous) = merged.iter_mut().rev().find(|previous| {
            previous.image_id == image.image_id
                && previous.z_index == image.z_index
                && content_bounds(previous)[0] == content_bounds(&image)[0]
                && content_bounds(previous)[2] == content_bounds(&image)[2]
                && content_bounds(previous)[3] == content_bounds(&image)[1]
                && previous.source[0] == image.source[0]
                && previous.source[2] == image.source[2]
                && previous.source[1] + previous.source[3] == image.source[1]
        }) {
            let end = content_bounds(&image)[3];
            previous.area.height =
                previous.area.bottom().max(image.area.bottom()) - previous.area.y;
            previous.source[3] += image.source[3];
            previous.padding[3] =
                (u32::from(previous.area.bottom()) * u32::from(previous.cell_size[1]) - end) as u16;
        } else {
            merged.push(image);
        }
    }
    merged
}

fn content_bounds(image: &ui::TerminalImageFragment) -> [u32; 4] {
    let [width, height] = image.cell_size.map(u32::from);
    [
        u32::from(image.area.x) * width + u32::from(image.padding[0]),
        u32::from(image.area.y) * height + u32::from(image.padding[1]),
        u32::from(image.area.right()) * width - u32::from(image.padding[2]),
        u32::from(image.area.bottom()) * height - u32::from(image.padding[3]),
    ]
}
