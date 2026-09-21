# Development recipes. Run `just` to list them.

# List the recipes
default:
    @just --list

# Build the frontend (web/, Astro + Solid) into web/dist, which the server serves
web:
    npm --prefix web install --silent
    npm --prefix web run build

# Build the frontend, then run the app on localhost
run *args: web
    cargo run --release -p ridge_redux -- {{args}}

# Run the app offline against the fixture tiles
offline: web fixtures
    cargo run --release -p ridge_redux -- --fixture-dir fixtures/srtm

# Frontend dev server with live reload on :4321; its API calls go to `just run` on :8420
web-dev:
    npm --prefix web install --silent
    npm --prefix web run dev

# Render headless to SVG, e.g. `just render --bbox "..." --out out.svg`
render *args:
    cargo run --release -p ridge-core --bin render -- {{args}}

# Fetch the SRTM tiles the golden parity test needs
fixtures:
    scripts/fetch_fixtures.sh

# Rust tests: unit, API integration and the golden upstream parity test
test:
    cargo test --workspace --release

# Frontend tests: app state, and the TS pipeline against the Rust one (bit-for-bit)
test-web:
    cargo run --release -q -p ridge-core --example dump_plane_fixture
    npm --prefix web install --silent
    npm --prefix web test

# Type-check the frontend
check-web:
    npm --prefix web install --silent
    npm --prefix web run check

# Format the code
fmt:
    cargo fmt --all

# Check formatting without changing anything
fmt-check:
    cargo fmt --all --check

# Lint, failing on any warning
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Everything CI runs
ci: fmt-check clippy test check-web test-web

# Regenerate the README screenshots (the app must be running; set BASE_URL / CHROMIUM_PATH as needed)
screenshots:
    npm install --silent --no-save --no-package-lock --prefix scripts playwright-core
    node scripts/screenshots.mjs
