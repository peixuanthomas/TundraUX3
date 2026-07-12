use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Cursor, Read};
use std::sync::OnceLock;

use geojson::{GeoJson, GeometryValue as GeoJsonValue, Position};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};
use zip::ZipArchive;

use crate::{BorderShape, TundraTheme};

const CITY_MARKER_SYMBOL: &str = "◎";
const DEFAULT_CACHE_CAPACITY: usize = 8;
const BRAILLE_PIXEL_WIDTH: u16 = 2;
const BRAILLE_PIXEL_HEIGHT: u16 = 4;
const BRAILLE_DOTS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
pub const TIMEZONE_MAP_ASSET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/timezones-with-oceans-2026b.geojson.zip"
);
pub const TIMEZONE_MAP_RASTER_ASSET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/timezone-map-2026b-raster.txt"
);
const TIMEZONE_MAP_ASSET_BYTES: &[u8] =
    include_bytes!("../assets/timezones-with-oceans-2026b.geojson.zip");
const TIMEZONE_MAP_RASTER_ASSET: &str = include_str!("../assets/timezone-map-2026b-raster.txt");

static TIMEZONE_BOUNDARY_INDEX: OnceLock<Result<TimezoneBoundaryIndex, TimezoneMapError>> =
    OnceLock::new();
static COMPACT_TIMEZONE_MAP: OnceLock<CompactTimezoneMap> = OnceLock::new();

