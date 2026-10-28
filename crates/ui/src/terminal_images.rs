//! Image preparation stays separate from child protocols and terminal I/O.
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

use image::{DynamicImage, Pixel, Rgba, RgbaImage, imageops::FilterType};
use ratatui::Frame;
use ratatui::buffer::Cell;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier};
use ratatui_image::Image;
use ratatui_image::protocol::{
    Protocol, halfblocks::Halfblocks, iterm2::Iterm2, kitty::Kitty, sixel::Sixel,
};

use crate::{
    CommandLineColor, CommandLineTerminalSnapshot, EDITOR_IMAGE_MAX_PIXELS, EditorGraphicsProtocol,
    EditorImagePicker, TerminalImageFragment,
};

const CACHE_BUDGET: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ImageKey {
    id: u64,
    data: usize,
    source: [u32; 4],
    image_size: (u32, u32),
    area_size: Size,
    padding: [u16; 4],
    source_cell_size: [u16; 2],
    crop: [u32; 4],
    size: Size,
    backend: Option<EditorGraphicsProtocol>,
    cell_size: (u16, u16),
    tmux: bool,
    backgrounds: Vec<[Color; 2]>,
}

struct PreparedImage {
    protocol: Protocol,
    _rgba: Arc<[u8]>,
    kitty: Option<(u32, bool)>,
    bytes: usize,
    transparent_cells: Vec<(u16, u16)>,
}

/// One page's prepared image fragments. Only the current viewport is retained.
#[derive(Default)]
pub struct TerminalImageCache {
    entries: HashMap<ImageKey, PreparedImage>,
    cleanup: Vec<u8>,
}

