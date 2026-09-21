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