thread_local! {
    static DEFAULT_RASTER_CACHE: TimezoneMapRasterCache = TimezoneMapRasterCache::default();
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimezoneCoordinate {
    pub longitude: f64,
    pub latitude: f64,
}

impl TimezoneCoordinate {
    pub fn new(longitude: f64, latitude: f64) -> Self {
        Self {
            longitude,
            latitude,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimezonePolygon {
    pub rings: Vec<Vec<TimezoneCoordinate>>,
}

impl TimezonePolygon {
    pub fn new(rings: Vec<Vec<TimezoneCoordinate>>) -> Self {
        Self { rings }
    }

    pub fn from_exterior(exterior: Vec<TimezoneCoordinate>) -> Self {
        Self {
            rings: vec![exterior],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimezoneBoundary {
    pub id: String,
    pub timezone_id: String,
    pub polygons: Vec<TimezonePolygon>,
}

impl TimezoneBoundary {
    pub fn new(
        id: impl Into<String>,
        timezone_id: impl Into<String>,
        polygons: Vec<TimezonePolygon>,
    ) -> Self {
        Self {
            id: id.into(),
            timezone_id: timezone_id.into(),
            polygons,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimezoneBoundaryIndex {
    boundaries: Vec<TimezoneBoundary>,
    boundary_index_by_tzid: BTreeMap<String, usize>,
}

impl TimezoneBoundaryIndex {
    pub fn new(boundaries: Vec<TimezoneBoundary>) -> Self {
        let boundary_index_by_tzid = boundaries
            .iter()
            .enumerate()
            .map(|(index, boundary)| (boundary.id.clone(), index))
            .collect();
        Self {
            boundaries,
            boundary_index_by_tzid,
        }
    }

    pub fn len(&self) -> usize {
        self.boundaries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boundaries.is_empty()
    }

    pub fn boundaries(&self) -> &[TimezoneBoundary] {
        &self.boundaries
    }

    pub fn timezone_ids(&self) -> impl Iterator<Item = &str> {
        self.boundary_index_by_tzid.keys().map(String::as_str)
    }

    pub fn boundary_for_timezone(&self, timezone_id: &str) -> Option<&TimezoneBoundary> {
        self.boundary_for_boundary_id(boundary_id_for_timezone(timezone_id))
    }

    pub fn boundary_for_boundary_id(&self, boundary_id: &str) -> Option<&TimezoneBoundary> {
        self.boundary_index_by_tzid
            .get(boundary_id)
            .and_then(|index| self.boundaries.get(*index))
    }

    pub fn polygons_for_timezone(&self, timezone_id: &str) -> Option<&[TimezonePolygon]> {
        self.boundary_for_timezone(timezone_id)
            .map(|boundary| boundary.polygons.as_slice())
    }

    pub fn polygons_for_boundary_id(&self, boundary_id: &str) -> Option<&[TimezonePolygon]> {
        self.boundary_for_boundary_id(boundary_id)
            .map(|boundary| boundary.polygons.as_slice())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimezoneMapError {
    ZipArchive {
        message: String,
    },
    ZipEntry {
        name: String,
        message: String,
    },
    NoGeojsonPayloadEntry,
    MultipleGeojsonPayloadEntries {
        entries: Vec<String>,
    },
    Json {
        message: String,
    },
    ExpectedFeatureCollection,
    MissingTzid {
        feature_index: usize,
    },
    EmptyTzid {
        feature_index: usize,
    },
    InvalidCoordinates {
        feature_index: usize,
        tzid: String,
        geometry_type: String,
        detail: String,
    },
}

impl fmt::Display for TimezoneMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZipArchive { message } => {
                write!(formatter, "failed to open timezone map zip: {message}")
            }
            Self::ZipEntry { name, message } => {
                write!(
                    formatter,
                    "failed to read timezone map zip entry {name}: {message}"
                )
            }
            Self::NoGeojsonPayloadEntry => {
                write!(formatter, "timezone map zip has no .json or .geojson entry")
            }
            Self::MultipleGeojsonPayloadEntries { entries } => {
                write!(
                    formatter,
                    "timezone map zip must contain one .json or .geojson entry, found {}: {}",
                    entries.len(),
                    entries.join(", ")
                )
            }
            Self::Json { message } => {
                write!(formatter, "failed to parse timezone GeoJSON: {message}")
            }
            Self::ExpectedFeatureCollection => {
                write!(
                    formatter,
                    "timezone GeoJSON root is not a FeatureCollection"
                )
            }
            Self::MissingTzid { feature_index } => {
                write!(
                    formatter,
                    "timezone feature {feature_index} is missing properties.tzid"
                )
            }
            Self::EmptyTzid { feature_index } => {
                write!(
                    formatter,
                    "timezone feature {feature_index} has an empty properties.tzid"
                )
            }
            Self::InvalidCoordinates {
                feature_index,
                tzid,
                geometry_type,
                detail,
            } => write!(
                formatter,
                "timezone feature {feature_index} ({tzid}) has invalid {geometry_type} coordinates: {detail}"
            ),
        }
    }
}

impl std::error::Error for TimezoneMapError {}

pub fn boundary_id_for_timezone(timezone_id: &str) -> &str {
    match timezone_id {
        "UTC" | "Etc/UTC" => "Etc/GMT",
        other => other,
    }
}

pub fn timezone_boundary_index() -> Result<&'static TimezoneBoundaryIndex, &'static TimezoneMapError>
{
    TIMEZONE_BOUNDARY_INDEX
        .get_or_init(load_timezone_boundary_index)
        .as_ref()
}

pub fn timezone_boundaries() -> Result<&'static [TimezoneBoundary], &'static TimezoneMapError> {
    timezone_boundary_index().map(TimezoneBoundaryIndex::boundaries)
}

pub fn parse_timezone_map_zip(bytes: &[u8]) -> Result<TimezoneBoundaryIndex, TimezoneMapError> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|error| TimezoneMapError::ZipArchive {
            message: error.to_string(),
        })?;
    let geojson_entry = unique_geojson_payload_entry_name(&mut archive)?;
    let mut geojson = String::new();
    archive
        .by_name(&geojson_entry)
        .map_err(|error| TimezoneMapError::ZipEntry {
            name: geojson_entry.clone(),
            message: error.to_string(),
        })?
        .read_to_string(&mut geojson)
        .map_err(|error| TimezoneMapError::ZipEntry {
            name: geojson_entry,
            message: error.to_string(),
        })?;
    parse_timezone_feature_collection(&geojson)
}

pub fn parse_timezone_feature_collection(
    contents: &str,
) -> Result<TimezoneBoundaryIndex, TimezoneMapError> {
    let geojson = contents
        .parse::<GeoJson>()
        .map_err(|error| TimezoneMapError::Json {
            message: error.to_string(),
        })?;
    let GeoJson::FeatureCollection(collection) = geojson else {
        return Err(TimezoneMapError::ExpectedFeatureCollection);
    };
    let mut polygons_by_tzid: BTreeMap<String, Vec<TimezonePolygon>> = BTreeMap::new();

    for (feature_index, feature) in collection.features.into_iter().enumerate() {
        let tzid = feature
            .properties
            .as_ref()
            .and_then(|properties| properties.get("tzid"))
            .and_then(|value| value.as_str())
            .ok_or(TimezoneMapError::MissingTzid { feature_index })?;
        if tzid.is_empty() {
            return Err(TimezoneMapError::EmptyTzid { feature_index });
        }

        let Some(geometry) = feature.geometry else {
            continue;
        };
        match geometry.value {
            GeoJsonValue::Polygon {
                coordinates: polygon,
                ..
            } => {
                polygons_by_tzid
                    .entry(tzid.to_string())
                    .or_default()
                    .push(parse_polygon(feature_index, tzid, "Polygon", polygon)?);
            }
            GeoJsonValue::MultiPolygon {
                coordinates: polygons,
                ..
            } => {
                let parsed = polygons
                    .into_iter()
                    .map(|polygon| parse_polygon(feature_index, tzid, "MultiPolygon", polygon))
                    .collect::<Result<Vec<_>, _>>()?;
                polygons_by_tzid
                    .entry(tzid.to_string())
                    .or_default()
                    .extend(parsed);
            }
            _ => {}
        }
    }

    Ok(TimezoneBoundaryIndex::new(
        polygons_by_tzid
            .into_iter()
            .map(|(tzid, polygons)| TimezoneBoundary::new(tzid.clone(), tzid, polygons))
            .collect(),
    ))
}

fn load_timezone_boundary_index() -> Result<TimezoneBoundaryIndex, TimezoneMapError> {
    parse_timezone_map_zip(TIMEZONE_MAP_ASSET_BYTES)
}

fn unique_geojson_payload_entry_name(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<String, TimezoneMapError> {
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| TimezoneMapError::ZipEntry {
                name: format!("#{index}"),
                message: error.to_string(),
            })?;
        if !file.is_dir() && is_geojson_payload_entry(file.name()) {
            names.push(file.name().to_string());
        }
    }

    match names.len() {
        0 => Err(TimezoneMapError::NoGeojsonPayloadEntry),
        1 => Ok(names.remove(0)),
        _ => Err(TimezoneMapError::MultipleGeojsonPayloadEntries { entries: names }),
    }
}

fn is_geojson_payload_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".geojson") || lower.ends_with(".json")
}

fn parse_polygon(
    feature_index: usize,
    tzid: &str,
    geometry_type: &str,
    rings: Vec<Vec<Position>>,
) -> Result<TimezonePolygon, TimezoneMapError> {
    if rings.is_empty() {
        return Err(invalid_coordinates(
            feature_index,
            tzid,
            geometry_type,
            "polygon must contain at least one linear ring",
        ));
    }

    let mut parsed_rings = Vec::with_capacity(rings.len());
    for ring in rings {
        if ring.is_empty() {
            return Err(invalid_coordinates(
                feature_index,
                tzid,
                geometry_type,
                "linear ring must contain at least one position",
            ));
        }
        let mut parsed_ring = Vec::with_capacity(ring.len());
        for position in ring {
            let position = position.as_slice();
            let longitude = *position.first().ok_or_else(|| {
                invalid_coordinates(
                    feature_index,
                    tzid,
                    geometry_type,
                    "position is missing numeric longitude",
                )
            })?;
            let latitude = *position.get(1).ok_or_else(|| {
                invalid_coordinates(
                    feature_index,
                    tzid,
                    geometry_type,
                    "position is missing numeric latitude",
                )
            })?;
            if !longitude.is_finite() || !latitude.is_finite() {
                return Err(invalid_coordinates(
                    feature_index,
                    tzid,
                    geometry_type,
                    "position has non-finite coordinate",
                ));
            }
            parsed_ring.push(TimezoneCoordinate::new(longitude, latitude));
        }
        parsed_rings.push(parsed_ring);
    }
    Ok(TimezonePolygon::new(parsed_rings))
}

fn invalid_coordinates(
    feature_index: usize,
    tzid: &str,
    geometry_type: &str,
    detail: &str,
) -> TimezoneMapError {
    TimezoneMapError::InvalidCoordinates {
        feature_index,
        tzid: tzid.to_string(),
        geometry_type: geometry_type.to_string(),
        detail: detail.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimezoneMapCity {
    pub longitude: f64,
    pub latitude: f64,
}

impl TimezoneMapCity {
    pub fn new(longitude: f64, latitude: f64) -> Self {
        Self {
            longitude,
            latitude,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimezoneMapColors {
    pub background: Color,
    pub border: Color,
    pub title: Color,
    pub unselected: Color,
    pub selected: Color,
    pub marker: Color,
}

impl TimezoneMapColors {
    pub fn from_theme(theme: &TundraTheme) -> Self {
        Self {
            background: theme.background,
            border: theme.foreground,
            title: theme.accent,
            unselected: theme.muted,
            selected: Color::White,
            marker: theme.accent,
        }
    }
}

impl Default for TimezoneMapColors {
    fn default() -> Self {
        Self::from_theme(&TundraTheme::default())
    }
}

impl From<&TundraTheme> for TimezoneMapColors {
    fn from(theme: &TundraTheme) -> Self {
        Self::from_theme(theme)
    }
}

impl From<TundraTheme> for TimezoneMapColors {
    fn from(theme: TundraTheme) -> Self {
        Self::from_theme(&theme)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimezoneMapInput<'a> {
    pub boundaries: &'a [TimezoneBoundary],
    pub selected_timezone_id: Option<&'a str>,
    pub selected_boundary_id: Option<&'a str>,
    pub city: Option<TimezoneMapCity>,
    pub colors: TimezoneMapColors,
}

impl<'a> TimezoneMapInput<'a> {
    pub fn new(boundaries: &'a [TimezoneBoundary], colors: impl Into<TimezoneMapColors>) -> Self {
        Self {
            boundaries,
            selected_timezone_id: None,
            selected_boundary_id: None,
            city: None,
            colors: colors.into(),
        }
    }

    pub fn selected_timezone_id(mut self, selected_timezone_id: Option<&'a str>) -> Self {
        self.selected_timezone_id = selected_timezone_id;
        self
    }

    pub fn selected_boundary_id(mut self, selected_boundary_id: Option<&'a str>) -> Self {
        self.selected_boundary_id = selected_boundary_id;
        self
    }

    pub fn city(mut self, longitude: f64, latitude: f64) -> Self {
        self.city = Some(TimezoneMapCity::new(longitude, latitude));
        self
    }
}

#[derive(Debug)]
pub struct TimezoneMapRasterCache {
    entries: RefCell<Vec<RasterCacheEntry>>,
    rasterization_count: Cell<u64>,
    capacity: usize,
}

impl TimezoneMapRasterCache {
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            rasterization_count: Cell::new(0),
            capacity: DEFAULT_CACHE_CAPACITY,
        }
    }

    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    pub fn rasterization_count(&self) -> u64 {
        self.rasterization_count.get()
    }

    fn get_or_rasterize(
        &self,
        key: RasterCacheKey,
        boundaries: &[TimezoneBoundary],
        selected_boundary: Option<&TimezoneBoundary>,
    ) -> RasterizedMap {
        if let Some(entry) = self.entries.borrow().iter().find(|entry| entry.key == key) {
            return entry.raster.clone();
        }

        let raster = if boundaries.is_empty() {
            rasterize_compact_timezone_map(
                key.width,
                key.height_pixels,
                key.selected_boundary_id.as_deref(),
            )
        } else {
            rasterize_boundaries(key.width, key.height_pixels, boundaries, selected_boundary)
        };
        self.rasterization_count
            .set(self.rasterization_count.get().saturating_add(1));

        let mut entries = self.entries.borrow_mut();
        if entries.len() >= self.capacity {
            entries.remove(0);
        }
        entries.push(RasterCacheEntry {
            key,
            raster: raster.clone(),
        });
        raster
    }
}

impl Default for TimezoneMapRasterCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimezoneMapWidget<'a> {
    input: TimezoneMapInput<'a>,
    cache: Option<&'a TimezoneMapRasterCache>,
    border_shape: BorderShape,
}

impl<'a> TimezoneMapWidget<'a> {
    pub fn new(boundaries: &'a [TimezoneBoundary], colors: impl Into<TimezoneMapColors>) -> Self {
        Self::from_input(TimezoneMapInput::new(boundaries, colors))
    }

    pub fn themed(boundaries: &'a [TimezoneBoundary], theme: &TundraTheme) -> Self {
        Self::new(boundaries, TimezoneMapColors::from_theme(theme)).border_shape(theme.border_shape)
    }

    pub fn from_input(input: TimezoneMapInput<'a>) -> Self {
        Self {
            input,
            cache: None,
            border_shape: BorderShape::default(),
        }
    }

    pub fn selected_timezone_id(mut self, selected_timezone_id: Option<&'a str>) -> Self {
        self.input.selected_timezone_id = selected_timezone_id;
        self
    }

    pub fn selected_boundary_id(mut self, selected_boundary_id: Option<&'a str>) -> Self {
        self.input.selected_boundary_id = selected_boundary_id;
        self
    }

    pub fn city(mut self, longitude: f64, latitude: f64) -> Self {
        self.input.city = Some(TimezoneMapCity::new(longitude, latitude));
        self
    }

    pub fn cache(mut self, cache: &'a TimezoneMapRasterCache) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn border_shape(mut self, border_shape: BorderShape) -> Self {
        self.border_shape = border_shape;
        self
    }
}

impl Widget for TimezoneMapWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let colors = self.input.colors;
        let block = Block::default()
            .border_type(self.border_shape.border_type())
            .title("Timezone Map")
            .title_style(
                Style::default()
                    .fg(colors.title)
                    .bg(colors.background)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors.border).bg(colors.background))
            .style(Style::default().fg(colors.unselected).bg(colors.background));
        let inner = block.inner(area);

        block.render(area, buffer);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let selected_boundary = selected_boundary(
            self.input.boundaries,
            self.input.selected_boundary_id,
            self.input.selected_timezone_id,
        );
        let key = RasterCacheKey::new(
            inner.width.saturating_mul(BRAILLE_PIXEL_WIDTH),
            inner.height.saturating_mul(BRAILLE_PIXEL_HEIGHT),
            self.input.boundaries,
            self.input.selected_boundary_id,
            self.input.selected_timezone_id,
            selected_boundary,
        );
        let raster = if let Some(cache) = self.cache {
            cache.get_or_rasterize(key, self.input.boundaries, selected_boundary)
        } else {
            DEFAULT_RASTER_CACHE
                .with(|cache| cache.get_or_rasterize(key, self.input.boundaries, selected_boundary))
        };

        render_raster(inner, &raster, colors, buffer);
        if let Some(city) = self.input.city {
            render_city_marker(inner, city, colors, buffer);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RasterCacheKey {
    width: u16,
    height_pixels: u16,
    boundary_source: usize,
    boundary_count: usize,
    selected_boundary_id: Option<String>,
    selected_timezone_id: Option<String>,
}

impl RasterCacheKey {
    fn new(
        width: u16,
        height_pixels: u16,
        boundaries: &[TimezoneBoundary],
        requested_boundary_id: Option<&str>,
        selected_timezone_id: Option<&str>,
        selected_boundary: Option<&TimezoneBoundary>,
    ) -> Self {
        Self {
            width,
            height_pixels,
            boundary_source: boundaries.as_ptr() as usize,
            boundary_count: boundaries.len(),
            selected_boundary_id: selected_boundary
                .map(|boundary| boundary.id.clone())
                .or_else(|| requested_boundary_id.map(ToOwned::to_owned)),
            selected_timezone_id: selected_timezone_id.map(ToOwned::to_owned),
        }
    }
}

#[derive(Debug, Clone)]
struct RasterCacheEntry {
    key: RasterCacheKey,
    raster: RasterizedMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RasterizedMap {
    width: u16,
    height_pixels: u16,
    samples: Vec<RasterSample>,
}

impl RasterizedMap {
    fn new(width: u16, height_pixels: u16) -> Self {
        Self {
            width,
            height_pixels,
            samples: vec![RasterSample::Empty; usize::from(width) * usize::from(height_pixels)],
        }
    }

    fn sample(&self, x: u16, y: u16) -> RasterSample {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        self.samples
            .get(index)
            .copied()
            .unwrap_or(RasterSample::Empty)
    }

    fn set_sample(&mut self, x: u16, y: u16, sample: RasterSample) {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        if let Some(existing) = self.samples.get_mut(index) {
            *existing = sample;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RasterSample {
    Empty,
    Base,
    Selected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactTimezoneMap {
    width: usize,
    height: usize,
    base: Vec<bool>,
    overlays: BTreeMap<String, Vec<bool>>,
}

impl CompactTimezoneMap {
    fn fallback() -> Self {
        Self {
            width: 1,
            height: 1,
            base: vec![true],
            overlays: BTreeMap::new(),
        }
    }

    fn selected_overlay(&self, selected_boundary_id: Option<&str>) -> Option<&[bool]> {
        selected_boundary_id.and_then(|id| self.overlays.get(id).map(Vec::as_slice))
    }

    fn sample(mask: &[bool], width: usize, x: usize, y: usize) -> bool {
        mask.get(y.saturating_mul(width).saturating_add(x))
            .copied()
            .unwrap_or(false)
    }
}

fn compact_timezone_map() -> &'static CompactTimezoneMap {
    COMPACT_TIMEZONE_MAP.get_or_init(|| {
        parse_compact_timezone_map(TIMEZONE_MAP_RASTER_ASSET)
            .unwrap_or_else(|_| CompactTimezoneMap::fallback())
    })
}

fn parse_compact_timezone_map(contents: &str) -> Result<CompactTimezoneMap, String> {
    let mut width = None;
    let mut height = None;
    let mut current_section: Option<String> = None;
    let mut current_rows: Vec<String> = Vec::new();
    let mut sections: BTreeMap<String, Vec<bool>> = BTreeMap::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() || (current_section.is_none() && line.starts_with('#')) {
            continue;
        }
        if let Some(value) = line.strip_prefix("width=") {
            width = Some(
                value
                    .parse::<usize>()
                    .map_err(|error| format!("invalid width: {error}"))?,
            );
            continue;
        }
        if let Some(value) = line.strip_prefix("height=") {
            height = Some(
                value
                    .parse::<usize>()
                    .map_err(|error| format!("invalid height: {error}"))?,
            );
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            finish_compact_map_section(
                current_section.take(),
                &mut current_rows,
                width,
                height,
                &mut sections,
            )?;
            current_section = Some(line[1..line.len() - 1].to_string());
            continue;
        }
        if current_section.is_none() {
            return Err(format!("raster row before section: {line}"));
        }
        current_rows.push(line.to_string());
    }

    finish_compact_map_section(
        current_section,
        &mut current_rows,
        width,
        height,
        &mut sections,
    )?;

    let width = width.ok_or_else(|| "missing width".to_string())?;
    let height = height.ok_or_else(|| "missing height".to_string())?;
    let base = sections
        .remove("base")
        .ok_or_else(|| "missing base section".to_string())?;

    Ok(CompactTimezoneMap {
        width,
        height,
        base,
        overlays: sections,
    })
}

fn finish_compact_map_section(
    section: Option<String>,
    rows: &mut Vec<String>,
    width: Option<usize>,
    height: Option<usize>,
    sections: &mut BTreeMap<String, Vec<bool>>,
) -> Result<(), String> {
    let Some(section) = section else {
        return Ok(());
    };
    let width = width.ok_or_else(|| format!("section {section} appears before width"))?;
    let height = height.ok_or_else(|| format!("section {section} appears before height"))?;
    if rows.len() != height {
        return Err(format!(
            "section {section} has {} rows, expected {height}",
            rows.len()
        ));
    }

    let mut mask = Vec::with_capacity(width.saturating_mul(height));
    for (row_index, row) in rows.drain(..).enumerate() {
        if row.len() != width {
            return Err(format!(
                "section {section} row {row_index} has {} columns, expected {width}",
                row.len()
            ));
        }
        for character in row.bytes() {
            match character {
                b'.' => mask.push(false),
                b'#' => mask.push(true),
                other => {
                    return Err(format!(
                        "section {section} contains invalid raster byte {other}"
                    ));
                }
            }
        }
    }

    sections.insert(section, mask);
    Ok(())
}

fn selected_boundary<'a>(
    boundaries: &'a [TimezoneBoundary],
    selected_boundary_id: Option<&str>,
    selected_timezone_id: Option<&str>,
) -> Option<&'a TimezoneBoundary> {
    selected_boundary_id
        .and_then(|id| boundaries.iter().find(|boundary| boundary.id == id))
        .or_else(|| {
            selected_timezone_id.and_then(|id| {
                let boundary_id = boundary_id_for_timezone(id);
                boundaries
                    .iter()
                    .find(|boundary| boundary.id == boundary_id || boundary.timezone_id == id)
            })
        })
}

fn rasterize_boundaries(
    width: u16,
    height_pixels: u16,
    boundaries: &[TimezoneBoundary],
    selected_boundary: Option<&TimezoneBoundary>,
) -> RasterizedMap {
    let mut raster = RasterizedMap::new(width, height_pixels);
    if width == 0 || height_pixels == 0 {
        return raster;
    }

    let prepared_boundaries = boundaries
        .iter()
        .map(PreparedBoundary::new)
        .collect::<Vec<_>>();
    let selected_boundary = selected_boundary.map(PreparedBoundary::new);
    for y in 0..height_pixels {
        for x in 0..width {
            let lon = raster_x_to_longitude(x, width);
            let lat = raster_y_to_latitude(y, height_pixels);

            if selected_boundary
                .as_ref()
                .is_some_and(|boundary| boundary.contains(lon, lat))
            {
                raster.set_sample(x, y, RasterSample::Selected);
            } else if prepared_boundaries
                .iter()
                .any(|boundary| boundary.contains(lon, lat))
            {
                raster.set_sample(x, y, RasterSample::Base);
            }
        }
    }

    raster
}

fn rasterize_compact_timezone_map(
    width: u16,
    height_pixels: u16,
    selected_boundary_id: Option<&str>,
) -> RasterizedMap {
    let mut raster = RasterizedMap::new(width, height_pixels);
    if width == 0 || height_pixels == 0 {
        return raster;
    }

    let map = compact_timezone_map();
    let selected = map.selected_overlay(selected_boundary_id);
    for y in 0..height_pixels {
        let source_y = scaled_sample_index(y, height_pixels, map.height);
        for x in 0..width {
            let source_x = scaled_sample_index(x, width, map.width);
            let selected_sample = selected.is_some_and(|mask| {
                CompactTimezoneMap::sample(mask, map.width, source_x, source_y)
            });
            if selected_sample {
                raster.set_sample(x, y, RasterSample::Selected);
            } else if CompactTimezoneMap::sample(&map.base, map.width, source_x, source_y) {
                raster.set_sample(x, y, RasterSample::Base);
            }
        }
    }

    raster
}

fn scaled_sample_index(position: u16, target_span: u16, source_span: usize) -> usize {
    if target_span == 0 || source_span == 0 {
        return 0;
    }
    let numerator =
        (usize::from(position).saturating_mul(2).saturating_add(1)).saturating_mul(source_span);
    let denominator = usize::from(target_span).saturating_mul(2);
    (numerator / denominator).min(source_span.saturating_sub(1))
}

fn render_raster(
    area: Rect,
    raster: &RasterizedMap,
    colors: TimezoneMapColors,
    buffer: &mut Buffer,
) {
    for row in 0..area.height.min(raster.height_pixels / BRAILLE_PIXEL_HEIGHT) {
        for column in 0..area.width.min(raster.width / BRAILLE_PIXEL_WIDTH) {
            let (symbol, style) = braille_cell(column, row, raster, colors);
            if let Some(cell) = buffer.cell_mut((area.x + column, area.y + row)) {
                cell.set_symbol(&symbol);
                cell.set_style(style);
            }
        }
    }
}

fn braille_cell(
    column: u16,
    row: u16,
    raster: &RasterizedMap,
    colors: TimezoneMapColors,
) -> (String, Style) {
    let mut base_bits = 0u8;
    let mut selected_bits = 0u8;
    let pixel_x = column.saturating_mul(BRAILLE_PIXEL_WIDTH);
    let pixel_y = row.saturating_mul(BRAILLE_PIXEL_HEIGHT);

    for dy in 0..BRAILLE_PIXEL_HEIGHT {
        for dx in 0..BRAILLE_PIXEL_WIDTH {
            let dot = BRAILLE_DOTS[usize::from(dy)][usize::from(dx)];
            match raster.sample(pixel_x.saturating_add(dx), pixel_y.saturating_add(dy)) {
                RasterSample::Selected => selected_bits |= dot,
                RasterSample::Base => base_bits |= dot,
                RasterSample::Empty => {}
            }
        }
    }

    if selected_bits != 0 {
        return (
            braille_symbol(selected_bits),
            Style::default().fg(colors.selected).bg(colors.background),
        );
    }
    if base_bits != 0 {
        return (
            braille_symbol(base_bits),
            Style::default().fg(colors.unselected).bg(colors.background),
        );
    }
    (
        " ".to_string(),
        Style::default().fg(colors.unselected).bg(colors.background),
    )
}

fn braille_symbol(bits: u8) -> String {
    char::from_u32(0x2800 + u32::from(bits))
        .unwrap_or(' ')
        .to_string()
}

fn render_city_marker(
    area: Rect,
    city: TimezoneMapCity,
    colors: TimezoneMapColors,
    buffer: &mut Buffer,
) {
    let Some((x, y)) = project_to_cell(area, city.longitude, city.latitude) else {
        return;
    };
    if let Some(cell) = buffer.cell_mut((x, y)) {
        let background = cell.bg;
        cell.set_symbol(CITY_MARKER_SYMBOL);
        cell.set_style(Style::default().fg(colors.marker).bg(background));
    }
}

fn project_to_cell(area: Rect, longitude: f64, latitude: f64) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 || !longitude.is_finite() || !latitude.is_finite() {
        return None;
    }

    let x_ratio = (longitude.clamp(-180.0, 180.0) + 180.0) / 360.0;
    let y_ratio = (90.0 - latitude.clamp(-90.0, 90.0)) / 180.0;
    let x = area.x + scaled_index(x_ratio, area.width);
    let y = area.y + scaled_index(y_ratio, area.height);
    Some((x, y))
}

fn scaled_index(ratio: f64, span: u16) -> u16 {
    let last = span.saturating_sub(1);
    ((ratio * f64::from(span)).floor() as u16).min(last)
}

fn raster_x_to_longitude(x: u16, width: u16) -> f64 {
    -180.0 + ((f64::from(x) + 0.5) / f64::from(width)) * 360.0
}

fn raster_y_to_latitude(y: u16, height_pixels: u16) -> f64 {
    90.0 - ((f64::from(y) + 0.5) / f64::from(height_pixels)) * 180.0
}

#[derive(Debug, Clone)]
struct PreparedBoundary<'a> {
    polygons: Vec<PreparedPolygon<'a>>,
}

impl<'a> PreparedBoundary<'a> {
    fn new(boundary: &'a TimezoneBoundary) -> Self {
        Self {
            polygons: boundary.polygons.iter().map(PreparedPolygon::new).collect(),
        }
    }

    fn contains(&self, longitude: f64, latitude: f64) -> bool {
        self.polygons
            .iter()
            .any(|polygon| polygon.contains(longitude, latitude))
    }
}

#[derive(Debug, Clone)]
struct PreparedPolygon<'a> {
    rings: Vec<PreparedRing<'a>>,
}

impl<'a> PreparedPolygon<'a> {
    fn new(polygon: &'a TimezonePolygon) -> Self {
        Self {
            rings: polygon.rings.iter().map(PreparedRing::new).collect(),
        }
    }

    fn contains(&self, longitude: f64, latitude: f64) -> bool {
        let Some(exterior) = self.rings.first() else {
            return false;
        };
        exterior.contains(longitude, latitude)
            && !self
                .rings
                .iter()
                .skip(1)
                .any(|hole| hole.contains(longitude, latitude))
    }
}

#[derive(Debug, Clone)]
struct PreparedRing<'a> {
    points: &'a [TimezoneCoordinate],
    min_longitude: f64,
    max_longitude: f64,
    min_latitude: f64,
    max_latitude: f64,
    wraps_dateline: bool,
}

impl<'a> PreparedRing<'a> {
    fn new(points: &'a Vec<TimezoneCoordinate>) -> Self {
        let mut min_longitude = f64::INFINITY;
        let mut max_longitude = f64::NEG_INFINITY;
        let mut min_latitude = f64::INFINITY;
        let mut max_latitude = f64::NEG_INFINITY;

        for point in points {
            min_longitude = min_longitude.min(point.longitude);
            max_longitude = max_longitude.max(point.longitude);
            min_latitude = min_latitude.min(point.latitude);
            max_latitude = max_latitude.max(point.latitude);
        }

        let wraps_dateline = max_longitude - min_longitude > 180.0;
        Self {
            points,
            min_longitude,
            max_longitude,
            min_latitude,
            max_latitude,
            wraps_dateline,
        }
    }

    fn contains(&self, longitude: f64, latitude: f64) -> bool {
        if self.points.len() < 3
            || latitude < self.min_latitude
            || latitude > self.max_latitude
            || (!self.wraps_dateline
                && (longitude < self.min_longitude || longitude > self.max_longitude))
        {
            return false;
        }

        let mut inside = false;
        let mut previous = self.points.len() - 1;
        for current in 0..self.points.len() {
            let current_point = self.points[current];
            let previous_point = self.points[previous];
            let current_x = unwrap_longitude(current_point.longitude, longitude);
            let previous_x = unwrap_longitude(previous_point.longitude, longitude);
            let current_y = current_point.latitude;
            let previous_y = previous_point.latitude;

            if ((current_y > latitude) != (previous_y > latitude))
                && (longitude
                    < (previous_x - current_x) * (latitude - current_y) / (previous_y - current_y)
                        + current_x)
            {
                inside = !inside;
            }
            previous = current;
        }
        inside
    }
}

fn unwrap_longitude(longitude: f64, reference: f64) -> f64 {
    let mut longitude = longitude;
    while longitude - reference > 180.0 {
        longitude -= 360.0;
    }
    while longitude - reference < -180.0 {
        longitude += 360.0;
    }
    longitude
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::widgets::Widget;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn boundary_id_for_timezone_maps_utc_aliases_to_etc_gmt() {
        assert_eq!(boundary_id_for_timezone("UTC"), "Etc/GMT");
        assert_eq!(boundary_id_for_timezone("Etc/UTC"), "Etc/GMT");
        assert_eq!(
            boundary_id_for_timezone("America/Los_Angeles"),
            "America/Los_Angeles"
        );
    }

    #[test]
    fn parses_polygon_and_multipolygon_features_by_tzid() {
        let index = parse_timezone_feature_collection(
            r#"{
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "properties": { "tzid": "Etc/GMT" },
                        "geometry": {
                            "type": "Polygon",
                            "coordinates": [[[-10, -5], [10, -5], [10, 5], [-10, 5], [-10, -5]]]
                        }
                    },
                    {
                        "type": "Feature",
                        "properties": { "tzid": "America/Los_Angeles" },
                        "geometry": {
                            "type": "MultiPolygon",
                            "coordinates": [
                                [[[-120, 30], [-110, 30], [-110, 40], [-120, 40], [-120, 30]]],
                                [[[-125, 45], [-115, 45], [-115, 50], [-125, 50], [-125, 45]]]
                            ]
                        }
                    }
                ]
            }"#,
        )
        .expect("valid GeoJSON should parse");

        assert_eq!(index.len(), 2);
        assert_eq!(index.polygons_for_timezone("UTC").unwrap().len(), 1);
        assert_eq!(
            index
                .polygons_for_timezone("America/Los_Angeles")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            index.polygons_for_timezone("UTC").unwrap()[0].rings[0][0],
            TimezoneCoordinate::new(-10.0, -5.0)
        );
    }

    #[test]
    fn ignores_unknown_geometry_types() {
        let index = parse_timezone_feature_collection(
            r#"{
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "properties": { "tzid": "Etc/GMT" },
                        "geometry": { "type": "Point", "coordinates": [0, 0] }
                    }
                ]
            }"#,
        )
        .expect("unknown geometry should be ignored");

        assert!(index.is_empty());
    }

    #[test]
    fn missing_tzid_is_a_parse_error() {
        let error = parse_timezone_feature_collection(
            r#"{
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "properties": {},
                        "geometry": { "type": "Point", "coordinates": [0, 0] }
                    }
                ]
            }"#,
        )
        .expect_err("missing tzid should fail");

        assert_eq!(error, TimezoneMapError::MissingTzid { feature_index: 0 });
    }

    #[test]
    fn parse_zip_requires_one_json_or_geojson_entry() {
        let error = parse_timezone_map_zip(&zip_with_entries(&[
            ("one.geojson", "{}"),
            ("two.json", "{}"),
        ]))
        .expect_err("ambiguous zip should fail");

        assert_eq!(
            error,
            TimezoneMapError::MultipleGeojsonPayloadEntries {
                entries: vec!["one.geojson".to_string(), "two.json".to_string()]
            }
        );
    }

    #[test]
    fn parse_zip_reads_unique_json_entry() {
        let bytes = zip_with_entries(&[(
            "combined-with-oceans.json",
            r#"{
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "properties": { "tzid": "Etc/GMT" },
                        "geometry": {
                            "type": "Polygon",
                            "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]]
                        }
                    }
                ]
            }"#,
        )]);
        let index = parse_timezone_map_zip(&bytes).expect("unique JSON GeoJSON zip should parse");

        assert_eq!(index.polygons_for_timezone("UTC").unwrap().len(), 1);
    }

    #[test]
    fn compact_raster_asset_includes_world_base_and_catalog_overlays() {
        let map = parse_compact_timezone_map(TIMEZONE_MAP_RASTER_ASSET)
            .expect("bundled compact timezone raster should parse");

        assert_eq!(map.width, 240);
        assert_eq!(map.height, 120);
        assert!(map.base.iter().filter(|cell| **cell).count() > 5_000);
        assert!(
            map.overlays
                .get("Asia/Shanghai")
                .is_some_and(|mask| mask.iter().any(|cell| *cell))
        );
        assert!(
            map.overlays
                .get("Etc/GMT")
                .is_some_and(|mask| mask.iter().any(|cell| *cell))
        );
    }

    #[test]
    fn widget_draws_bordered_panel_selected_polygon_and_marker() {
        let boundaries = sample_boundaries();
        let cache = TimezoneMapRasterCache::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 48, 14));
        let colors = test_colors();

        TimezoneMapWidget::new(&boundaries, colors)
            .selected_timezone_id(Some("Asia/Tokyo"))
            .selected_boundary_id(Some("asia-tokyo"))
            .city(139.6917, 35.6895)
            .cache(&cache)
            .render(buffer.area, &mut buffer);

        assert!(buffer_text(&buffer).contains("Timezone Map"));
        assert!(buffer.content().iter().any(|cell| {
            cell.symbol() != " " && (cell.fg == Color::White || cell.bg == Color::White)
        }));
        assert!(buffer.content().iter().any(|cell| {
            cell.symbol() != " " && (cell.fg == Color::DarkGray || cell.bg == Color::DarkGray)
        }));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.symbol() == CITY_MARKER_SYMBOL && cell.fg == Color::Cyan)
        );
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "╭");
    }

    #[test]
    fn themed_widget_uses_themes_square_border_shape() {
        let theme = TundraTheme::default().with_border_shape(BorderShape::Square);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 48, 14));

        TimezoneMapWidget::themed(&sample_boundaries(), &theme).render(buffer.area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "┌");
    }

    #[test]
    fn missing_selected_boundary_keeps_base_map_and_marker() {
        let boundaries = sample_boundaries();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 48, 14));
        let colors = test_colors();

        TimezoneMapWidget::new(&boundaries, colors)
            .selected_timezone_id(Some("Etc/Missing"))
            .selected_boundary_id(Some("missing"))
            .city(-74.0060, 40.7128)
            .render(buffer.area, &mut buffer);

        assert!(buffer.content().iter().any(|cell| {
            cell.symbol() != " " && (cell.fg == Color::DarkGray || cell.bg == Color::DarkGray)
        }));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.symbol() == CITY_MARKER_SYMBOL && cell.fg == Color::Cyan)
        );
    }

    #[test]
    fn cache_reuses_same_size_and_selection_raster() {
        let boundaries = sample_boundaries();
        let cache = TimezoneMapRasterCache::default();
        let colors = test_colors();

        for city in [(139.6917, 35.6895), (140.0, 36.0)] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 48, 14));
            TimezoneMapWidget::new(&boundaries, colors)
                .selected_boundary_id(Some("asia-tokyo"))
                .city(city.0, city.1)
                .cache(&cache)
                .render(buffer.area, &mut buffer);
        }

        assert_eq!(cache.rasterization_count(), 1);
        assert_eq!(cache.len(), 1);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 48, 14));
        TimezoneMapWidget::new(&boundaries, colors)
            .selected_boundary_id(Some("america-new-york"))
            .city(-74.0060, 40.7128)
            .cache(&cache)
            .render(buffer.area, &mut buffer);

        assert_eq!(cache.rasterization_count(), 2);
        assert_eq!(cache.len(), 2);
    }

    fn sample_boundaries() -> Vec<TimezoneBoundary> {
        vec![
            TimezoneBoundary::new(
                "america-new-york",
                "America/New_York",
                vec![rectangle(-100.0, 25.0, -60.0, 50.0)],
            ),
            TimezoneBoundary::new(
                "asia-tokyo",
                "Asia/Tokyo",
                vec![rectangle(120.0, 20.0, 150.0, 50.0)],
            ),
        ]
    }

    fn zip_with_entries(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, contents) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn rectangle(west: f64, south: f64, east: f64, north: f64) -> TimezonePolygon {
        TimezonePolygon::from_exterior(vec![
            TimezoneCoordinate::new(west, south),
            TimezoneCoordinate::new(east, south),
            TimezoneCoordinate::new(east, north),
            TimezoneCoordinate::new(west, north),
        ])
    }

    fn test_colors() -> TimezoneMapColors {
        TimezoneMapColors {
            background: Color::Black,
            border: Color::Gray,
            title: Color::Cyan,
            unselected: Color::DarkGray,
            selected: Color::White,
            marker: Color::Cyan,
        }
    }

    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content().iter().map(|cell| cell.symbol()).collect()
    }
}