impl TerminalImageCache {
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        terminal: &CommandLineTerminalSnapshot,
        picker: Option<&EditorImagePicker>,
    ) {
        let clipped = area.intersection(frame.area());
        let viewport = Rect::new(
            clipped.x.saturating_sub(area.x),
            clipped.y.saturating_sub(area.y),
            clipped.width,
            clipped.height,
        );
        let mut used = HashSet::new();
        let mut images = terminal.images.iter().collect::<Vec<_>>();
        let raster_layers = images.iter().any(|image| {
            !image.area.intersection(viewport).is_empty()
                && (image.padding != [0; 4]
                    || image.rgba.chunks_exact(4).any(|pixel| pixel[3] != 255))
        }) || images.iter().enumerate().any(|(index, image)| {
            images[index + 1..].iter().any(|other| {
                !image
                    .area
                    .intersection(other.area)
                    .intersection(viewport)
                    .is_empty()
            })
        });
        // Native payloads clear cells or use spanning placeholders, so layers must compose first.
        let picker = picker.filter(|_| !raster_layers);
        images.sort_by_key(|image| image.z_index);
        for image in images {
            let visible = image.area.intersection(viewport);
            let mut canvas = None;
            for piece in visible_pieces(image, visible, terminal) {
                let cell_size = picker.map_or((1, 2), EditorImagePicker::cell_size);
                let Some((stored, working)) = preparation_cost(image, piece.as_size(), cell_size)
                else {
                    continue;
                };
                if image
                    .rgba
                    .len()
                    .saturating_add(stored)
                    .saturating_add(working)
                    > CACHE_BUDGET
                {
                    continue;
                }
                let key_bytes = if picker.is_none() {
                    usize::from(piece.width)
                        * usize::from(piece.height)
                        * std::mem::size_of::<[Color; 2]>()
                } else {
                    0
                };
                if self.cached_bytes().saturating_add(key_bytes) > CACHE_BUDGET {
                    self.retain_used(&used);
                    if self.cached_bytes().saturating_add(key_bytes) > CACHE_BUDGET {
                        continue;
                    }
                }
                let Some(crop) = source_for_area(image, piece) else {
                    continue;
                };
                let target = Rect::new(
                    area.x.saturating_add(piece.x),
                    area.y.saturating_add(piece.y),
                    piece.width,
                    piece.height,
                );
                let key = ImageKey {
                    id: image.image_id,
                    data: image.rgba.as_ptr() as usize,
                    source: image.source,
                    image_size: (image.width, image.height),
                    area_size: image.area.as_size(),
                    padding: image.padding,
                    source_cell_size: image.cell_size,
                    crop,
                    size: piece.as_size(),
                    backend: picker.map(EditorImagePicker::protocol),
                    cell_size,
                    tmux: picker.is_some_and(EditorImagePicker::is_tmux),
                    backgrounds: if picker.is_none() {
                        (target.y..target.bottom())
                            .flat_map(|y| (target.x..target.right()).map(move |x| (x, y)))
                            .map(|position| raster_background(&frame.buffer_mut()[position]))
                            .collect()
                    } else {
                        Vec::new()
                    },
                };
                if !self.entries.contains_key(&key) {
                    let needed = |cache: &Self| {
                        let source = if cache
                            .entries
                            .values()
                            .any(|entry| entry._rgba.as_ptr() == image.rgba.as_ptr())
                        {
                            0
                        } else {
                            image.rgba.len()
                        };
                        cache
                            .cached_bytes()
                            .saturating_add(source)
                            .saturating_add(stored)
                            .saturating_add(working)
                    };
                    if needed(self) > CACHE_BUDGET {
                        self.retain_used(&used);
                        if needed(self) > CACHE_BUDGET {
                            continue;
                        }
                    }
                    let Some(canvas) = canvas.get_or_insert_with(|| image_canvas(image)).as_ref()
                    else {
                        continue;
                    };
                    let Some(prepared) = prepare_image(image, &key, canvas, stored) else {
                        continue;
                    };
                    self.entries.insert(key.clone(), prepared);
                }
                let prepared = &self.entries[&key];
                used.insert(key.clone());
                let transparent = prepared
                    .transparent_cells
                    .iter()
                    .map(|&(x, y)| {
                        let position = (target.x + x, target.y + y);
                        (position, frame.buffer_mut()[position].clone())
                    })
                    .collect::<Vec<_>>();
                if picker.is_none() {
                    for y in target.y..target.bottom() {
                        for x in target.x..target.right() {
                            frame.buffer_mut()[(x, y)].reset();
                        }
                    }
                }
                frame.render_widget(Image::new(&prepared.protocol), target);
                for (position, cell) in transparent {
                    frame.buffer_mut()[position] = cell;
                }
            }
        }
        self.retain_used(&used);
    }

    fn cached_bytes(&self) -> usize {
        let mut data = HashSet::new();
        self.entries.values().fold(0_usize, |bytes, entry| {
            bytes.saturating_add(entry.bytes).saturating_add(
                if data.insert(entry._rgba.as_ptr() as usize) {
                    entry._rgba.len()
                } else {
                    0
                },
            )
        })
    }

    fn retain_used(&mut self, used: &HashSet<ImageKey>) {
        self.entries.retain(|key, image| {
            if used.contains(key) {
                true
            } else {
                append_cleanup(&mut self.cleanup, image.kitty);
                false
            }
        });
    }

    pub fn clear(&mut self) {
        for (_, image) in self.entries.drain() {
            append_cleanup(&mut self.cleanup, image.kitty);
        }
    }

    /// Trusted host-resource deletion commands; no child bytes are included.
    pub fn take_cleanup(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cleanup)
    }
}

fn preparation_cost(
    image: &TerminalImageFragment,
    size: Size,
    cell_size: (u16, u16),
) -> Option<(usize, usize)> {
    let data = u64::from(image.width).checked_mul(u64::from(image.height))?;
    let source = u64::from(image.source[2]).checked_mul(u64::from(image.source[3]))?;
    let canvas_width = u64::from(image.area.width) * u64::from(image.cell_size[0]);
    let canvas_height = u64::from(image.area.height) * u64::from(image.cell_size[1]);
    let canvas = canvas_width.checked_mul(canvas_height)?;
    let crop_width = u64::from(size.width) * u64::from(image.cell_size[0]);
    let crop = crop_width.checked_mul(u64::from(size.height) * u64::from(image.cell_size[1]))?;
    let target_height = u64::from(size.height) * u64::from(cell_size.1);
    let pixels = (u64::from(size.width) * u64::from(cell_size.0)).checked_mul(target_height)?;
    let content_height =
        canvas_height.checked_sub(u64::from(image.padding[1]) + u64::from(image.padding[3]))?;
    // Triangle resizing allocates a floating-point row pass before the final pixels.
    let canvas_pass = u64::from(image.source[2]).checked_mul(content_height)?;
    let target_pass = crop_width.checked_mul(target_height)?;
    if [data, source, canvas, crop, pixels, canvas_pass, target_pass]
        .into_iter()
        .any(|pixels| pixels == 0 || pixels > EDITOR_IMAGE_MAX_PIXELS)
    {
        return None;
    }
    let cells = u64::from(size.width) * u64::from(size.height);
    let keys = cells.checked_mul((std::mem::size_of::<[Color; 2]>() * 3 + 8) as u64)?;
    let stored = pixels.checked_mul(32)?.checked_add(keys)?.checked_add(
        (std::mem::size_of::<ImageKey>() + std::mem::size_of::<PreparedImage>() + 64) as u64,
    )?;
    let working = source
        .checked_mul(4)?
        .checked_add(canvas.checked_mul(8)?)?
        .checked_add(crop.checked_mul(4)?)?
        .checked_add(pixels.checked_mul(8)?)?
        .checked_add(canvas_pass.checked_mul(16)?)?
        .checked_add(target_pass.checked_mul(16)?)?;
    Some((stored.try_into().ok()?, working.try_into().ok()?))
}

