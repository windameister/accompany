# Accompany (陪伴) 🐱

A desktop AI companion — a cat-girl (猫娘) that lives on your desktop, monitors your Claude Code sessions, and proactively helps with work tasks.

Built with **Tauri v2** (Rust) + **React** + **PixiJS/Live2D**.

![Status: Prototype](https://img.shields.io/badge/status-prototype-orange)

## What It Does

- **Live2D Character** on your desktop — transparent, always-on-top, draggable, with click-through on transparent areas
- **Voice Dialogue** — always-on microphone listening with VAD (Voice Activity Detection), speaks back via TTS
- **Claude Code Monitoring** — HTTP hook server receives events from Claude Code sessions, alerts you with voice when approval is needed
- **GitHub Actions Watch** — polls your repos for CI/CD status, notifies on failures
- **Memory System** — remembers facts about you across sessions (SQLite + keyword retrieval)
- **Smart Intent Detection** — distinguishes when you're talking to her vs. background noise, with wake words ("猫娘", "你好", etc.)

## Architecture

```
┌──────────────────────────────────────────┐
│              Tauri v2 Shell              │
│                                          │
│  Frontend (WebView)     Backend (Rust)   │
│  ├─ PixiJS v7 + Live2D  ├─ Axum Hook    │
│  ├─ React 19 + Zustand  │  Server:17832 │
│  ├─ TTS Audio Queue     ├─ MiniMax Chat  │
│  ├─ VAD + MediaRecorder ├─ TTS (MiniMax  │
│  └─ Click-through       │  + edge-tts)  │
│                          ├─ SQLite Memory│
│                          ├─ GitHub Poll  │
│                          └─ Session Track│
└──────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- **Rust** (latest stable) + **Node.js** 20+
- **Tauri v2 CLI**: `cargo install tauri-cli`
- **Python 3** with: `pip3 install edge-tts SpeechRecognition`
- **ffmpeg**: `brew install ffmpeg`
- A **MiniMax API key** (for chat + TTS) — [platform.minimaxi.com](https://platform.minimaxi.com)

### Setup

```bash
git clone https://github.com/windameister/accompany.git
cd accompany
npm install

# Create .env with your API key
echo "MINIMAX_API_KEY=your-key-here" > .env

# Run in development mode
cargo tauri dev
```

### Claude Code Hooks

To enable Claude session monitoring, click the tray icon → **"Claude Hooks (点击安装)"**. This adds hooks to `~/.claude/settings.json` that POST to `localhost:17832`.

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Desktop Shell | Tauri v2 (Rust) |
| Frontend | React 19 + TypeScript + Vite |
| Character | PixiJS v7 + Live2D (pixi-live2d-display) |
| State | Zustand |
| Chat LLM | MiniMax M2.7 (OpenAI-compatible) |
| TTS | MiniMax Speech 2.8 HD → edge-tts fallback |
| STT | Google Speech Recognition (via Python) |
| Voice Detection | Web Audio API (VAD) |
| Hook Server | Axum (shares Tokio runtime with Tauri) |
| Memory | SQLite (rusqlite + spawn_blocking) |
| GitHub | gh CLI token + REST API polling |
| Styling | Tailwind CSS |

## Project Status

This is an active prototype. Current features work but are being iterated on.

### What Works
- Live2D character rendering (Hiyori model)
- Voice conversation (always-on listening + push-to-talk)
- Claude Code hook alerts with voice notification
- GitHub Actions monitoring (3 repos)
- Memory extraction from conversations
- Tray menu with hook install/uninstall toggle

### Known Limitations
- macOS only (Tauri supports cross-platform but Live2D + voice are macOS-tuned)
- Click-through uses bounding box approximation, not pixel-perfect
- Non-active window requires first click to activate (macOS limitation)
- STT has ~2-3s latency (network round-trip to Google)
- MiniMax TTS has daily quota on free/plus plans

## License

MIT

## Acknowledgments

- [Live2D Cubism SDK](https://www.live2d.com/) — Character rendering
- [pixi-live2d-display](https://github.com/guansss/pixi-live2d-display) — PixiJS Live2D plugin
- [Hiyori model](https://www.live2d.com/en/learn/sample/) — Free sample model by Live2D Inc.
- [edge-tts](https://github.com/rany2/edge-tts) — Free Microsoft neural TTS
- [MiniMax](https://platform.minimaxi.com) — LLM and TTS API
