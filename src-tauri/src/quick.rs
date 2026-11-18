//! Instant commands that need no model: media keys, volume, YouTube and Spotify search, web search, locking the PC.
//! "pause the music", "next song", "volume 30", "play lofi on spotify", "lock my pc".

#[derive(Debug, PartialEq)]
pub enum Quick {
    Media(Media),
    /// Volume steps of 2% (positive up, negative down).
    VolumeStep(i32),
    /// Set the volume to a percentage.
    VolumeSet(u8),
    Mute,
    YouTube(String),
    Spotify(String),
    Search(String),
    Lock,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Media {
    PlayPause,
    Next,
    Previous,
}

/// Lowercases and strips the wake word, "please" and trailing punctuation.
fn clean(input: &str) -> String {
    let mut s = input.trim().to_lowercase();
    for prefix in ["hey engine", "ok engine", "engine", "please", "can you", "could you"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with([' ', ',']) {
                s = rest.trim_start_matches([' ', ',']).to_string();
            }
        }
    }
    let s = s.trim_end_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    s.strip_suffix(" please").unwrap_or(s).trim().to_string()
}

/// "Stop everything": Engine should end its running agents and stop talking. Handled in the core, since it needs
/// Engine's own state rather than a key press.
pub fn stop_all(input: &str) -> bool {
    let s = clean(input).replace(['\u{2019}', '\''], "");
    [
        "stop everything", "stop all", "stop all processes", "stop all tasks", "stop all agents", "stop all jobs",
        "stop whatevers going on", "stop whatever is going on", "stop what youre doing", "stop what you are doing",
        "cancel everything", "cancel all", "cancel all tasks",
    ]
    .contains(&s.as_str())
}

/// "Stop", "quiet", "never mind": stop talking only. Agents keep working; "stop everything" ends those.
pub fn hush(input: &str) -> bool {
    let s = clean(input).replace(['\u{2019}', '\''], "");
    ["stop", "stop it", "be quiet", "quiet", "shut up", "stop talking", "never mind", "nevermind", "thats enough", "enough"]
        .contains(&s.as_str())
}

/// Small talk answered on the spot, with no model: "hi", "wsp", "thanks", "how are you".
pub fn small_talk(input: &str) -> Option<&'static str> {
    let s = clean(input).replace(['\u{2019}', '\''], "");
    Some(match s.as_str() {
        "hi" | "hey" | "hello" | "yo" | "hiya" | "hey there" | "hi there" => "Hello, sir. What can I do for you?",
        "wsp" | "sup" | "whats up" | "wassup" | "whatsup" | "what up" => "Not much, sir. All systems running. What do you need?",
        "how are you" | "how are you doing" | "hows it going" | "you good" => "Running smoothly, thank you. How can I help?",
        "thanks" | "thank you" | "thanks engine" | "cheers" | "ty" | "thx" => "Anytime, sir.",
        "good morning" => "Good morning, sir. Ready when you are.",
        "good afternoon" => "Good afternoon, sir. What's next?",
        "good evening" => "Good evening, sir. What can I do for you?",
        "good night" | "goodnight" => "Good night, sir.",
        "you there" | "are you there" | "you awake" => "Always, sir.",
        _ => return None,
    })
}

