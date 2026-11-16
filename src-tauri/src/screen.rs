//! "What's on my screen?": a screenshot of the main display, for an agent to look at.

use std::{path::Path, process::Command};

/// Whether a request is about what's on screen ("what's on my screen", "read my screen", "summarize this page").
pub fn asks_about_screen(input: &str) -> bool {
    let s = input.trim().to_lowercase();
    [
        "my screen", "the screen", "this screen", "on screen", "what am i looking at", "what's this", "whats this",
        "summarize this", "summarise this", "explain this", "read this", "what does this say", "this page", "this window",
    ]
    .iter()
    .any(|p| s.contains(p))
}

/// Saves a PNG of the primary display to `path`. Runs DPI-aware so a scaled display is captured whole.
pub fn capture(path: &Path) -> Result<(), String> {
    // No double quotes in here: they don't survive being passed to powershell.exe as an argument.
    let script = r#"
$q = [char]34
Add-Type -MemberDefinition ('[DllImport(' + $q + 'user32.dll' + $q + ')] public static extern bool SetProcessDPIAware();') -Name Dpi -Namespace Engine
[Engine.Dpi]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save($env:ENGINE_SHOT, [System.Drawing.Imaging.ImageFormat]::Png)
'ok'
"#;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]).env("ENGINE_SHOT", path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("Couldn't take a screenshot: {e}"))?;
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("Couldn't take a screenshot: {}", String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("unknown error")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_screen_questions() {
        assert!(asks_about_screen("What's on my screen?"));
        assert!(asks_about_screen("summarize this page"));
        assert!(!asks_about_screen("what's the weather"));
    }
}