fn raster_background(cell: &Cell) -> [Color; 2] {
    let (foreground, background) = if cell.modifier.contains(Modifier::REVERSED) {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    };
    match cell.symbol() {
        "▀" => [foreground, background],
        "▄" => [background, foreground],
        "█" => [foreground; 2],
        _ => [background; 2],
    }
}

fn image_canvas(image: &TerminalImageFragment) -> Option<RgbaImage> {
    let pixels = u64::from(image.width).checked_mul(u64::from(image.height))?;
    if pixels == 0
        || pixels > EDITOR_IMAGE_MAX_PIXELS
        || pixels.checked_mul(4)? != image.rgba.len() as u64
    {
        return None;
    }
    let [x, y, width, height] = image.source;
    if width == 0
        || height == 0
        || x.checked_add(width)? > image.width
        || y.checked_add(height)? > image.height
    {
        return None;
    }
    let canvas_width = u32::from(image.area.width) * u32::from(image.cell_size[0]);
    let canvas_height = u32::from(image.area.height) * u32::from(image.cell_size[1]);
    if canvas_width == 0
        || canvas_height == 0
        || u64::from(canvas_width) * u64::from(canvas_height) > EDITOR_IMAGE_MAX_PIXELS
    {
        return None;
    }
    let [left, top, right, bottom] = image.padding.map(u32::from);
    let content_width = canvas_width.checked_sub(left.checked_add(right)?)?;
    let content_height = canvas_height.checked_sub(top.checked_add(bottom)?)?;
    if content_width == 0 || content_height == 0 {
        return None;
    }
    let mut cropped = Vec::with_capacity(width as usize * height as usize * 4);
    for row in y..y + height {
        let start = (row as usize * image.width as usize + x as usize) * 4;
        cropped.extend_from_slice(&image.rgba[start..start + width as usize * 4]);
    }
    let cropped = DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, cropped)?);
    let content = resize_rgba(cropped, content_width, content_height);
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);
    image::imageops::overlay(&mut canvas, &content, i64::from(left), i64::from(top));
    Some(canvas)
}