/// "Timer 10 minutes", "set a timer for 90 seconds", "5 minute timer": the minutes (rounded up), or None.
pub fn timer(input: &str) -> Option<u32> {
    let s = clean(input);
    // "Cancel my 10 minute timer" or "how long is left on the timer" isn't a new timer.
    let asking = ["cancel", "stop", "delete", "remove", "how", "what", "when", "is "].iter().any(|w| s.starts_with(w));
    if !s.contains("timer") || asking {
        return None;
    }
    let words: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == '-').filter(|w| !w.is_empty()).collect();
    let number = |w: &str| -> Option<f32> {
        w.parse::<f32>().ok().or(match w {
            "a" | "an" | "one" => Some(1.0),
            "two" => Some(2.0),
            "three" => Some(3.0),
            "five" => Some(5.0),
            "ten" => Some(10.0),
            "fifteen" => Some(15.0),
            "twenty" => Some(20.0),
            "thirty" => Some(30.0),
            "half" => Some(0.5),
            _ => None,
        })
    };
    let mut total = 0.0;
    let mut skip = 0; // words already used by the number before them ("half an hour": the "an")
    for (i, w) in words.iter().enumerate() {
        if i < skip {
            continue;
        }
        let Some(n) = number(w) else { continue };
        let unit = words.get(i + 1).copied().unwrap_or_default();
        let (unit, used) = if unit == "an" || unit == "a" { (words.get(i + 2).copied().unwrap_or_default(), i + 3) } else { (unit, i + 2) };
        skip = used;
        total += match unit.trim_end_matches('s') {
            "minute" | "min" => n,
            "second" | "sec" => n / 60.0,
            "hour" | "hr" => n * 60.0,
            _ => 0.0,
        };
    }
    (total > 0.0).then(|| total.ceil() as u32)
}

/// "Clear the chat", "clear the conversation": clears finished messages from the chat.
pub fn clear_chat(input: &str) -> bool {
    let s = clean(input);
    ["clear chat", "clear the chat", "clear my chat", "clear the conversation", "clear conversation", "clear history", "clear the history"]
        .contains(&s.as_str())
}

/// "Mark all briefings as completed", "clear the briefing": clears every finished or waiting item.
pub fn clear_briefing(input: &str) -> bool {
    let s = clean(input);
    // Must name the briefing: "mark all my emails as read" or "clear all my downloads" isn't about Engine.
    let about = s.split_whitespace().any(|x| x == "briefing" || x == "briefings");
    let clear = ["clear", "mark all", "mark everything", "mark my", "mark the", "complete all", "dismiss all", "empty"].iter().any(|w| s.starts_with(w));
    let done = s.starts_with("clear") || s.starts_with("empty") || ["done", "completed", "complete", "finished", "read"].iter().any(|w| s.ends_with(w));
    about && clear && done
}

/// The text between `prefix` and `suffix` in `s`, if `s` has that shape and the middle isn't empty.
fn between<'a>(s: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let middle = s.strip_prefix(prefix)?.strip_suffix(suffix)?.trim();
    (!middle.is_empty()).then_some(middle)
}

pub fn parse(input: &str) -> Option<Quick> {
    let s = clean(input);
    let s = s.as_str();
    let any = |options: &[&str]| options.contains(&s);

    if any(&["play", "resume", "unpause", "play music", "play some music", "resume music", "resume the music",
        "pause", "pause music", "pause the music", "pause it", "stop music", "stop the music", "pause the song",
        "pause the video", "pause spotify"])
    {
        return Some(Quick::Media(Media::PlayPause));
    }
    if any(&["next", "skip", "next song", "next track", "skip song", "skip this song", "skip track", "skip this track"]) {
        return Some(Quick::Media(Media::Next));
    }
    if any(&["previous", "previous song", "previous track", "last song", "go back a song", "back a song"]) {
        return Some(Quick::Media(Media::Previous));
    }
    if any(&["volume up", "turn it up", "turn the volume up", "turn up the volume", "louder"]) {
        return Some(Quick::VolumeStep(5));
    }
    if any(&["volume down", "turn it down", "turn the volume down", "turn down the volume", "quieter"]) {
        return Some(Quick::VolumeStep(-5));
    }
    if any(&["mute", "unmute", "mute the volume", "mute sound"]) {
        return Some(Quick::Mute);
    }
    for prefix in ["set the volume to ", "set volume to ", "volume to ", "volume "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let n = rest.trim_end_matches('%').trim_end_matches(" percent").trim();
            if let Ok(n) = n.parse::<u8>() {
                return (n <= 100).then_some(Quick::VolumeSet(n));
            }
        }
    }
    if any(&["lock my pc", "lock the pc", "lock my computer", "lock the computer", "lock my screen", "lock the screen",
        "lock my laptop", "lock pc", "lock computer", "lock screen"])
    {
        return Some(Quick::Lock);
    }
    for (prefix, suffix) in [("play ", " on youtube"), ("watch ", " on youtube"), ("search youtube for ", ""), ("youtube ", "")] {
        if let Some(q) = between(s, prefix, suffix) {
            return Some(Quick::YouTube(q.to_string()));
        }
    }
    for (prefix, suffix) in [("play ", " on spotify"), ("search spotify for ", ""), ("spotify ", "")] {
        if let Some(q) = between(s, prefix, suffix) {
            return Some(Quick::Spotify(q.to_string()));
        }
    }
    for prefix in ["search the web for ", "search google for ", "search for ", "google "] {
        if let Some(q) = between(s, prefix, "") {
            return Some(Quick::Search(q.to_string()));
        }
    }
    None
}

