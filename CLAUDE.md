# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Accompany (陪伴) is a desktop AI companion app — a cat-girl (猫娘) character that lives on your desktop, monitors your Claude Code sessions, and proactively helps with work tasks. Built with Tauri v2 (Rust backend) + React/PixiJS (frontend).

## Commands

```bash
# Development (starts both Vite dev server and Tauri window)
cargo tauri dev

# Build for production
cargo tauri build

# Frontend only (no Tauri shell)
npm run dev

# Check Rust compilation
cd src-tauri && cargo check

# Run Rust tests
cd src-tauri && cargo test
```

## Architecture

**Tauri v2 dual-process model:**
- **Rust backend** (`src-tauri/src/`): system-level agent logic — Claude session monitoring via HTTP hook server (Axum on localhost:17832), notification aggregation (GitHub/Slack/Calendar), memory system (SQLite + local embeddings via fastembed-rs), character state machine (FSM driving expressions/animations)
- **Web frontend** (`src/`): React 19 + PixiJS v8 for character rendering (sprite sheets for prototype, Live2D via `@naari3/pixi-live2d-display` later), chat UI, notification toasts. State management with Zustand.

**Window design:** Two Tauri windows — a transparent always-on-top character window (click-through on non-character areas) and a separate chat panel window that toggles with `Cmd+Shift+A`.

**Claude Code integration:** The app runs an Axum HTTP server that receives Claude Code hook events (`Notification`, `SessionStart`, `Stop`, `PreToolUse`). Users configure hooks in `~/.claude/settings.json` to POST to `localhost:17832`. Fallback: JSONL file watching on `~/.claude/projects/`.

**Key Rust modules:**
- `commands/` — Tauri IPC command handlers (frontend ↔ backend bridge)
- `agent/` — Anthropic API client, system prompt, conversation context
- `claude_monitor/` — Hook HTTP server + JSONL watcher + process detection
- `memory/` — SQLite via sqlx, fastembed-rs embeddings, retrieval scoring
- `notifications/` — GitHub (octocrab), Slack, Calendar aggregation
- `character/` — State machine (IDLE/CHATTING/ALERTING/THINKING/SLEEPING)

## Key Technical Decisions

- Tauri v2 over game engines: core value is agent intelligence, not rendering. Single-process, no IPC complexity.
- `macOSPrivateApi: true` in tauri.conf.json is required for transparent windows on macOS.
- Local embeddings (fastembed-rs) over API embeddings: no network latency for memory retrieval.
- SQLite over vector DB: <100K memories, brute-force cosine similarity in Rust is sub-millisecond.
- Axum hook server shares Tokio runtime with Tauri — no extra process needed.
