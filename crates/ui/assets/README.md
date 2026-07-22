# Timezone map data

This directory contains timezone boundary data used by the first-run setup
timezone map.

- Source: timezone-boundary-builder
- Release: 2026b
- Asset: `timezones-with-oceans.geojson.zip`
- URL: https://github.com/evansiroky/timezone-boundary-builder/releases/tag/2026b
- Local file: `timezones-with-oceans-2026b.geojson.zip`
- Archive member: `combined-with-oceans.json`
- SHA-256: `011f5e5336ea8b10521d07c5b977e295dd9a6efdcaba32bb79c63f620122e618`
- Runtime raster: `timezone-map-2026b-raster.txt`

The runtime raster is a compact equirectangular grid generated from the source
GeoJSON for the first-run setup map. The application renders this small text
asset directly so the setup wizard does not parse the full GeoJSON archive on
the first timezone page draw.

The generated timezone boundary data is licensed under the Open Data Commons
Open Database License (ODbL). The source project code is MIT licensed, but the
data file included here follows the data license documented by
timezone-boundary-builder.
