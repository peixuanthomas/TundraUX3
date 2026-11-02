# UI assets

## Navigation icons

`icons/back.txt` contains the solid left-pointing triangle used by the Shell's
Back button. It is embedded at build time and drawn by the shared themed Button,
including on text-only terminals.

## Timezone map data

The timezone map in setup and Region & Time uses boundary data from
timezone-boundary-builder.

- Source release: 2026b
- Asset: `timezones-with-oceans.geojson.zip`
- URL: https://github.com/evansiroky/timezone-boundary-builder/releases/tag/2026b
- Local archive: `timezones-with-oceans-2026b.geojson.zip`
- Archive member: `combined-with-oceans.json`
- SHA-256: `011f5e5336ea8b10521d07c5b977e295dd9a6efdcaba32bb79c63f620122e618`
- Runtime raster: `timezone-map-2026b-raster.txt`

The runtime raster is a compact equirectangular grid generated from the source
GeoJSON. The interface draws this text asset directly instead of parsing the
full archive for every map render.

The generated boundary data is licensed under the Open Data Commons Open
Database License (ODbL). The source project's MIT license applies to its code;
the bundled data follows the upstream data license.
