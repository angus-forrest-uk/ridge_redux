# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge-core-v0.2.0...ridge-core-v0.3.0) - 2026-09-22

### Added

- use each platform's cache directory, with a clean-cache command

## [0.2.0](https://github.com/angus-forrest-uk/ridge_redux/compare/ridge-core-v0.1.0...ridge-core-v0.2.0) - 2026-09-22

### Other

- *(ridge-core)* give the library its own README

## [0.1.0](https://github.com/angus-forrest-uk/ridge_redux/releases/tag/ridge-core-v0.1.0) - 2026-09-21

### Added

- print the app URL on start and open it in the browser
- embed the built frontend in the binary
- *(web)* add map tools and README modal, restyle UI, add screenshots

### Fixed

- read SRTM samples with as_chunks
- clear clippy warnings in ridge-core tests and examples

### Other

- add badges and document packaging and releases
- make both crates publishable to crates.io
- fail the golden parity test when required fixtures are missing
- rename the server crate to ridge_redux
- apply rustfmt
- *(web)* port the frontend to Astro and SolidJS
- remove em dashes from README
- rewrite README intro, install and local-server rationale
- Initial commit: Rust port of ridge_map
