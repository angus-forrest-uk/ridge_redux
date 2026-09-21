# Development recipes. Run `just` to list them.

# List the recipes
default:
    @just --list

# Run the app on localhost, serving web/ from disk so frontend edits show on reload
run *args:
    cargo run --release -p ridge-server -- --web-dir web {{args}}

# Run the app offline against the fixture tiles (fetch them first with `just fixtures`)
offline: fixtures
    cargo run --release -p ridge-server -- --web-dir web --fixture-dir fixtures/srtm

# Render headless to SVG, e.g. `just render --bbox "..." --out out.svg`
render *args:
    cargo run --release -p ridge-core --bin render -- {{args}}

# Fetch the SRTM tiles the golden parity test needs
fixtures:
    scripts/fetch_fixtures.sh

# Rust tests: unit, API integration and the golden upstream parity test
test:
    cargo test --workspace --release

# Frontend logic and JS/Rust pipeline parity
test-web:
    node scripts/test_frontend.mjs
    node scripts/parity_frontend.mjs

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
ci: fmt-check clippy test test-web

# Regenerate the README screenshots (the app must be running; set BASE_URL / CHROMIUM_PATH as needed)
screenshots:
    npm install --silent --no-save --no-package-lock --prefix scripts playwright-core
    node scripts/screenshots.mjs
