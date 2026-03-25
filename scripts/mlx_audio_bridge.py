#!/usr/bin/env python3
"""
Bridge script for mlx-audio TTS and STT.
Called from Rust backend. Keeps model loaded across calls via a simple HTTP server,
or runs one-shot from CLI.

Commands:
  tts <text> <output_wav>    - Generate speech from text
  stt <input_audio> <output_txt> - Transcribe audio to text
"""

import sys
import os

# Suppress warnings
os.environ["TOKENIZERS_PARALLELISM"] = "false"

TTS_MODEL = "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit"
STT_MODEL = "mlx-community/whisper-large-v3-turbo"

def tts(text, output_path):
    from mlx_audio.tts.generate import generate_audio, load_model
    import numpy as np
    import soundfile as sf

    model_data = load_model(TTS_MODEL)
    # load_model returns variable items depending on version; use CLI-style approach
    from mlx_audio.tts.generate import main as tts_main
    sys.argv = [
        "mlx_audio.tts.generate",
        "--model", TTS_MODEL,
        "--text", text,
        "--output_path", os.path.dirname(output_path) or "/tmp",
        "--file_prefix", os.path.splitext(os.path.basename(output_path))[0],
        "--lang_code", "zh",
    ]
    tts_main()
    # The output file gets _000 appended
    expected = output_path.replace(".wav", "_000.wav")
    if os.path.exists(expected) and expected != output_path:
        os.rename(expected, output_path)
    print(f"OK:{output_path}")

def stt(audio_path, output_path):
    from mlx_audio.stt.generate import main as stt_main
    sys.argv = [
        "mlx_audio.stt.generate",
        "--model", STT_MODEL,
        "--audio", audio_path,
        "--output-path", output_path,
        "--format", "txt",
        "--language", "zh",
    ]
    stt_main()
    # Read the generated text file
    txt_file = os.path.join(output_path, os.path.splitext(os.path.basename(audio_path))[0] + ".txt")
    if os.path.exists(txt_file):
        text = open(txt_file).read().strip()
        print(f"OK:{text}")
    else:
        # Try finding any .txt in output
        for f in os.listdir(output_path):
            if f.endswith(".txt"):
                text = open(os.path.join(output_path, f)).read().strip()
                print(f"OK:{text}")
                return
        print("ERROR:no output")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: mlx_audio_bridge.py <tts|stt> <args...>", file=sys.stderr)
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "tts" and len(sys.argv) == 4:
        tts(sys.argv[2], sys.argv[3])
    elif cmd == "stt" and len(sys.argv) == 4:
        stt(sys.argv[2], sys.argv[3])
    else:
        print(f"Unknown: {cmd}", file=sys.stderr)
        sys.exit(1)
