#!/usr/bin/env python3
"""
Persistent MLX TTS/STT/Voiceprint server. Models loaded once, stay in memory.

POST /tts      {"text":"你好","output":"/tmp/out.wav"}  → {"status":"ok","size":N,"elapsed":T}
POST /stt      {"audio":"/tmp/in.wav"}                   → {"status":"ok","text":"你好","elapsed":T}
POST /enroll   {"audio":"/tmp/in.wav"}                   → {"status":"ok","sample_count":N}
POST /verify   {"audio":"/tmp/in.wav"}                   → {"status":"ok","is_host":true,"similarity":0.85}
GET  /health                                              → {"status":"ok"}
"""

import os, sys, json, time
import numpy as np
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler

os.environ["TOKENIZERS_PARALLELISM"] = "false"

PORT = 17833
TTS_MODEL = "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit"
TTS_VOICE = "Serena"
TTS_INSTRUCT = "年轻清纯的少女声音，音调偏高，语气温柔自然，像一个16岁左右的邻家女孩，真诚亲切"
TTS_SPEED = 2.0
STT_MODEL = "mlx-community/whisper-large-v3-turbo"
VOICEPRINT_PATH = os.path.join(
    os.environ.get("HOME", "/tmp"), "Library/Application Support/accompany/voiceprint.json"
)
VERIFY_THRESHOLD = 0.75

# Global model handles
voice_encoder = None


def warmup_tts():
    print("[MLX] Loading TTS model...", flush=True)
    from mlx_audio.tts.generate import generate_audio
    generate_audio(
        text="嗯", model=TTS_MODEL, voice=TTS_VOICE, instruct=TTS_INSTRUCT,
        speed=TTS_SPEED, lang_code="zh", output_path="/tmp",
        file_prefix=f"_w_{os.getpid()}", verbose=False,
    )
    for f in os.listdir("/tmp"):
        if f.startswith(f"_w_{os.getpid()}"):
            try: os.remove(f"/tmp/{f}")
            except: pass
    print("[MLX] TTS ready", flush=True)


def warmup_stt():
    print("[MLX] Loading STT model...", flush=True)
    import mlx_whisper, soundfile as sf
    tmp = f"/tmp/_stt_w_{os.getpid()}.wav"
    sf.write(tmp, np.zeros(16000, dtype=np.float32), 16000)
    try: mlx_whisper.transcribe(tmp, language="zh", path_or_hf_repo=STT_MODEL)
    except: pass
    try: os.remove(tmp)
    except: pass
    print("[MLX] STT ready", flush=True)


def warmup_voiceprint():
    global voice_encoder
    print("[MLX] Loading voice encoder...", flush=True)
    from resemblyzer import VoiceEncoder
    voice_encoder = VoiceEncoder()
    print("[MLX] Voice encoder ready", flush=True)


def do_tts(text, output_path):
    from mlx_audio.tts.generate import generate_audio
    out_dir = os.path.dirname(output_path) or "/tmp"
    prefix = os.path.splitext(os.path.basename(output_path))[0]
    t0 = time.time()
    generate_audio(
        text=text, model=TTS_MODEL, voice=TTS_VOICE, instruct=TTS_INSTRUCT,
        speed=TTS_SPEED, lang_code="zh", output_path=out_dir,
        file_prefix=prefix, verbose=False,
    )
    elapsed = time.time() - t0
    generated = os.path.join(out_dir, f"{prefix}_000.wav")
    if os.path.exists(generated) and generated != output_path:
        os.rename(generated, output_path)
    if os.path.exists(output_path):
        return {"status": "ok", "size": os.path.getsize(output_path), "elapsed": round(elapsed, 2)}
    return {"status": "error", "message": "no output"}


def do_stt(audio_path):
    import mlx_whisper
    t0 = time.time()
    result = mlx_whisper.transcribe(audio_path, language="zh", path_or_hf_repo=STT_MODEL)
    elapsed = time.time() - t0
    return {"status": "ok", "text": result.get("text", "").strip(), "elapsed": round(elapsed, 2)}


def do_enroll(audio_path):
    from resemblyzer import preprocess_wav
    wav = preprocess_wav(Path(audio_path))
    embedding = voice_encoder.embed_utterance(wav)

    store = Path(VOICEPRINT_PATH)
    if store.exists():
        data = json.loads(store.read_text())
        existing = np.array(data["embeddings"])
        embeddings = np.vstack([existing, embedding.reshape(1, -1)])
    else:
        embeddings = embedding.reshape(1, -1)

    mean_embed = embeddings.mean(axis=0)
    data = {
        "embeddings": embeddings.tolist(),
        "mean": mean_embed.tolist(),
        "sample_count": len(embeddings),
    }
    store.parent.mkdir(parents=True, exist_ok=True)
    store.write_text(json.dumps(data))
    return {"status": "ok", "sample_count": len(embeddings)}


def do_verify(audio_path):
    from resemblyzer import preprocess_wav
    store = Path(VOICEPRINT_PATH)
    if not store.exists():
        return {"status": "ok", "is_host": False, "similarity": 0.0, "enrolled": False}

    data = json.loads(store.read_text())
    host_mean = np.array(data["mean"])

    wav = preprocess_wav(Path(audio_path))
    embedding = voice_encoder.embed_utterance(wav)

    similarity = float(np.dot(host_mean, embedding) / (
        np.linalg.norm(host_mean) * np.linalg.norm(embedding)
    ))
    is_host = similarity >= VERIFY_THRESHOLD

    return {"status": "ok", "is_host": is_host, "similarity": round(similarity, 4), "enrolled": True}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self._json({"status": "ok"})
        else:
            self._json({"error": "not found"}, 404)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or "{}")

        if self.path == "/tts":
            text = body.get("text", "")
            output = body.get("output", f"/tmp/mlx_tts_{os.getpid()}.wav")
            if not text: self._json({"status": "error", "message": "no text"}, 400); return
            self._json(do_tts(text, output))

        elif self.path == "/stt":
            audio = body.get("audio", "")
            if not audio or not os.path.exists(audio):
                self._json({"status": "error", "message": "no audio"}, 400); return
            self._json(do_stt(audio))

        elif self.path == "/enroll":
            audio = body.get("audio", "")
            if not audio or not os.path.exists(audio):
                self._json({"status": "error", "message": "no audio"}, 400); return
            self._json(do_enroll(audio))

        elif self.path == "/verify":
            audio = body.get("audio", "")
            if not audio or not os.path.exists(audio):
                self._json({"status": "error", "message": "no audio"}, 400); return
            self._json(do_verify(audio))

        else:
            self._json({"error": "not found"}, 404)

    def _json(self, data, code=200):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, fmt, *args): pass


if __name__ == "__main__":
    print(f"[MLX] Starting on :{PORT}...", flush=True)
    warmup_tts()
    warmup_stt()
    warmup_voiceprint()
    server = HTTPServer(("127.0.0.1", PORT), Handler)
    print(f"[MLX] Ready on http://127.0.0.1:{PORT}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()
