//! Local voice. Speech to text is Whisper (faster-whisper) in a Python helper that stays loaded while voice is in
//! use and quits after a few idle minutes. Recording is done here with cpal. Speech out is Windows' built-in voice,
//! or Chatterbox (a cloned voice) from Engine's own Python environment when the user has installed it.

use crate::{schedule::Block, store::Item};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// How long the helper (and its loaded models) stays alive with no voice use.
pub const IDLE: Duration = Duration::from_secs(15 * 60);
/// Longest recording kept, so a stuck key can't fill memory.
const MAX_SECONDS: usize = 120;

const HELPER: &str = include_str!("voice_helper.py");

fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

// ---------- recording ----------

pub struct Recording {
    stop: mpsc::Sender<()>,
    done: JoinHandle<Result<(Vec<f32>, u32), String>>,
    /// What has been heard so far, readable while recording (for acting on phrases as they're finished).
    samples: Arc<Mutex<Vec<f32>>>,
    pub rate: u32,
}

/// Starts recording the default microphone, mixed down to mono.
pub fn record() -> Result<Recording, String> {
    let (stop, stopped) = mpsc::channel::<()>();
    let (ready_tx, ready) = mpsc::channel::<(u32, Arc<Mutex<Vec<f32>>>)>();
    let done = std::thread::spawn(move || {
        let device = cpal::default_host().default_input_device().ok_or("No microphone found.")?;
        let supported = device.default_input_config().map_err(|e| format!("Couldn't use the microphone: {e}"))?;
        let (rate, channels, format) = (supported.sample_rate(), supported.channels() as usize, supported.sample_format());
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let limit = rate as usize * MAX_SECONDS;
        let sink = samples.clone();
        let push = move |frame: &mut dyn Iterator<Item = f32>| {
            let mut s = sink.lock().unwrap();
            if s.len() < limit {
                s.extend(frame);
            }
        };
        let mono = move |data: Vec<f32>| -> Vec<f32> {
            data.chunks(channels.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect()
        };
        let err = |e| eprintln!("microphone: {e}");
        let config = supported.config();
        let stream = match format {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(config, move |d: &[f32], _: &_| push(&mut mono(d.to_vec()).into_iter()), err, None)
            }
            cpal::SampleFormat::I16 => {
                device.build_input_stream(
                    config,
                    move |d: &[i16], _: &_| push(&mut mono(d.iter().map(|&x| x as f32 / i16::MAX as f32).collect()).into_iter()),
                    err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                device.build_input_stream(
                    config,
                    move |d: &[u16], _: &_| {
                        push(&mut mono(d.iter().map(|&x| (x as f32 - 32768.0) / 32768.0).collect()).into_iter())
                    },
                    err,
                    None,
                )
            }
            other => return Err(format!("The microphone uses a sample format Engine can't read ({other:?}).")),
        }
        .map_err(|e| format!("Couldn't open the microphone: {e}"))?;
        stream.play().map_err(|e| format!("Couldn't start the microphone: {e}"))?;
        let _ = ready_tx.send((rate, samples.clone()));
        let _ = stopped.recv();
        drop(stream);
        let taken = std::mem::take(&mut *samples.lock().unwrap());
        Ok((taken, rate))
    });
    match ready.recv() {
        Ok((rate, samples)) => Ok(Recording { stop, done, samples, rate }),
        Err(_) => Err(done.join().ok().and_then(|r| r.err()).unwrap_or_else(|| "The microphone stopped.".into())),
    }
}

impl Recording {
    /// The audio heard from sample `from` onwards, while still recording.
    pub fn since(&self, from: usize) -> Vec<f32> {
        let s = self.samples.lock().unwrap();
        s.get(from..).map(<[f32]>::to_vec).unwrap_or_default()
    }

    /// How many samples have been heard since recording started (or since the last `clear`).
    pub fn len(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    /// Forgets what's been heard, so an always-on listener stays small between phrases.
    pub fn clear(&self) {
        self.samples.lock().unwrap().clear();
    }

    /// How loud the last tenth of a second was, 0 to 1, for the listening animation.
    pub fn level(&self) -> f32 {
        let s = self.samples.lock().unwrap();
        let tail = &s[s.len().saturating_sub(self.rate as usize / 10)..];
        if tail.is_empty() {
            return 0.0;
        }
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        (rms * 6.0).min(1.0)
    }

    /// Stops and writes the audio as a 16-bit mono WAV. None if it was too short to hold any speech.
    pub fn finish(self, wav: &Path) -> Result<Option<PathBuf>, String> {
        let _ = self.stop.send(());
        let (samples, rate) = self.done.join().map_err(|_| "The recording failed.".to_string())??;
        if samples.len() < rate as usize * 3 / 10 {
            return Ok(None);
        }
        write_wav(wav, &samples, rate)?;
        Ok(Some(wav.to_path_buf()))
    }
}

/// How long a pause must be before a phrase counts as finished.
pub const PAUSE_SECONDS: f32 = 0.35;

/// Which timed pieces of speech are finished, given how much audio there is so far. Every piece but the last is
/// finished (Whisper splits at pauses); the last is finished once a pause follows it. Returns the finished texts
/// and the time (seconds into this audio) they end at.
pub fn finished(segments: &[(f32, f32, String)], audio_seconds: f32) -> (Vec<String>, f32) {
    let mut done: Vec<(String, f32)> = Vec::new();
    for (i, (_, end, text)) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        if !last || *end <= audio_seconds - PAUSE_SECONDS {
            done.push((text.clone(), *end));
        }
    }
    // "Can you open… Opera": a phrase ending on a word that needs more ("open", "and") waits for the rest,
    // unless the person has clearly stopped.
    while let Some((text, end)) = done.last() {
        if dangling(text) && *end > audio_seconds - HOLD_SECONDS {
            done.pop();
        } else {
            break;
        }
    }
    let until = done.last().map_or(0.0, |d| d.1);
    (done.into_iter().map(|d| d.0).collect(), until)
}

/// Silence after which a phrase counts as finished even if it ends mid-thought.
const HOLD_SECONDS: f32 = 1.6;

/// Whether a phrase ends on a word that needs something after it.
pub fn dangling(text: &str) -> bool {
    const NEEDS_MORE: [&str; 16] =
        ["open", "launch", "start", "play", "search", "find", "and", "the", "to", "a", "my", "for", "on", "of", "can", "please"];
    let last = text.split_whitespace().last().unwrap_or_default();
    let ends_sentence = last.ends_with(['.', '!']) && !last.ends_with("..");
    let word = last.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    NEEDS_MORE.contains(&word.as_str()) && !(ends_sentence && word != "open")
}

pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("Couldn't save the recording: {e}"))?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