/// Percent-encodes text for a URL query or path segment.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "%20".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Quick {
    /// What the briefing says it did.
    pub fn label(&self) -> String {
        match self {
            Quick::Media(Media::PlayPause) => "Play/pause".into(),
            Quick::Media(Media::Next) => "Next track".into(),
            Quick::Media(Media::Previous) => "Previous track".into(),
            Quick::VolumeStep(n) if *n > 0 => "Volume up".into(),
            Quick::VolumeStep(_) => "Volume down".into(),
            Quick::VolumeSet(n) => format!("Volume {n}%"),
            Quick::Mute => "Mute on/off".into(),
            Quick::YouTube(q) => format!("YouTube: {q}"),
            Quick::Spotify(q) => format!("Spotify: {q}"),
            Quick::Search(q) => format!("Searched the web for {q}"),
            Quick::Lock => "Locked the PC".into(),
        }
    }

    /// The URL to open, for the commands that open something.
    pub fn url(&self, spotify_app: bool) -> Option<String> {
        match self {
            Quick::YouTube(q) => Some(format!("https://www.youtube.com/results?search_query={}", encode(q))),
            Quick::Spotify(q) if spotify_app => Some(format!("spotify:search:{}", encode(q))),
            Quick::Spotify(q) => Some(format!("https://open.spotify.com/search/{}", encode(q))),
            Quick::Search(q) => Some(format!("https://www.google.com/search?q={}", encode(q))),
            _ => None,
        }
    }
}

