# Toxxi

A Terminal Tox Client (TUI).

## Features

-   **TUI Mode**: A full-featured terminal interface based on `ratatui` and
    `crossterm`.
-   **Dynamic Help**: Built-in command registry providing automatic help.
-   **Event Tracking**: Real-time monitoring of network events, read receipts,
    and status changes.

## Running

Simply run the binary to start the interactive client:

```bash
bazel run //rs-toxcore-c/toxxi
```

## Development

Toxxi uses a Registry-Based Architecture. To add a new command, add a new
`CommandDef` to the `COMMANDS` array of the matching module in `src/commands/`.
It will automatically appear in `/help`.

## Testing

### UI Snapshot Tests

Toxxi uses "golden" snapshot tests to verify the UI and widget rendering. These
tests compare the rendered terminal output against stored reference files in
`tests/snapshots/`.

To run all UI and widget tests:

```bash
bazel test //rs-toxcore-c/toxxi:...
```

To update widget snapshots (e.g., after changing a single widget's rendering):

```bash
INSTA_UPDATE=always bazel run //rs-toxcore-c/toxxi:widgets-sidebar-test -- --nocapture
```

To update full UI snapshots (e.g., after changing the global layout or status
bar):

```bash
INSTA_UPDATE=always bazel run //rs-toxcore-c/toxxi:ui-golden-test -- --nocapture
```

To update all snapshots in the project:

```bash
# This will run all UI and widget tests and update snapshots in the source tree
for t in $(bazel query 'kind(rust_test, //rs-toxcore-c/toxxi:all)' | grep -E "widgets|ui-golden"); do
    INSTA_UPDATE=always bazel run $t -- --nocapture
done
```

The snapshots are stored in `tests/snapshots/`. We use a `FakeTimeProvider`
initialized to a fixed UTC time (2023-01-01 12:00:00 UTC) to ensure
deterministic output across different environments and timezones.
