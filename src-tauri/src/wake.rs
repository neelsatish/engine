//! The wake word. With it on, Engine keeps the microphone open, finds bursts of speech by their loudness, and
//! transcribes each one; a burst starting with "Engine" wakes it. Audio stays on this PC and nothing is kept.

/// Whether the wake word is on (from Settings).
pub static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Words that may come before "Engine" ("Hey Engine", "Okay, Engine").
const GREETINGS: [&str; 5] = ["hey", "okay", "ok", "hi", "yo"];

/// What follows the wake word, or `None` when the speech doesn't start with it. Empty means "Engine" on its own.
/// Matched by whole words, so punctuation Whisper puts between them doesn't matter and "engineering" isn't "engine".
pub fn command(heard: &str) -> Option<String> {
    let words: Vec<(usize, &str)> = heard
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .scan(0, |at, w| {
            let start = *at;
            *at += w.len() + 1;
            Some((start, w))
        })
        .filter(|(_, w)| !w.is_empty())
        .take(2)
        .collect();
    let is = |i: usize, w: &str| words.get(i).is_some_and(|(_, x)| x.eq_ignore_ascii_case(w));
    let wake = if is(0, "engine") {
        0
    } else if GREETINGS.iter().any(|g| is(0, g)) && is(1, "engine") {
        1
    } else {
        return None;
    };
    let (at, word) = words[wake];
    let rest = &heard[at + word.len()..];
    Some(rest.trim_start_matches(|c: char| !c.is_alphanumeric()).trim_end_matches(['.', '!', '?']).trim().to_string())
}

/// Seconds of sound kept from before speech was first loud, so the start of the first word isn't clipped.
const PRE_ROLL: f32 = 0.4;
/// Quiet this long ends a burst.
const END_QUIET_MS: u32 = 500;
/// A burst must be loud at least this long to be worth transcribing (skips clicks, taps and coughs).
const MIN_LOUD_MS: u32 = 200;
/// Bursts are cut at this length: a wake word comes first, so nothing later matters.
const MAX_SECONDS: usize = 8;
/// Never treat anything quieter than this (on the 0 to 1 level scale) as speech.
const FLOOR: f32 = 0.04;

/// Finds bursts of speech in a stream of loudness readings, against a noise level it learns while it's quiet.
#[derive(Default)]
pub struct Gate {
    noise: f32,
    start: Option<usize>,
    loud_ms: u32,
    quiet_ms: u32,
}

impl Gate {
    /// Feeds one reading: `level` for the last `step_ms`, with `len` samples heard so far at `rate`. Returns the
    /// sample range of a burst once it has ended.
    pub fn push(&mut self, level: f32, len: usize, rate: u32, step_ms: u32) -> Option<(usize, usize)> {
        let loud = level > (self.noise * 3.0).max(FLOOR);
        let Some(start) = self.start else {
            if loud {
                self.start = Some(len.saturating_sub((rate as f32 * PRE_ROLL) as usize));
                (self.loud_ms, self.quiet_ms) = (step_ms, 0);
            } else {
                self.noise = if self.noise == 0.0 { level } else { self.noise * 0.95 + level * 0.05 };
            }
            return None;
        };
        if loud {
            (self.loud_ms, self.quiet_ms) = (self.loud_ms + step_ms, 0);
        } else {
            self.quiet_ms += step_ms;
        }
        let too_long = len - start >= rate as usize * MAX_SECONDS;
        if self.quiet_ms < END_QUIET_MS && !too_long {
            return None;
        }
        self.start = None;
        (self.loud_ms >= MIN_LOUD_MS).then_some((start, len))
    }

    /// Whether a burst is in progress.
    pub fn in_speech(&self) -> bool {
        self.start.is_some()
    }

    /// Drops any burst in progress (the audio it pointed into was cleared). The learned noise level is kept.
    pub fn reset(&mut self) {
        self.start = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hears_the_wake_word() {
        assert_eq!(command("Engine.").as_deref(), Some(""));
        assert_eq!(command(" Hey Engine, open Spotify.").as_deref(), Some("open Spotify"));
        assert_eq!(command("Okay engine what's the time?").as_deref(), Some("what's the time"));
        assert_eq!(command("Okay, engine.").as_deref(), Some(""));
        assert_eq!(command("Hey. Engine? Play lofi on Spotify").as_deref(), Some("Play lofi on Spotify"));
        assert_eq!(command("Engineering is hard."), None);
        assert_eq!(command("Open the engine bay."), None);
        assert_eq!(command(""), None);
    }

    #[test]
    fn finds_bursts_of_speech() {
        let (rate, step) = (16_000, 100);
        let mut g = Gate::default();
        let mut len = 0;
        let mut feed = |g: &mut Gate, level: f32, steps: usize| {
            let mut found = None;
            for _ in 0..steps {
                len += rate as usize / 10;
                found = found.or(g.push(level, len, rate, step));
            }
            found
        };
        assert_eq!(feed(&mut g, 0.01, 20), None); // room noise: learned, not speech
        assert_eq!(feed(&mut g, 0.3, 6), None); // talking
        assert!(g.in_speech());
        let (start, end) = feed(&mut g, 0.01, 5).expect("half a second of quiet ends the burst");
        assert_eq!(start, 21 * 1600 - 6400); // pre-roll before the first loud reading
        assert_eq!(end, 31 * 1600);
        assert_eq!(feed(&mut g, 0.3, 1), None);
        assert_eq!(feed(&mut g, 0.01, 6), None); // a click is too short to count
    }
}
