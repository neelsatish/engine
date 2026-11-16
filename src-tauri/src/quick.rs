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
        "stop", "stop it", "stop everything", "stop all", "stop all processes", "stop all tasks", "stop all agents",
        "stop all jobs", "stop whatevers going on", "stop whatever is going on", "stop what youre doing",
        "stop what you are doing", "cancel everything", "cancel all",
        "cancel all tasks", "be quiet", "quiet", "shut up", "stop talking", "never mind", "nevermind",
    ]
    .contains(&s.as_str())
}

/// "Mark all briefings as completed", "clear the briefing": clears every finished or waiting item.
pub fn clear_briefing(input: &str) -> bool {
    let s = clean(input);
    let about = ["briefing", "briefings", "items", "tasks", "everything", "all"].iter().any(|w| s.split_whitespace().any(|x| x == *w));
    let clear = ["clear", "mark all", "mark everything", "complete all", "dismiss all"].iter().any(|w| s.starts_with(w));
    let done = s.starts_with("clear") || ["done", "completed", "complete", "finished", "read"].iter().any(|w| s.ends_with(w));
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
        assert!(!stop_all("stop by the shop at 5"));
        assert!(clear_briefing("Mark all briefings as completed"));
        assert!(clear_briefing("clear the briefing"));
        assert!(clear_briefing("mark everything as done"));
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
