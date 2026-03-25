#!/usr/bin/env python3
"""
Voice fingerprint tool for Accompany.
Uses resemblyzer to generate speaker embeddings.

Commands:
  enroll <wav_path> <store_path>   - Add a voice sample to the host voiceprint
  verify <wav_path> <store_path>   - Check if audio matches the host voice
  reset <store_path>               - Clear the stored voiceprint
"""

import sys
import json
import numpy as np
from pathlib import Path

def get_encoder():
    from resemblyzer import VoiceEncoder
    return VoiceEncoder()

def load_and_preprocess(wav_path):
    from resemblyzer import preprocess_wav
    return preprocess_wav(Path(wav_path))

def enroll(wav_path, store_path):
    """Add a voice sample to build/refine the host voiceprint."""
    encoder = get_encoder()
    wav = load_and_preprocess(wav_path)
    embedding = encoder.embed_utterance(wav)

    store = Path(store_path)
    if store.exists():
        data = json.loads(store.read_text())
        existing = np.array(data["embeddings"])
        embeddings = np.vstack([existing, embedding.reshape(1, -1)])
    else:
        embeddings = embedding.reshape(1, -1)

    # Compute mean voiceprint
    mean_embed = embeddings.mean(axis=0)

    data = {
        "embeddings": embeddings.tolist(),
        "mean": mean_embed.tolist(),
        "sample_count": len(embeddings),
    }

    store.parent.mkdir(parents=True, exist_ok=True)
    store.write_text(json.dumps(data))

    print(json.dumps({
        "status": "ok",
        "sample_count": len(embeddings),
    }))

def verify(wav_path, store_path):
    """Check if audio matches the stored host voiceprint."""
    store = Path(store_path)
    if not store.exists():
        print(json.dumps({"status": "no_voiceprint", "is_host": False, "similarity": 0.0}))
        return

    data = json.loads(store.read_text())
    host_mean = np.array(data["mean"])

    encoder = get_encoder()
    wav = load_and_preprocess(wav_path)
    embedding = encoder.embed_utterance(wav)

    # Cosine similarity
    similarity = float(np.dot(host_mean, embedding) / (
        np.linalg.norm(host_mean) * np.linalg.norm(embedding)
    ))

    # Threshold: 0.75+ is likely the same speaker
    is_host = similarity >= 0.75

    print(json.dumps({
        "status": "ok",
        "is_host": is_host,
        "similarity": round(similarity, 4),
    }))

def reset(store_path):
    """Clear stored voiceprint."""
    store = Path(store_path)
    if store.exists():
        store.unlink()
    print(json.dumps({"status": "ok"}))

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: voiceprint.py <enroll|verify|reset> <args...>", file=sys.stderr)
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "enroll" and len(sys.argv) == 4:
        enroll(sys.argv[2], sys.argv[3])
    elif cmd == "verify" and len(sys.argv) == 4:
        verify(sys.argv[2], sys.argv[3])
    elif cmd == "reset" and len(sys.argv) == 3:
        reset(sys.argv[2])
    else:
        print(f"Unknown command: {cmd}", file=sys.stderr)
        sys.exit(1)
