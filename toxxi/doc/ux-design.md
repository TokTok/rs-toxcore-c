# Toxxi UX Design Specification

## 1. Introduction
Toxxi is a TUI chat client for the Tox protocol, written in Rust. This document outlines the UX and UI design principles, layout structures, and interaction models.

Toxxi aims for a "power-user" experience that is fast, discoverable, and aesthetically modern. It prioritizes keyboard-driven navigation, low latency, and a high-information-density display, remaining functional over SSH across Linux and macOS.

---

## 2. Global Layout Structure
Toxxi uses a multi-pane layout managed by a flexible grid system adapting to varying terminal sizes.

### 2.1 Pane Overview
1.  **Sidebar (Left):** Context navigation (Friends, Groups, Conferences). Fixed width by default (e.g., 25 chars), toggleable or resizable.
2.  **Main Content (Center):** Active conversation, game, or file manager.
3.  **Info Pane (Right - Optional):** Metadata about current context (Profile info, member list, shared files).
4.  **Input Area (Bottom-Center):** Framed, dynamic text entry box that grows vertically.
5.  **Status Bar (Bottom-Global):** System-wide status, notifications, and connectivity health.

### 2.2 Responsive Layout Logic
The UI adaptively changes based on the terminal width (`W`) and height (`H`):

#### 2.2.1 Wide Mode (W > 120 chars)
*   **Sidebar:** Visible with full names and status icons.
*   **Main Chat:** Uses an "irssi-style" layout.
    ```text
    [14:30] <  Alice> | Hey, are we still on for the game?
    [14:31] <     Me> | Yeah, just finishing some work.
    [14:32] <  Alice> | Cool. I'll send the invite in a bit.
                      | It's going to be a long night!
    ```
*   **Info Pane:** Visible on the right, showing participants or file history.

#### 2.2.2 Medium Mode (80 < W <= 120 chars)
*   **Sidebar:** Visible but potentially collapsed to icons/initials if space is needed.
*   **Main Chat:** Standard linear layout.
*   **Info Pane:** Hidden by default; accessible via the `i` toggle.

#### 2.2.3 Narrow Mode (W <= 80 chars)
*   **Sidebar:** Hidden; accessible via a modal or a slide-out overlay.
*   **Main Chat:** Stacked layout to maximize horizontal space.
    ```text
    14:30 Alice:
    Hey, are we still on for the game?
    ---------------------------------
    14:31 Me:
    Yeah, just finishing some work.
    ```

---

## 3. Navigation: The Sidebar and Quick Switcher

### 3.1 The Sidebar Structure
Sidebar sections can be collapsed using arrow keys or shortcuts. Conversations are identified by `ConversationId` (see [doc/merkle-tox-dag.md](../../doc/merkle-tox-dag.md)).

*   **[F] Friends (1:1)**
    *   `● Alice` (Online, Green)
    *   `◑ Bob` (Away, Yellow)
    *   `● Charlie` (Busy, Red)
    *   `○ Dave` (Offline, Gray)
*   **[G] DHT Groups (New)**
    *   `# Rust-Devs` (42 Online)
    *   `# Tox-General` (128 Online)
*   **[C] Conferences (Legacy)**
    *   `& Old-School-Tox`

### 3.2 The Quick Switcher (Ctrl+P)
Quick Switcher is a fuzzy-find modal for instantaneous context switching.

