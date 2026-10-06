# Pacman query fixtures

The small named fixtures model installed, foreign, repository-priority, epoch,
pkgrel, architecture-independent and literal-search cases.

The `real-*` package fixtures are output captures from stock pacman 7.0.0
queried against isolated synthetic local/sync databases with `LC_ALL=C.UTF-8`,
`LANG=C.UTF-8`, `COLUMNS=64`, and `--color never`. Only the final empty record-separator line is normalized for patch whitespace checks.
They preserve actual wrapped
descriptions, optional dependencies, packager details, annotations and UTF-8.
Only metadata queries produced these captures; the host package databases were
not used or changed. The fixture package and packager are synthetic.

The `real-*config` fixtures are exact verbose configuration dumps from the
official pacman-conf shipped with pacman 7.0.0. They cover default/all repository
Usage and restricted Usage supplied through an Include file. Includes and
default values are resolved by the native helper. Cached previews require
Search, Install and Upgrade all enabled in every configured repository; other
configurations retain local installed details and removal without guessing
repository candidates.

The runtime provider never accepts database/configuration paths, environment
overrides, or executable paths from a caller. The isolated verifier's alternate
paths and terminal width exist only to exercise real pacman output safely.