/// Presses media and volume keys, or locks the PC. Commands that open a URL go through the launcher instead.
#[cfg(windows)]
pub fn press(q: &Quick) -> Result<(), String> {
    use windows_sys::Win32::System::Shutdown::LockWorkStation;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_VOLUME_DOWN,
        VK_VOLUME_MUTE, VK_VOLUME_UP,
    };
    let tap = |vk: u16, times: u32| {
        for _ in 0..times {
            // SAFETY: keybd_event takes plain values and synthesizes one key press.
            unsafe {
                keybd_event(vk as u8, 0, 0, 0);
                keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
            }
        }
    };
    match q {
        Quick::Media(Media::PlayPause) => tap(VK_MEDIA_PLAY_PAUSE, 1),
        Quick::Media(Media::Next) => tap(VK_MEDIA_NEXT_TRACK, 1),
        Quick::Media(Media::Previous) => tap(VK_MEDIA_PREV_TRACK, 1),
        Quick::VolumeStep(n) if *n > 0 => tap(VK_VOLUME_UP, *n as u32),
        Quick::VolumeStep(n) => tap(VK_VOLUME_DOWN, n.unsigned_abs()),
        // Each volume key moves 2%: go to zero, then up to the target.
        Quick::VolumeSet(n) => {
            tap(VK_VOLUME_DOWN, 50);
            tap(VK_VOLUME_UP, (*n as u32).div_ceil(2));
        }
        Quick::Mute => tap(VK_VOLUME_MUTE, 1),
        // SAFETY: LockWorkStation takes no arguments and only locks the session.
        Quick::Lock => {
            if unsafe { LockWorkStation() } == 0 {
                return Err("Windows didn't lock the PC.".into());
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn press(_: &Quick) -> Result<(), String> {
    Err("Media keys only work on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quick_commands() {
        assert_eq!(parse("Pause the music."), Some(Quick::Media(Media::PlayPause)));
        assert_eq!(parse("hey engine, next song please"), Some(Quick::Media(Media::Next)));
        assert_eq!(parse("go back a song"), Some(Quick::Media(Media::Previous)));
        assert_eq!(parse("turn it up"), Some(Quick::VolumeStep(5)));
        assert_eq!(parse("set the volume to 30%"), Some(Quick::VolumeSet(30)));
        assert_eq!(parse("volume 101"), None);
        assert_eq!(parse("Play lofi beats on Spotify"), Some(Quick::Spotify("lofi beats".into())));
        assert_eq!(parse("play cooking videos on youtube"), Some(Quick::YouTube("cooking videos".into())));
        assert_eq!(parse("search the web for tauri updater"), Some(Quick::Search("tauri updater".into())));
        assert_eq!(parse("lock my pc"), Some(Quick::Lock));
        // Not quick commands: these go to the launcher or the router.
        assert_eq!(parse("open spotify"), None);
        assert!(stop_all("Stop whatever’s going on."));
        assert!(stop_all("Engine, stop all processes"));
        assert!(!stop_all("stop the music")); // that's a media key, handled by parse
        assert!(!stop_all("stop") && hush("Stop.") && hush("never mind")); // plain "stop" only silences Engine
        assert!(!stop_all("stop by the shop at 5"));
        assert!(small_talk("wsp").is_some() && small_talk("What's up?").is_some() && small_talk("Thanks!").is_some());
        assert!(small_talk("what's up with my build").is_none());
        assert_eq!(timer("timer 10 minutes"), Some(10));
        assert_eq!(timer("Engine, set a timer for 90 seconds"), Some(2));
        assert_eq!(timer("5 minute timer"), Some(5));
        assert_eq!(timer("set a timer for an hour"), Some(60));
        assert_eq!(timer("what's a timer"), None);
        assert!(clear_chat("Clear the chat.") && !clear_briefing("clear the chat") && !clear_chat("clear the briefing"));
        assert!(clear_briefing("Mark all briefings as completed"));
        assert!(clear_briefing("clear the briefing"));
        assert!(clear_briefing("mark my briefing as done"));
        assert!(!clear_briefing("mark all my emails as read") && !clear_briefing("clear all my downloads"));
        assert_eq!(timer("set a timer for half an hour"), Some(30));
        assert_eq!(timer("cancel my 10 minute timer"), None);
        assert_eq!(timer("how long is left on the timer"), None);
        assert!(!clear_briefing("mark the meeting in my calendar"));
        assert_eq!(parse("play"), Some(Quick::Media(Media::PlayPause)));
        assert_eq!(parse("make me a playlist for studying"), None);
        assert_eq!(parse("research the best headphones"), None);
    }

    #[test]
    fn builds_urls() {
        assert_eq!(encode("lo-fi & chill"), "lo-fi%20%26%20chill");
        let yt = Quick::YouTube("a b".into()).url(false).unwrap();
        assert_eq!(yt, "https://www.youtube.com/results?search_query=a%20b");
        assert_eq!(Quick::Spotify("x y".into()).url(true).unwrap(), "spotify:search:x%20y");
        assert_eq!(Quick::Spotify("x".into()).url(false).unwrap(), "https://open.spotify.com/search/x");
        assert_eq!(Quick::Lock.url(true), None);
    }
}