fn prepare_image(
    image: &TerminalImageFragment,
    key: &ImageKey,
    canvas: &RgbaImage,
    bytes: usize,
) -> Option<PreparedImage> {
    let [x, y, width, height] = key.crop;
    let cropped =
        DynamicImage::ImageRgba8(image::imageops::crop_imm(canvas, x, y, width, height).to_image());
    let target_width = u32::from(key.size.width).checked_mul(u32::from(key.cell_size.0))?;
    let target_height = u32::from(key.size.height).checked_mul(u32::from(key.cell_size.1))?;
    if u64::from(target_width) * u64::from(target_height) > EDITOR_IMAGE_MAX_PIXELS {
        return None;
    }
    let mut resized = resize_rgba(cropped, target_width, target_height);
    let mut transparent_cells = Vec::new();
    if key.backend.is_none() {
        let mut transparent =
            vec![true; usize::from(key.size.width) * usize::from(key.size.height)];
        for (x, y, pixel) in resized.enumerate_pixels_mut() {
            let column = x / u32::from(key.cell_size.0);
            let row = y / u32::from(key.cell_size.1);
            let index = row as usize * usize::from(key.size.width) + column as usize;
            transparent[index] &= pixel[3] == 0;
            let half = usize::from(
                y % u32::from(key.cell_size.1) >= u32::from(key.cell_size.1).div_ceil(2),
            );
            let (red, green, blue) =
                crate::theme::color_rgb(key.backgrounds[index][half]).unwrap_or((0, 0, 0));
            let mut background = Rgba([red, green, blue, 255]);
            background.blend(pixel);
            *pixel = background;
        }
        transparent_cells = transparent
            .into_iter()
            .enumerate()
            .filter_map(|(index, transparent)| {
                transparent.then_some((
                    (index % usize::from(key.size.width)) as u16,
                    (index / usize::from(key.size.width)) as u16,
                ))
            })
            .collect();
    }
    let resized = DynamicImage::ImageRgba8(resized);
    let tmux = key.tmux;
    let mut kitty = None;
    let protocol = match key.backend {
        Some(EditorGraphicsProtocol::Kitty) => {
            let id = crate::editor_media::next_kitty_image_id();
            kitty = Some((id, tmux));
            Protocol::Kitty(Kitty::new(resized, key.size, id, tmux).ok()?)
        }
        Some(EditorGraphicsProtocol::Sixel) => {
            Protocol::Sixel(Sixel::new(resized, key.size, tmux).ok()?)
        }
        Some(EditorGraphicsProtocol::Iterm2) => {
            Protocol::ITerm2(Iterm2::new(resized, key.size, tmux).ok()?)
        }
        None => Protocol::Halfblocks(Halfblocks::new(resized, key.size).ok()?),
    };
    Some(PreparedImage {
        protocol,
        _rgba: image.rgba.clone(),
        kitty,
        transparent_cells,
        bytes,
    })
}

fn resize_rgba(image: DynamicImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        image.into_rgba8()
    } else {
        image
            .resize_exact(width, height, FilterType::Triangle)
            .into_rgba8()
    }
}

fn source_for_area(image: &TerminalImageFragment, area: Rect) -> Option<[u32; 4]> {
    if area.is_empty() || image.area.is_empty() || image.cell_size.contains(&0) {
        return None;
    }
    Some([
        u32::from(area.x.saturating_sub(image.area.x)) * u32::from(image.cell_size[0]),
        u32::from(area.y.saturating_sub(image.area.y)) * u32::from(image.cell_size[1]),
        u32::from(area.width) * u32::from(image.cell_size[0]),
        u32::from(area.height) * u32::from(image.cell_size[1]),
    ])
}

fn visible_pieces(
    image: &TerminalImageFragment,
    visible: Rect,
    terminal: &CommandLineTerminalSnapshot,
) -> Vec<Rect> {
    if visible.is_empty() {
        return Vec::new();
    }
    if image.z_index >= 0 {
        return vec![visible];
    }
    let mut pieces = Vec::new();
    for y in visible.y..visible.bottom() {
        let mut start = None;
        for x in visible.x..visible.right() {
            let text = terminal.cell(x, y).is_some_and(|cell| {
                cell.cursor
                    || cell.command_status.is_some()
                    || !cell.symbol.trim().is_empty()
                    || image.z_index < -(1 << 30)
                        && if cell.style.inverse {
                            cell.style.foreground != CommandLineColor::Default
                        } else {
                            cell.style.background != CommandLineColor::Default
                        }
            }) || x > 0
                && terminal
                    .cell(x - 1, y)
                    .is_some_and(|cell| ratatui::text::Span::raw(&cell.symbol).width() > 1);
            if !text {
                start.get_or_insert(x);
            } else if let Some(left) = start.take() {
                pieces.push(Rect::new(left, y, x - left, 1));
            }
        }
        if let Some(left) = start {
            pieces.push(Rect::new(left, y, visible.right() - left, 1));
        }
    }
    pieces
}

fn append_cleanup(output: &mut Vec<u8>, kitty: Option<(u32, bool)>) {
    if let Some((id, tmux)) = kitty {
        let mut command = String::new();
        let _ = write!(command, "\x1b_Ga=d,d=I,i={id}\x1b\\");
        if tmux {
            output.extend_from_slice(b"\x1bPtmux;");
            output.extend_from_slice(command.replace('\x1b', "\x1b\x1b").as_bytes());
            output.extend_from_slice(b"\x1b\\");
        } else {
            output.extend_from_slice(command.as_bytes());
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/terminal_images.rs"]
mod tests;
