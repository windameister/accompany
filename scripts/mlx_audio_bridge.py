#!/usr/bin/env python3
"""
Local TTS/STT bridge using MLX on Apple Silicon.
Called as subprocess from Rust backend.

Usage:
  python3 mlx_audio_bridge.py tts "你好喵" /tmp/output.wav
  python3 mlx_audio_bridge.py stt /tmp/input.wav
"""

import sys
import os
import subprocess

os.environ["TOKENIZERS_PARALLELISM"] = "false"

TTS_MODEL = "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit"
TTS_VOICE = "Serena"
TTS_INSTRUCT = "年轻清纯的少女声音，音调偏高，语气温柔自然，像一个16岁左右的邻家女孩，真诚亲切"
STT_MODEL = "mlx-community/whisper-large-v3-turbo"


def tts(text, output_path):
    """Generate speech using mlx-audio CLI (most reliable path)."""
    out_dir = os.path.dirname(output_path) or "/tmp"
    prefix = os.path.splitext(os.path.basename(output_path))[0]

    result = subprocess.run(
        [
            sys.executable, "-m", "mlx_audio.tts.generate",
            "--model", TTS_MODEL,
            "--text", text,
            "--voice", TTS_VOICE,
            "--instruct", TTS_INSTRUCT,
            "--output_path", out_dir,
            "--file_prefix", prefix,
            "--speed", "2.0",
            "--lang_code", "zh",
        ],
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        print(f"ERROR:{result.stderr[:200]}", flush=True)
        sys.exit(1)

    # mlx-audio appends _000
    generated = os.path.join(out_dir, f"{prefix}_000.wav")
    if os.path.exists(generated) and generated != output_path:
        os.rename(generated, output_path)

    if os.path.exists(output_path):
        print(f"OK:{os.path.getsize(output_path)}", flush=True)
    else:
        print("ERROR:no output file", flush=True)
        sys.exit(1)


def stt(audio_path):
    """Transcribe audio using mlx-whisper."""
    import mlx_whisper

    result = mlx_whisper.transcribe(
        audio_path,
        language="zh",
        path_or_hf_repo=STT_MODEL,
    )
    text = result.get("text", "").strip()
    if text:
        print(f"OK:{text}", flush=True)
    else:
        print("ERROR:no speech", flush=True)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: mlx_audio_bridge.py <tts|stt> <args...>", file=sys.stderr)
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "tts" and len(sys.argv) == 4:
        tts(sys.argv[2], sys.argv[3])
    elif cmd == "stt" and len(sys.argv) == 3:
        stt(sys.argv[2])
    else:
        print(f"Unknown: {cmd}", file=sys.stderr)
        sys.exit(1)
