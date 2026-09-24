# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.4.0...ridge_redux-v0.4.1) - 2026-09-24

### Added

- publish a docker image to ghcr.io

## [0.4.0](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.3.2...ridge_redux-v0.4.0) - 2026-09-23

### Added

- *(render)* make the legacy frame crop a clip_to_land toggle
- *(web)* export and import the view configuration

### Fixed

- *(api)* carve the plane view window from the disc centre
- *(render)* frame the requested window so water cannot crop the scene

### Other

- describe the frame toggle and correct the stale test counts

## [0.3.2](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.3.1...ridge_redux-v0.3.2) - 2026-09-22

### Other

- *(ridge-core)* prefer iterator and ndarray idioms over index loops
- *(ridge-core)* pin the frozen ports with Python-reference fixtures

## [0.3.1](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.3.0...ridge_redux-v0.3.1) - 2026-09-22

### Added

- *(ci)* measure coverage and publish a badge

## [0.3.0](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.2.0...ridge_redux-v0.3.0) - 2026-09-22

### Added

- use each platform's cache directory, with a clean-cache command

### Other

- keep the prefetch switch out of ServerConfig

## [0.2.0](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge_redux-v0.1.0...ridge_redux-v0.2.0) - 2026-09-22

### Added

- prefetch the default scene on startup

### Other

- use absolute links in the README

## [0.1.0](https://github.com/angus-forrest-uk/ridge_redux/releases/tag/ridge_redux-v0.1.0) - 2026-09-21

### Added

- fall back to a free port when 8420 is taken
- print the app URL on start and open it in the browser
- embed the built frontend in the binary
- *(web)* add map tools and README modal, restyle UI, add screenshots

### Other

- *(deps)* bump tower-http from 0.6.11 to 0.7.1 ([#1](https://github.com/angus-forrest-uk/ridge_redux/pull/1))
- add badges and document packaging and releases
- make both crates publishable to crates.io
- rename the server crate to ridge_redux
- *(web)* port the frontend to Astro and SolidJS
- remove em dashes from README
- rewrite README intro, install and local-server rationale
- Initial commit: Rust port of ridge_map
