//! Window control by voice or chat: "switch to Discord", "minimize Chrome", "maximize Spotify",
//! "snap Opera left", "minimize everything".

/// What to do with a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Focus,
    Minimize,
    Maximize,
    SnapLeft,
    SnapRight,
}

/// A window request: the action and the app's name, or `None` for "minimize everything".
#[derive(Debug, PartialEq)]
pub enum Request {
    One(Act, String),
    MinimizeAll,
}

/// Reads a window request. Only at the start of what was said (after a wake word or "please"), so "I switched
/// to decaf" mid-sentence is left alone.
pub fn parse(input: &str) -> Option<Request> {
    let s = input.trim().to_lowercase();
    let s = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    let mut s = s;
    for lead in ["hey engine", "ok engine", "engine", "please", "can you", "could you"] {
        if let Some(rest) = s.strip_prefix(lead) {
            s = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
        }
    }
    if ["minimize all", "minimize everything", "minimise all", "minimise everything", "show desktop", "show the desktop", "show me the desktop"]
        .contains(&s)
    {
        return Some(Request::MinimizeAll);
    }
    let name = |rest: &str| -> Option<String> {
        let rest = rest.trim();
        let rest = rest.strip_prefix("the ").or_else(|| rest.strip_prefix("my ")).unwrap_or(rest).trim();
        (!rest.is_empty()).then(|| rest.to_string())
    };
    for (verb, act) in [
        ("switch to ", Act::Focus),
        ("focus on ", Act::Focus),
        ("focus ", Act::Focus),
        ("bring up ", Act::Focus),
        ("minimize ", Act::Minimize),
        ("minimise ", Act::Minimize),
        ("maximize ", Act::Maximize),
        ("maximise ", Act::Maximize),
        ("snap ", Act::SnapLeft),
    ] {
        let Some(rest) = s.strip_prefix(verb) else { continue };
        if act == Act::SnapLeft {
            // "snap chrome left", "snap chrome to the right"
            let side = if rest.ends_with("right") { Act::SnapRight } else if rest.ends_with("left") { Act::SnapLeft } else { return None };
            let app = rest.trim_end_matches("left").trim_end_matches("right").trim_end();
            let app = app.strip_suffix(" to the").or_else(|| app.strip_suffix(" to")).unwrap_or(app);
            return name(app).map(|n| Request::One(side, n));
        }
        return name(rest).map(|n| Request::One(act, n));
    }
    None
}

/// Carries out a request. Returns what to tell the person.
#[cfg(windows)]
pub fn act(request: &Request, display_name: &str) -> Result<String, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let (act, query) = match request {
        Request::MinimizeAll => {
            // Win+M, the way Windows itself minimizes everything.
            // SAFETY: plain key presses.
            unsafe {
                keybd_event(VK_LWIN as u8, 0, 0, 0);
                keybd_event(b'M', 0, 0, 0);
                keybd_event(b'M', 0, KEYEVENTF_KEYUP, 0);
                keybd_event(VK_LWIN as u8, 0, KEYEVENTF_KEYUP, 0);
            }
            return Ok("Minimized everything".into());
        }
        Request::One(act, query) => (*act, query.as_str()),
    };
    let hwnd = find(query).ok_or_else(|| format!("{display_name} has no window open"))?;
    // SAFETY: `hwnd` came from EnumWindows just now; these calls only move or show it.
    unsafe {
        match act {
            Act::Minimize => {
                ShowWindow(hwnd, SW_MINIMIZE);
                return Ok(format!("Minimized {display_name}"));
            }
            Act::Maximize => {
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
            Act::SnapLeft | Act::SnapRight => {
                let mut area: windows_sys::Win32::Foundation::RECT = std::mem::zeroed();
                SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut area as *mut _) as *mut _, 0);
                let half = (area.right - area.left) / 2;
                let x = if act == Act::SnapLeft { area.left } else { area.left + half };
                ShowWindow(hwnd, SW_RESTORE);
                MoveWindow(hwnd, x, area.top, half, area.bottom - area.top, 1);
            }
            Act::Focus => {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
            }
        }
        bring_forward(hwnd);
    }
    Ok(match act {
        Act::Focus => format!("Switched to {display_name}"),
        Act::Maximize => format!("Maximized {display_name}"),
        Act::SnapLeft => format!("Snapped {display_name} left"),
        Act::SnapRight => format!("Snapped {display_name} right"),
        Act::Minimize => unreachable!(),
    })
}

#[cfg(not(windows))]
pub fn act(_: &Request, _: &str) -> Result<String, String> {
    Err("Window control only works on Windows.".into())
}

/// Brings a window to the front. Windows only lets the foreground app do that, so a tap of Alt first lifts the
/// lock (the usual workaround).
#[cfg(windows)]
unsafe fn bring_forward(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
    use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    // SAFETY: a key tap and a focus change on a live window.
    unsafe {
        keybd_event(VK_MENU as u8, 0, 0, 0);
        keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);
        SetForegroundWindow(hwnd);
    }
}

/// The main visible window of the app called `query`, matched by process name, then window title.
#[cfg(windows)]
fn find(query: &str) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let q: String = query.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if q.len() < 3 {
        return None;
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the Vec passed below, alive for the whole EnumWindows call.
        let found = unsafe { &mut *(data as *mut Vec<(HWND, u32, String)>) };
        // SAFETY: read-only queries on a window handle Windows just gave us.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || GetWindow(hwnd, GW_OWNER) as usize != 0 {
                return 1;
            }
            let mut title = [0u16; 256];
            let n = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if n <= 0 {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            found.push((hwnd, pid, String::from_utf16_lossy(&title[..n as usize])));
        }
        1
    }
    let mut windows: Vec<(HWND, u32, String)> = Vec::new();
    // SAFETY: the callback only touches `windows`, which outlives the call.
    unsafe { EnumWindows(Some(each), (&mut windows as *mut _) as LPARAM) };
    let mut sys = sysinfo::System::new().ok()?;
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let plain = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let me = std::process::id();
    let skip = ["explorer", "applicationframehost", "textinputhost", "engine"];
    let by_process = windows.iter().find(|(_, pid, _)| {
        *pid != me
            && sys.process(sysinfo::Pid::from_u32(*pid)).is_some_and(|p| {
                let name = plain(&p.name().to_string_lossy().replace(".exe", ""));
                !skip.contains(&name.as_str()) && (name.contains(&q) || (name.len() >= 4 && q.contains(&name)))
            })
    });
    by_process
        .or_else(|| windows.iter().find(|(_, pid, title)| *pid != me && plain(title).contains(&q)))
        .map(|w| w.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_window_requests() {
        assert_eq!(parse("switch to Discord"), Some(Request::One(Act::Focus, "discord".into())));
        assert_eq!(parse("Engine, minimize Chrome."), Some(Request::One(Act::Minimize, "chrome".into())));
        assert_eq!(parse("snap spotify to the right"), Some(Request::One(Act::SnapRight, "spotify".into())));
        assert_eq!(parse("snap opera left"), Some(Request::One(Act::SnapLeft, "opera".into())));
        assert_eq!(parse("minimize everything"), Some(Request::MinimizeAll));
        assert_eq!(parse("I switched to decaf"), None);
        assert_eq!(parse("snap a photo"), None);
    }
}
