# Remaining Tasks

Checked items were on the original task list but have since been completed (verified against code and commits).

## Core

- [x] File watcher debouncing
- [x] Glob pattern filtering for watched files
- [x] Fake adapter responses when IDE reconnects to a live session
- [x] Intercept disconnect/terminate from IDE in headless mode
- [x] Build error handling (BuildingFailed status, retry on next file change)
- [x] Program output forwarding (mpsc channel from Runtime to Proxy)
- [x] Attach-to-launch conversion for headless rebuild
- [ ] Graceful shutdown on Ctrl+C (signal handling)
- [ ] gitignore support in file watcher (config field exists, not wired up yet)
- [ ] env_file loading for program environment variables (config field exists, not wired up yet)

## Config & CLI

- [ ] Auto-detect adapter binary path based on installed editor extensions
- [ ] `dap-watch init` command to generate per-language config files (scaffolding exists)
- [ ] Config validation on startup

## Testing

- [ ] Integration tests with full end-to-end DAP sessions
- [ ] Expand unit test coverage (currently 27 tests)

## Editor Support

- [ ] VS Code extension: polish, binary resolution, marketplace listing
- [ ] Editor setup guides for Neovim and other DAP clients
- [ ] Example configurations for Rust, Go, C++

## CI/CD

- [ ] GitHub Actions pipeline (build, test, clippy)
- [ ] Release automation

## Documentation

- [ ] Config reference
- [ ] Architecture walkthrough
