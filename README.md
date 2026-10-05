# dap-watch

A debug proxy for compiled languages. Sits between your editor and debug adapter, watches your source files, and when something changes: rebuilds the project, restarts the debug adapter, and replays your debug state (breakpoints, launch config, etc.) so your session picks up where it left off.

Written in Rust with Tokio. Single binary.

> **Work in progress.** Core functionality works end to end. See [TASKS.md](TASKS.md) for what's remaining.

<p align="center">
  <img src="assets/demo.svg?v=3" alt="dap-watch in action" width="100%">
</p>

## Why

Coming from Node.js, I was used to VS Code's debug experience where the editor automatically reconnects to a restarted process. You change a file, nodemon restarts, and VS Code picks the session right back up with your breakpoints intact. You barely notice it happened.

When debugging compiled languages (Rust, Go), that experience is gone. Every code change meant: stop the debugger, rebuild, restart, re-attach, hope your breakpoints are still there. That friction adds up fast when you're in a tight change-debug loop, tweaking values or narrowing down a bug. That gap bugged me enough to go down the rabbit hole of how DAP actually works, and dap-watch is what came out of it.

Even the Node setup has a gap though: the program starts running before the debugger re-attaches, so you can miss the first few lines of execution. dap-watch doesn't have that problem since it replays breakpoints _before_ launching the program.

## How It Works

dap-watch is a [DAP](https://microsoft.github.io/debug-adapter-protocol/) (Debug Adapter Protocol) proxy. DAP is the standard protocol editors like VS Code and Neovim use to communicate with language debuggers (codelldb for Rust/C++, delve for Go, etc.).

```
Your Editor (VS Code, Neovim, ...)
        |
        v
    dap-watch (proxy)  <-- file watcher
        |
        v
  Debug Adapter (codelldb, delve, ...)
        |
        v
    Your Program
```

It intercepts all DAP messages between editor and adapter, captures the ones that define the debug state (breakpoints, launch config, initialization), and when a file changes:

1. Kills the running adapter and program
2. Rebuilds using your configured build command
3. Spawns a new debug adapter
4. Replays the captured state to the new adapter
5. Suppresses the replay responses so the editor never notices

From the editor's perspective, nothing happened. Your breakpoints are where you left them.

## Modes

### Headless

dap-watch runs independently and owns the program's lifecycle. Your editor connects and disconnects via TCP whenever you want - the program keeps running between debug sessions.

Good for long-running services like web servers or APIs.

```sh
dap-watch --port 2500 ./target/debug/my-server
```

When the editor disconnects, dap-watch clears all breakpoints, resumes execution, and keeps the program running. When the editor reconnects, dap-watch fakes the adapter handshake using cached responses so the editor thinks it's a fresh session.

### Launch

The editor spawns dap-watch as a child process and communicates over stdin/stdout. From the editor's perspective, dap-watch _is_ the debug adapter. Used with the included VS Code extension or equivalent launch configuration.

## Architecture

| Module         | What it does                                                                                  |
| -------------- | --------------------------------------------------------------------------------------------- |
| `proxy`        | Main event loop (`tokio::select!`). Routes messages, manages state machine, triggers rebuilds |
| `proxy_state`  | Captures and replays debug state: breakpoints, launch config, initialization sequence         |
| `dap_message`  | Parses and constructs DAP protocol messages (requests, responses, events)                     |
| `dap_stream`   | Async read/write with Content-Length framing (DAP's wire format)                              |
| `runtime`      | Spawns and kills the debug adapter and user program as child processes                        |
| `file_watcher` | File watching with OS-native events (FSEvents/inotify), glob pattern matching, debouncing     |
| `ide`          | IDE connection handling: TCP listener for headless, stdin/stdout for launch                   |
| `config`       | Layered configuration: CLI args > TOML file > auto-detected defaults                          |
| `cli`          | Command line interface (clap)                                                                 |
| `logger`       | Colored, source-tagged, leveled output                                                        |

### Design Choices

- **Single-threaded Tokio runtime.** Everything runs on one thread through `select!`. No locks, no shared mutable state. The proxy owns everything.

- **Full state replay.** On rebuild, dap-watch kills the adapter and spawns a fresh one, then replays the entire captured state sequence (initialize, breakpoints, launch, configurationDone) in the correct order.

- **Attach-to-launch conversion.** In headless mode the editor sends `attach` requests, but after a rebuild there's no running process to attach to. dap-watch converts the captured `attach` into a `launch` so the adapter starts the program _after_ breakpoints are configured. No missed breakpoints on startup code.

- **Fake handshake on reconnect.** When the editor reconnects in headless mode, dap-watch intercepts initialization requests and responds with cached adapter capabilities. The editor never knows the difference.

## Configuration

`dap-watch.toml`:

```toml
[runtime]
adapter = "/path/to/codelldb"
build = "cargo build"

[runtime.mode.Headless]
program = "target/debug/my-app"
program_args = []
port = 2500

[watcher]
paths = ["src/**/*.rs"]
ignore_paths = ["target/**"]
debounce_ms = 200
```

Configuration is layered: CLI flags override the TOML file, which overrides built-in defaults.

## Build & Run

```sh
cargo build --release
```

```sh
# Headless mode
./target/release/dap-watch --port 2500 ./target/debug/my-app

# With verbose logging
./target/release/dap-watch --verbose --port 2500 ./target/debug/my-app
```

## Tests

```sh
cargo test
```

27 unit tests covering DAP message parsing, Content-Length framing, state capture/replay sequencing, and edge cases.

## Requirements

- Rust 1.87+ (edition 2024)
- A DAP-compatible debug adapter (codelldb for Rust/C++, delve for Go, etc.)