#### 3.2.1 Search Logic
Uses a weighted fuzzy-matching algorithm (like `fzf`'s Smith-Waterman variant):
*   **Exact Matches:** Boosted to the top.
*   **Prefix Matches:** High priority.
*   **Acronym Matches:** (e.g., `rd` matches `Rust-Devs`) Medium priority.

#### 3.2.2 Action Prefixes
Switcher triggers actions:
*   `f: <name>`: Filter for friends only.
*   `g: <name>`: Filter for groups only.
*   `h: <text>`: Search message history globally. Selecting a result jumps to that conversation and scrolls to the message.
*   `>: <command>`: Quick access to settings or UI actions (e.g., `> Theme: Dark`).

---

## 4. The Message Gutter and Display

### 4.1 The Gutter (Status Column)
A 2-character wide "Gutter" left of every message provides message state feedback.

*   `● ` : Delivered.
*   `○ ` : Sending (not yet acknowledged by the peer).
*   `! ` : Error (peer unreachable or message rejected).
*   `✓ ` : Read (if supported by the client and protocol extensions).
*   `⚙ ` : System information (e.g., "Alice changed her name").

**Visibility:** Timestamps and status indicators in gutter are globally toggleable via shortcut or `/set` to provide a cleaner, "chat-focused" view.

### 4.2 Message Grouping
Consecutive messages from same sender within short timeframe (e.g., 2 minutes) are grouped.
*   The first message shows the timestamp and name.
*   Subsequent messages only show the "Gutter" status and the message body.

---

## 5. Input Mechanics: The Framed Box

Input box is a sophisticated line editor wrapped in a Unicode frame.

### 5.1 Dynamic Framing
Box lives at bottom of main chat pane.
```text
╭─────────────────────────────────────────────────────────────────────────────╮
│ > This is a long message that will eventually wrap to the next line. As I   │
│   continue to type, the box will grow upwards until it reaches its max size.│
╰─────────────────────────────────────────────────────────────────────────────╯
```
*   **Max Height:** Input box grows vertically. Maximum height is configurable (default 10 text rows, 12 total including borders).
*   **Border Styling:** Border color changes when focused, dims when unfocused.
*   **Visual Wrap Marker:** UI-wrapped lines indicated by Unicode arrow (`↳`) or dimmed vertical line on right border.

### 5.2 Editing Features
*   **Multi-line Support:** 
    *   **Mode A (Default):** `Enter` sends message, `Shift+Enter` inserts literal newline.
    *   **Mode B:** `Enter` inserts literal newline, `Shift+Enter` sends message.
    *   **Toggling:** Configurable shortcut swaps modes.
*   **Selection:** `Shift+Left/Right` selects characters. `Ctrl+Shift+Left/Right` selects words.
*   **Clipboard:** `Ctrl+C/V/X` integrates with system clipboard.
*   **Readline Shortcuts:**
    *   `Ctrl+A`: Home
    *   `Ctrl+E`: End
    *   `Ctrl+K`: Kill to end of line
    *   `Ctrl+U`: Kill to start of line
    *   `Ctrl+W`: Kill previous word

---

## 6. The Slash Command System

Commands interact with Tox protocol and UI.

### 6.1 Discoverability
Typing `/` at start of input triggers Command Discovery Overlay.

#### 6.1.1 The Suggestion List
Command list appears above input box, sorted by relevance or frequency.
```text
  /about             Show version info
  /attach            Attach a file to the current chat
  /block             Block the current user
  /call              Start an audio call
  /clear             Clear the current window
  --------------------------------------------------
  (1/24) Press [Tab] to complete
```

#### 6.1.2 Fuzzy Argument Completion
Fuzzy matching provided for local filesystem paths.
```text
  /file send ~/Docu_
  ------------------
  ~/Documents/
  ~/Downloads/
```
The UI will proactively suggest completions for:
*   **Friends' names:** (e.g., `/msg Al<tab>`)
*   **Paths:** (e.g., `/file send <tab>`)
*   **Settings keys:** (e.g., `/set ui.th<tab>`)

---

## 7. File Transfers

File transfers are a core part of the Tox experience and require a high-friction-reduction UX.

### 7.1 Inline Presence
File transfer offers appear as inline cards.
```text
╭─ 📄 incoming: presentation.zip ─────────────────────────────────────────────╮
│ Size: 45.2 MB                                                               │
│ [ (a) Accept ] [ (x) Decline ] [ (o) Change Destination ]                   │
╰─────────────────────────────────────────────────────────────────────────────╯
```

### 7.2 Progress Visualization
Accepted cards update with progress bar and throughput stats.
```text
╭─ 📄 downloading: presentation.zip ──────────────────────────────────────────╮
│ [████████████████████░░░░░░░] 72%                                           │
│ Rate: 1.2 MB/s | ETA: 12s                                                   │
│ [ (p) Pause ] [ (x) Cancel ]                                                │
╰─────────────────────────────────────────────────────────────────────────────╯
```

### 7.3 The File Manager Modal
Global view (`/file list` or `Ctrl+F`) shows all active, completed, and failed transfers across all profiles and chats, allowing for bulk actions (e.g., "Cancel all active").

---

## 8. Network Games and Extensibility

Toxxi supports P2P interaction beyond text.

### 8.1 Custom Packet Handling
`core` routes custom DHT packets. UI provides hooks for specialized views.

### 8.2 Game Session Lifecycle
1.  **Invite:** Game-specific invite box appears.
2.  **Acceptance:** UI enters Game Mode.
3.  **Split-Screen:** Main area splits: game UI top 70%, chat bottom 30%.
4.  **Input Capturing:** Terminal enters Raw Input mode for game pane. Escape sequence (e.g., `Ctrl+G`) switches focus to chat.

### 8.3 The "Oscilloscope" Widget
Audio calls use real-time visualization.
*   Braille Unicode characters create high-resolution waveform in 1-2 line box.

---

## 9. Advanced UX Features

### 9.1 Multi-Profile Support
Supports multiple Tox IDs simultaneously.
*   **Tabs:** Tab bar appears for multiple active profiles.
*   **Shortcuts:** `Alt + [1..9]` switches profile.
*   **Combined View:** Optional "All Messages" view aggregates mentions and notifications from all profiles into a single stream.

### 9.2 Notifications and Beeps
*   **In-TUI Visuals:** Sidebar item flashes or changes color.
*   **Terminal Bell:** Standard `\a` (ASCII Bell) for mentions, configurable.
*   **External Integration:** Optional `libnotify` or `terminal-notifier` integration.

### 9.3 Connectivity Health (The Heartbeat)
Status bar provides Tox network health check using median-based consensus clock (see [doc/merkle-tox-clock.md](../../doc/merkle-tox-clock.md)).
*   `Nodes: [ ⠿⠿⠿⠿⠶ ]`: Braille sparkline of connected DHT nodes over last 10 minutes.
*   `Mode: [ UDP/DHT ]`: Shows direct (UDP) or relayed (TCP) connection.

---

## 10. Design Trade-offs and Rationale

### 10.1 Why TUI?
*   **Performance:** Low memory footprint.
*   **Accessibility:** SSH/Mosh compatible.
*   **Focus:** Distraction-free.

### 10.2 Why Unicode Frames?
*   Modern aesthetics.
*   Visually separates input from history.

### 10.3 The "Alt" Key Problem
Terminals often struggle with `Alt` key.
*   **Rationale:** `Ctrl` and `Esc` preferred for primary navigation. `Alt` reserved for secondary shortcuts.

---

## 11. Interaction State Machine (Internal)

UI follows strict state machine:

1.  **Normal Mode:** Input box is focused. Standard typing.
2.  **Command Mode:** Triggered by `/`. Tab completion is active.
3.  **Navigation Mode:** Triggered by `Esc`. Input box loses focus. Arrows move focus between Sidebar, Chat history (for selection), and Info Pane.
4.  **Modal Mode:** (Quick Switcher, File Manager). All input is captured by the modal until `Enter` or `Esc` is pressed.
5.  **Game Mode:** Special input capturing for custom game logic.

---

## 12. Accessibility and Customization

### 12.1 Themes
Colors defined in `theme.toml`.
*   Supports 16-color, 256-color, and TrueColor.
*   "No-Unicode" mode for legacy terminals.

### 12.2 Keyboard Mapping
Actions bindable via `keys.toml`.

---

## 13. UX Scenarios / User Journeys

### 13.1 Scenario: Sending a File to Alice
1.  User starts typing `/fi` in the chat with Alice.
2.  The command overlay suggests `/file`. User hits `Tab`.
3.  The overlay suggests `send`. User hits `Tab`.
4.  The overlay suggests `~/`. User types `Do` and hits `Tab` to complete `~/Documents/`.
5.  User selects `photo.jpg` from the fuzzy list and hits `Enter`.
6.  An inline transfer card appears in the chat stream.
7.  Alice accepts; the user sees the progress bar fill in real-time.

### 13.2 Scenario: Joining a Group Game
1.  A notification appears in the `#General` DHT group: "Bob has started a game of 2048".
2.  User hits `Esc` to leave input mode.
3.  User uses `Up Arrow` to highlight the game invite card.
4.  User hits `j` to join.
5.  The screen splits; the game appears in the top half.
6.  User plays using arrow keys. When someone mentions them in chat, the "New Message" indicator in the status bar flashes.
7.  User hits `Ctrl+G` to jump focus back to the chat to reply, then `Ctrl+G` again to resume the game.

### 13.3 Scenario: Finding an Old Message
1.  User hits `Ctrl+P`.
2.  User types `h: sushi`.
3.  A list of messages containing "sushi" appears from all chats.
4.  User selects one from "Alice" from 3 days ago.
5.  The UI switches context to the Alice chat and scrolls back to the exact timestamp, highlighting the message in a temporary background color.

---

## 14. UI Component Architecture (Internal Perspective)

From an implementation standpoint, the UI is built from atomic components that manage their own state and rendering logic.

### 14.1 The `MessageList` Component
*   **Virtual Scrolling:** Calculates layout only for on-screen messages.
*   **Reflow Logic:** Recalculates layout on terminal resize.
*   **Search Overlay:** Highlights text during searches.

### 14.2 The `DynamicInput` Component
*   **Syntax Highlighting:** Real-time highlighting.
*   **Buffer Management:** Maintains undo/redo stack.

### 14.3 The `StatusLine` Component
*   **Polling:** Background timer polls DHT health.
*   **Transient Alerts:** Displays temporary fading messages.

---

## 15. Conclusion
Toxxi UX is designed to be "invisible": staying out of the way of the conversation while providing powerful tools at a moment's notice. By combining traditional IRC-like efficiency with modern IDE-inspired navigation, it sets a new standard for Tox clients.

(End of Document)
