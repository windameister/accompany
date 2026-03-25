#!/usr/bin/env python3
"""
Local TTS/STT bridge using MLX on Apple Silicon.

TTS: Qwen3-TTS via mlx-audio
STT: Whisper via mlx-whisper

Usage:
  python3 mlx_audio_bridge.py tts "你好喵" /tmp/output.wav
  python3 mlx_audio_bridge.py stt /tmp/input.wav
"""

import sys
import os

os.environ["TOKENIZERS_PARALLELISM"] = "false"

TTS_MODEL = "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit"
STT_MODEL = "mlx-community/whisper-large-v3-turbo"


def tts(text, output_path):
    """Generate speech from text using Qwen3-TTS."""
    from mlx_audio.tts.generate import main as tts_main

    out_dir = os.path.dirname(output_path) or "/tmp"
    prefix = os.path.splitext(os.path.basename(output_path))[0]

    sys.argv = [
        "mlx_audio.tts.generate",
        "--model", TTS_MODEL,
        "--text", text,
        "--output_path", out_dir,
        "--file_prefix", prefix,
        "--lang_code", "zh",
    ]
    tts_main()

    # mlx-audio appends _000 to the filename
    generated = os.path.join(out_dir, f"{prefix}_000.wav")
    if os.path.exists(generated) and generated != output_path:
        os.rename(generated, output_path)

    if os.path.exists(output_path):
        size = os.path.getsize(output_path)
        print(f"OK:{size}")
    else:
        print("ERROR:no output file")
        sys.exit(1)


def stt(audio_path):
    """Transcribe audio to text using Whisper."""
    import mlx_whisper

    result = mlx_whisper.transcribe(
        audio_path,
        language="zh",
        path_or_hf_repo=STT_MODEL,
    )
    text = result.get("text", "").strip()
    if text:
        print(f"OK:{text}")
    else:
        print("ERROR:no speech detected")


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
        print(f"Unknown: {cmd} (args: {sys.argv[2:]})", file=sys.stderr)
        sys.exit(1)
