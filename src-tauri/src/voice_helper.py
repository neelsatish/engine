"""Engine's voice helper: one JSON request per line on stdin, one JSON reply per line on stdout.

Whisper models stay loaded between requests; Engine ends this process after a few idle minutes to free memory.
"""

import importlib.util
import json
import os
import sys
import wave

whisper_models = {}
tts = None


def gpu_libraries():
    """Lets CTranslate2 (Whisper) find CUDA's cuBLAS and cuDNN: they ship inside the GPU build of PyTorch."""
    spec = importlib.util.find_spec("torch")
    if spec and spec.origin:
        lib = os.path.join(os.path.dirname(spec.origin), "lib")
        if os.path.isdir(lib):
            os.add_dll_directory(lib)
            os.environ["PATH"] = lib + os.pathsep + os.environ.get("PATH", "")


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
        try:  # the first pass on a GPU is slow while CUDA warms up: spend it now, on a second of silence
            import numpy as np

            list(whisper_models[name].transcribe(np.zeros(16000, dtype=np.float32), beam_size=1)[0])
        except Exception:
            pass
    return whisper_models[name]


def segments(path, name):
    """Timed pieces of speech, so Engine can act on each finished phrase while the person is still talking."""
    # Split at short pauses (the default waits two seconds), so each phrase can run as soon as it's said.
    vad = {"min_silence_duration_ms": 350, "speech_pad_ms": 150}
    found, _ = whisper(name).transcribe(path, beam_size=1, vad_filter=True, vad_parameters=vad)
    return [{"start": s.start, "end": s.end, "text": s.text.strip()} for s in found if s.text.strip()]


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


def save_wav(path, samples, rate):
    """Writes mono float samples as 16-bit PCM with the standard library (newer torchaudio needs extra codecs)."""
    import numpy as np

    pcm = (np.clip(samples, -1.0, 1.0) * 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


british = None
KOKORO = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files/"
KOKORO_FILES = ("kokoro-v0_19.onnx", "voices.json")


def kokoro():
    """Kokoro, a small fast voice model with British voices. Its files (~350 MB) download next to this script once."""
    global british
    if british is None:
        import urllib.request
        from kokoro_onnx import Kokoro

        folder = os.path.join(os.path.dirname(os.path.abspath(__file__)), "kokoro")
        os.makedirs(folder, exist_ok=True)
        for name in KOKORO_FILES:
            path = os.path.join(folder, name)
            if not os.path.exists(path):
                urllib.request.urlretrieve(KOKORO + name, path + ".part")
                os.replace(path + ".part", path)
        british = Kokoro(*(os.path.join(folder, n) for n in KOKORO_FILES))
        british.create("Ready.", voice="bm_george", lang="en-gb")  # the first run is slow: spend it now
    return british


def say(text, out):
    """Writes `text` in a British voice (Kokoro's George) to `out`."""
    samples, rate = kokoro().create(text, voice="bm_george", speed=1.05, lang="en-gb")
    save_wav(out, samples, rate)


def speak(text, clip, out, engine="chatterbox"):
    global tts
    if engine == "kokoro":
        say(text, out)
    else:
        import torch
        from chatterbox.tts import ChatterboxTTS

        if tts is None:
            tts = ChatterboxTTS.from_pretrained(device="cuda" if torch.cuda.is_available() else "cpu")
        wav = tts.generate(text, audio_prompt_path=clip or None)
        save_wav(out, wav.squeeze().detach().cpu().numpy(), tts.sr)
    import winsound

    winsound.PlaySound(out, winsound.SND_FILENAME)


def handle(req):
    cmd = req.get("cmd")
    if cmd == "transcribe":
        return {"text": transcribe(req["path"], req.get("model") or "large-v3-turbo")}
    if cmd == "segments":
        return {"segments": segments(req["path"], req.get("model") or "large-v3-turbo")}
    if cmd == "speak":
        speak(req["text"], req.get("clip", ""), req["out"], req.get("engine", "chatterbox"))
        return {"ok": True}
    if cmd == "say":  # write speech to a file without playing it (Engine caches short replies like "Yes sir?")
        say(req["text"], req["out"])
        return {"ok": True}
    if cmd == "warm":
        whisper(req.get("model") or "large-v3-turbo")
        return {"ok": True}
    if cmd == "check":
        found = lambda m: importlib.util.find_spec(m) is not None
        return {"whisper": found("faster_whisper"), "chatterbox": found("chatterbox"), "kokoro": found("kokoro_onnx")}
    return {"error": f"Unknown voice command: {cmd}"}


gpu_libraries()
for line in sys.stdin:
    try:
        reply = handle(json.loads(line))
    except Exception as e:  # report and keep serving
        reply = {"error": f"{type(e).__name__}: {e}"}
    print(json.dumps(reply), flush=True)