// ---------- the Python helper ----------

pub struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub last_used: Instant,
}

impl Helper {
    /// Starts the helper with `python`, writing its script into `dir` first so it always matches this build.
    pub fn start(python: &Path, dir: &Path) -> Result<Self, String> {
        let script = dir.join("voice_helper.py");
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&script, HELPER).map_err(|e| format!("Couldn't write the voice helper: {e}"))?;
        let mut child = no_window(Command::new(python).arg("-u").arg(&script))
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start Python for voice ({e}). Is Python installed?"))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        Ok(Self { child, stdin, stdout, last_used: Instant::now() })
    }

    /// Sends one request and waits for its one-line JSON reply.
    pub fn call(&mut self, request: &Value) -> Result<Value, String> {
        self.last_used = Instant::now();
        writeln!(self.stdin, "{request}").and_then(|()| self.stdin.flush()).map_err(|_| "The voice helper stopped.".to_string())?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("The voice helper stopped.".into());
        }
        self.last_used = Instant::now();
        let reply: Value = serde_json::from_str(&line).map_err(|e| format!("The voice helper said something odd: {e}"))?;
        match reply["error"].as_str() {
            Some(e) => Err(e.to_string()),
            None => Ok(reply),
        }
    }

    /// Timed pieces of speech in `wav`: (start seconds, end seconds, text).
    pub fn segments(&mut self, wav: &Path, model: &str) -> Result<Vec<(f32, f32, String)>, String> {
        let reply = self.call(&json!({"cmd": "segments", "path": wav, "model": model}))?;
        Ok(reply["segments"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|s| {
                        let t = |k: &str| s[k].as_f64().unwrap_or_default() as f32;
                        (t("start"), t("end"), s["text"].as_str().unwrap_or_default().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn transcribe(&mut self, wav: &Path, model: &str) -> Result<String, String> {
        let reply = self.call(&json!({"cmd": "transcribe", "path": wav, "model": model}))?;
        Ok(reply["text"].as_str().unwrap_or_default().trim().to_string())
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// The Python to use: Engine's own voice environment once installed, otherwise `python` on the PATH.
pub fn python(voice_env: &Path) -> PathBuf {
    let own = voice_env.join("Scripts").join("python.exe");
    if own.is_file() { own } else { PathBuf::from("python") }
}

/// Checks what voice pieces are available: faster-whisper (speech in) and Chatterbox (cloned voice out).
pub fn status(python: &Path) -> Value {
    let probe = "import importlib.util as u, json; print(json.dumps({m: bool(u.find_spec(m)) for m in ['faster_whisper', 'chatterbox', 'kokoro_onnx']}))";
    let out = no_window(Command::new(python).args(["-c", probe])).output();
    match out {
        Ok(o) if o.status.success() => {
            let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(json!({}));
            json!({"python": true, "whisper": v["faster_whisper"] == true, "chatterbox": v["chatterbox"] == true,
                   "kokoro": v["kokoro_onnx"] == true})
        }
        _ => json!({"python": false, "whisper": false, "chatterbox": false, "kokoro": false}),
    }
}

/// Builds Engine's voice environment with uv: Python 3.11, faster-whisper and Chatterbox. Long (a few GB); the
/// user starts it from Settings. `progress` gets each output line.
pub fn install(voice_env: &Path, mut progress: impl FnMut(String)) -> Result<(), String> {
    let mut run = |args: &[&str]| -> Result<(), String> {
        let mut child = no_window(Command::new("uv").args(args))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't run uv ({e}). Install it from https://docs.astral.sh/uv/ and try again."))?;
        let stderr = child.stderr.take().expect("stderr is piped");
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            progress(line);
        }
        match child.wait() {
            Ok(s) if s.success() => Ok(()),
            _ => Err("The voice install failed. The last lines above say why.".into()),
        }
    };
    let env = voice_env.to_string_lossy().into_owned();
    run(&["venv", "--python", "3.11", &env])?;
    let py = voice_env.join("Scripts").join("python.exe").to_string_lossy().into_owned();
    // setuptools under 81 keeps pkg_resources, which Chatterbox's watermarker still imports. Kokoro (the British
    // voice) stays on 0.3, the last release that works with the numpy Chatterbox needs.
    run(&["pip", "install", "--python", &py, "chatterbox-tts", "faster-whisper", "setuptools<81", "kokoro-onnx==0.3.3"])
}

// ---------- speech out ----------

/// Whether Engine is playing speech right now (so the wake word listener doesn't hear Engine itself).
pub static PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Plays a WAV file and waits for it to finish. `hush` cuts it short from any thread.
#[cfg(windows)]
pub fn play(wav: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_FILENAME, SND_NODEFAULT, SND_SYNC};
    let wide: Vec<u16> = wav.as_os_str().encode_wide().chain(Some(0)).collect();
    PLAYING.store(true, Ordering::Relaxed);
    // SAFETY: a NUL-terminated path; PlaySoundW only reads it.
    unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_NODEFAULT | SND_SYNC) };
    PLAYING.store(false, Ordering::Relaxed);
}

/// Stops whatever `play` is playing, e.g. when the person starts talking.
#[cfg(windows)]
pub fn hush() {
    // SAFETY: a null sound stops the one playing; nothing is read.
    unsafe { windows_sys::Win32::Media::Audio::PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
}

#[cfg(not(windows))]
pub fn play(_: &Path) {}

#[cfg(not(windows))]
pub fn hush() {}

/// A short spoken summary of the briefing: today's blocks, then running, needs-you and done items.
pub fn briefing_text(items: &[Item], today: &[Block]) -> String {
    fn list(names: Vec<String>) -> String {
        match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            2 => format!("{} and {}", names[0], names[1]),
            n if n <= 3 => format!("{}, and {}", names[..n - 1].join(", "), names[n - 1]),
            n => format!("{}, and {} more", names[..2].join(", "), n - 2),
        }
    }
    let titles = |pred: &dyn Fn(&Item) -> bool| items.iter().filter(|i| pred(i)).map(|i| i.title.clone()).collect::<Vec<_>>();
    let running = titles(&|i| i.kind != "instant" && matches!(i.status.as_str(), "running" | "queued"));
    let needs = titles(&|i| i.kind != "instant" && matches!(i.status.as_str(), "held" | "failed"));
    let done = titles(&|i| i.kind != "instant" && i.status == "done");

    let mut parts = vec!["Here's your briefing.".to_string()];
    if !today.is_empty() {
        let blocks = today.iter().map(|b| format!("{} at {}", b.title, &b.start[11..])).collect();
        parts.push(format!("Today: {}.", list(blocks)));
    }
    let count = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    if !running.is_empty() {
        parts.push(format!("{} running: {}.", count(running.len(), "job", "jobs"), list(running.clone())));
    }
    if !needs.is_empty() {
        let verb = if needs.len() == 1 { "needs" } else { "need" };
        parts.push(format!("{} {verb} you: {}.", count(needs.len(), "thing", "things"), list(needs.clone())));
    }
    if !done.is_empty() {
        parts.push(format!("Done: {}.", list(done.clone())));
    }
    if parts.len() == 1 {
        parts.push("Nothing on yet today.".into());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str, title: &str, status: &str) -> Item {
        Item {
            id: 0,
            kind: kind.into(),
            title: title.into(),
            detail: String::new(),
            status: status.into(),
            process: String::new(),
            result: String::new(),
            created: String::new(),
            verdict: String::new(),
            verdict_reason: String::new(),
            dump: None,
            said: String::new(),
        }
    }

    #[test]
    fn speaks_a_short_briefing() {
        let items = [
            item("instant", "Open Spotify", "done"),
            item("job", "Research headphones", "running"),
            item("outward", "Send reply to Sam", "held"),
            item("answer", "Plan A", "done"),
            item("answer", "Plan B", "done"),
            item("answer", "Plan C", "done"),
            item("answer", "Plan D", "done"),
        ];
        let today = [Block { title: "Physics".into(), start: "2026-11-17T19:00".into(), ..Default::default() }];
        assert_eq!(
            briefing_text(&items, &today),
            "Here's your briefing. Today: Physics at 19:00. 1 job running: Research headphones. \
             1 thing needs you: Send reply to Sam. Done: Plan A, Plan B, and 2 more."
        );
        assert_eq!(briefing_text(&[], &[]), "Here's your briefing. Nothing on yet today.");
    }

    #[test]
    fn acts_on_finished_phrases_only() {
        let segs = vec![(0.0, 1.2, "open spotify".to_string()), (1.6, 2.9, "and pause".to_string())];
        // The last phrase ended just now: it may still be going.
        assert_eq!(finished(&segs, 3.0), (vec!["open spotify".to_string()], 1.2));
        // A pause after it: both are done.
        let (done, until) = finished(&segs, 3.8);
        assert_eq!((done.len(), until), (2, 2.9));
        assert_eq!(finished(&[], 2.0), (vec![], 0.0));

        // "Can you open… Opera": the dangling phrase waits for the rest, then goes once the person stops.
        let split = vec![(0.0, 1.0, "Hello, can you open?".to_string()), (1.5, 2.0, "Opera.".to_string())];
        assert_eq!(finished(&split[..1], 1.8), (vec![], 0.0));
        assert_eq!(finished(&split, 2.7).0, vec!["Hello, can you open?".to_string(), "Opera.".to_string()]);
        assert_eq!(finished(&split[..1], 3.0).0.len(), 1); // long silence: give up waiting
    }

    #[test]
    fn writes_mono_wav() {
        let path = std::env::temp_dir().join("engine-voice-test").join("a.wav");
        write_wav(&path, &[0.0, 0.5, -0.5, 1.5], 16_000).unwrap();
        let r = hound::WavReader::open(&path).unwrap();
        assert_eq!((r.spec().channels, r.spec().sample_rate, r.len()), (1, 16_000, 4));
    }
}
