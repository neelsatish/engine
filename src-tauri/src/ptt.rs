//! Push-to-talk on the key under Esc (` and ~ on a US layout), matched by its position so any layout works.
//! A tap still types the character; holding it past `HOLD_MS` listens until it's let go. With Shift, Ctrl, Alt or
//! Win held, the key passes through untouched (Shift types ~, Ctrl+` toggles editor terminals).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

/// How long the key must be held before Engine starts listening.
pub const HOLD_MS: u64 = 250;
/// Scan code of the key under Esc on PC keyboards, whatever the layout.
const SCAN_CODE: u32 = 0x29;

/// Whether hold-to-talk is on (from Settings).
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// What the hook should do with one press or release of the key.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Not ours (modifiers held, or off): let the key through.
    Pass,
    /// Swallow it; on a fresh press, arm the hold timer for this press number.
    Arm(u64),
    /// Swallow a key-repeat while held.
    Swallow,
    /// Released before the hold time: type the character after all.
    Tap,
    /// Released after listening started: stop and transcribe.
    Finish,
}

/// The press/hold state machine, kept apart from Windows so it can be tested.
#[derive(Default)]
pub struct Keys {
    down: bool,
    press: u64,
    listening: bool,
}

impl Keys {
    pub fn key(&mut self, down: bool, modifiers: bool, enabled: bool) -> Action {
        if !self.down && (modifiers || !enabled) {
            return Action::Pass;
        }
        match (down, self.down) {
            (true, false) => {
                self.down = true;
                self.press += 1;
                Action::Arm(self.press)
            }
            (true, true) => Action::Swallow,
            (false, true) => {
                self.down = false;
                if std::mem::take(&mut self.listening) { Action::Finish } else { Action::Tap }
            }
            (false, false) => Action::Pass,
        }
    }

    /// The hold timer fired: start listening only if that same press is still held.
    pub fn held(&mut self, press: u64) -> bool {
        let start = self.down && self.press == press && !self.listening;
        self.listening |= start;
        start
    }
}

static KEYS: std::sync::Mutex<Keys> = std::sync::Mutex::new(Keys { down: false, press: 0, listening: false });
static HANDLERS: OnceLock<(Box<dyn Fn() + Send + Sync>, Box<dyn Fn() + Send + Sync>)> = OnceLock::new();

/// Installs the keyboard hook on its own thread. `start` runs when a hold begins, `finish` when it's released.
#[cfg(windows)]
pub fn install(start: impl Fn() + Send + Sync + 'static, finish: impl Fn() + Send + Sync + 'static) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, SetWindowsHookExW, MSG, WH_KEYBOARD_LL};
    if HANDLERS.set((Box::new(start), Box::new(finish))).is_err() {
        return; // already installed
    }
    std::thread::spawn(|| {
        // SAFETY: a low-level keyboard hook needs a message loop on the thread that installs it; this thread
        // only runs that loop.
        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                eprintln!("couldn't install the push-to-talk key hook");
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        }
    });
}

#[cfg(not(windows))]
pub fn install(_: impl Fn() + Send + Sync + 'static, _: impl Fn() + Send + Sync + 'static) {}

#[cfg(windows)]
unsafe extern "system" fn hook(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    // SAFETY: Windows passes a valid KBDLLHOOKSTRUCT for HC_ACTION keyboard events.
    let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    // Our own re-typed character comes back marked as injected: let it through.
    if code != HC_ACTION as i32 || kb.scanCode != SCAN_CODE || kb.flags & LLKHF_INJECTED != 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let down = match wparam as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => true,
        WM_KEYUP | WM_SYSKEYUP => false,
        _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) },
    };
    // SAFETY: GetAsyncKeyState only reads key state.
    let held = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    let modifiers = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].into_iter().any(held);
    let action = KEYS.lock().unwrap().key(down, modifiers, ENABLED.load(Ordering::Relaxed));
    match action {
        Action::Pass => return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) },
        Action::Arm(press) => {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(HOLD_MS));
                if KEYS.lock().unwrap().held(press) {
                    if let Some((start, _)) = HANDLERS.get() {
                        start();
                    }
                }
            });
        }
        Action::Swallow => {}
        Action::Tap => type_key(),
        Action::Finish => {
            if let Some((_, finish)) = HANDLERS.get() {
                finish();
            }
        }
    }
    1 // swallowed
}

/// Re-types the key by its scan code, so the active layout decides the character.
#[cfg(windows)]
fn type_key() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    };
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: SCAN_CODE as u16, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(KEYEVENTF_SCANCODE), key(KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP)];
    // SAFETY: two well-formed keyboard inputs; Windows copies them.
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_types_hold_talks() {
        let mut k = Keys::default();
        // A quick tap: armed, released before the timer, so the character is typed.
        assert_eq!(k.key(true, false, true), Action::Arm(1));
        assert_eq!(k.key(false, false, true), Action::Tap);
        assert!(!k.held(1)); // the timer for that tap finds nothing held

        // A hold: armed, timer fires while held, repeats swallowed, release finishes.
        assert_eq!(k.key(true, false, true), Action::Arm(2));
        assert!(k.held(2));
        assert_eq!(k.key(true, false, true), Action::Swallow);
        assert_eq!(k.key(false, false, true), Action::Finish);

        // A stale timer from an older press never starts listening.
        assert_eq!(k.key(true, false, true), Action::Arm(3));
        assert!(!k.held(2));
        assert_eq!(k.key(false, false, true), Action::Tap);

        // Shift+` (~), Ctrl+` and a disabled setting pass straight through.
        assert_eq!(k.key(true, true, true), Action::Pass);
        assert_eq!(k.key(false, true, true), Action::Pass);
        assert_eq!(k.key(true, false, false), Action::Pass);
    }
}
