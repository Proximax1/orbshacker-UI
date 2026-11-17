use std::path::Path;

pub fn remaining_secs(started_unix: u64, now_unix: u64, minutes: u32) -> u64 {
    let total = minutes as u64 * 60;
    total.saturating_sub(now_unix.saturating_sub(started_unix))
}

pub fn format_clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn win(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\")
}

pub fn self_destruct_command(exe: &Path, manifest: Option<&Path>, dir: &Path) -> String {
    let mut parts = vec![
        "ping 127.0.0.1 -n 3 > nul".to_string(),
        format!("del /f /q \"{}\"", win(exe)),
    ];
    if let Some(m) = manifest {
        parts.push(format!("del /f /q \"{}\"", win(m)));
    }
    parts.push(format!("rmdir \"{}\"", win(dir)));
    parts.join(" && ")
}

#[cfg(windows)]
pub fn spawn_self_destruct(cmd: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("cmd")
        .raw_arg("/C")
        .raw_arg(format!("\"{cmd}\""))
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
pub fn spawn_self_destruct(_cmd: &str) -> std::io::Result<()> {
    Ok(())
}

pub fn run_fake_game(cfg: &crate::baked::Baked, exe: &Path) {
    let total = std::time::Duration::from_secs(cfg.timer_minutes as u64 * 60);
    let title = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if !crate::window::run_game_window(&title, total) {
        let end = std::time::Instant::now() + total;
        loop {
            let now = std::time::Instant::now();
            if now >= end {
                break;
            }
            std::thread::sleep((end - now).min(std::time::Duration::from_secs(1)));
        }
    }
    if cfg.auto_delete {
        let dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let manifest = cfg.steam_manifest_path.as_deref().map(Path::new);
        let _ = spawn_self_destruct(&self_destruct_command(exe, manifest, dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn remaining_counts_down_and_stops_at_zero() {
        assert_eq!(remaining_secs(100, 100, 15), 900);
        assert_eq!(remaining_secs(100, 160, 15), 840);
        assert_eq!(remaining_secs(100, 5000, 15), 0);
    }

    #[test]
    fn remaining_survives_clock_going_backwards() {
        assert_eq!(remaining_secs(500, 400, 1), 60);
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(0), "00:00");
        assert_eq!(format_clock(65), "01:05");
        assert_eq!(format_clock(900), "15:00");
        assert_eq!(format_clock(3725), "1:02:05");
    }

    #[test]
    fn self_destruct_without_manifest() {
        let c = self_destruct_command(
            &PathBuf::from("C:/Users/x/Desktop/Win64/game.exe"),
            None,
            &PathBuf::from("C:/Users/x/Desktop/Win64"),
        );
        assert_eq!(
            c,
            "ping 127.0.0.1 -n 3 > nul && del /f /q \"C:\\Users\\x\\Desktop\\Win64\\game.exe\" && rmdir \"C:\\Users\\x\\Desktop\\Win64\""
        );
    }

    #[test]
    fn self_destruct_with_manifest_deletes_it_before_rmdir() {
        let c = self_destruct_command(
            &PathBuf::from("C:/S/steamapps/common/G/g.exe"),
            Some(&PathBuf::from("C:/S/steamapps/appmanifest_1.acf")),
            &PathBuf::from("C:/S/steamapps/common/G"),
        );
        let del_m = c.find("appmanifest_1.acf").unwrap();
        let rmdir = c.find("rmdir").unwrap();
        assert!(del_m < rmdir);
    }
}
