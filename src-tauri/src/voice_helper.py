"""Engine's voice helper: one JSON request per line on stdin, one JSON reply per line on stdout.

Whisper models stay loaded between requests; Engine ends this process after a few idle minutes to free memory.
"""

import importlib.util
import json
import sys

whisper_models = {}
tts = None


def whisper(name):
    if name not in whisper_models:
        from faster_whisper import WhisperModel

        def load(device, compute):
            try:  # use a cached copy without touching the network when there is one
                return WhisperModel(name, device=device, compute_type=compute, local_files_only=True)
            except Exception:
                return WhisperModel(name, device=device, compute_type=compute)

        try:
            whisper_models[name] = load("cuda", "float16")
        except Exception:
            whisper_models[name] = load("cpu", "int8")
    return whisper_models[name]


def transcribe(path, name):
    def run(model):
        segments, _ = model.transcribe(path, beam_size=1, vad_filter=True)
        return " ".join(s.text.strip() for s in segments).strip()

    try:
        return run(whisper(name))
    except Exception as e:
        # The GPU can load a model yet still lack a runtime library: fall back to the CPU once.
        if any(k in str(e).lower() for k in ("cuda", "cublas", "cudnn")):
            from faster_whisper import WhisperModel

            whisper_models[name] = WhisperModel(name, device="cpu", compute_type="int8")
            return run(whisper_models[name])
        raise


def speak(text, clip, out):
    global tts
    import torch
    import torchaudio
    from chatterbox.tts import ChatterboxTTS

    if tts is None:
        tts = ChatterboxTTS.from_pretrained(device="cuda" if torch.cuda.is_available() else "cpu")
    wav = tts.generate(text, audio_prompt_path=clip or None)
    torchaudio.save(out, wav, tts.sr)
    import winsound

    winsound.PlaySound(out, winsound.SND_FILENAME)


def handle(req):
    cmd = req.get("cmd")
    if cmd == "transcribe":
        return {"text": transcribe(req["path"], req.get("model") or "small")}
    if cmd == "speak":
        speak(req["text"], req.get("clip", ""), req["out"])
        return {"ok": True}
    if cmd == "check":
        found = lambda m: importlib.util.find_spec(m) is not None
        return {"whisper": found("faster_whisper"), "chatterbox": found("chatterbox")}
    return {"error": f"Unknown voice command: {cmd}"}


for line in sys.stdin:
    try:
        reply = handle(json.loads(line))
    except Exception as e:  # report and keep serving
        reply = {"error": f"{type(e).__name__}: {e}"}
    print(json.dumps(reply), flush=True)
