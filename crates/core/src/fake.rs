use crate::baked::{self, Baked};
use crate::error::{Error, Result};
use crate::paths::sanitize_relative_path;
use crate::settings::Settings;
use std::path::{Component, Path, PathBuf};

pub fn normalize_exe_name(name: &str) -> String {
    let mut n = sanitize_relative_path(name.trim());
    if !n.to_lowercase().ends_with(".exe") {
        n.push_str(".exe");
    }
    n
}

pub fn target_for(settings: &Settings, exe_name: &str) -> PathBuf {
    let mut p = settings.fake_dir();
    for seg in normalize_exe_name(exe_name).split('/') {
        p.push(seg);
    }
    p
}

fn lexical(p: &Path) -> String {
    p.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
        .replace('\\', "/")
}

pub fn limit_dir_for(target: &Path, chosen_folder: &Path) -> PathBuf {
    let s = target.to_string_lossy().replace('\\', "/");
    let needle = "steamapps/common";
    if let Some(idx) = s.to_lowercase().find(needle) {
        return PathBuf::from(&s[..idx + needle.len()]);
    }
    chosen_folder.to_path_buf()
}

pub fn dirs_to_clean(target: &Path, limit: &Path) -> Vec<PathBuf> {
    let limit_key = lexical(limit);
    let mut dirs = Vec::new();
    let mut cur = target.parent();
    while let Some(d) = cur {
        if lexical(d) == limit_key || d.parent().is_none() || d.as_os_str().is_empty() {
            break;
        }
        dirs.push(d.to_path_buf());
        cur = d.parent();
    }
    dirs
}

pub fn ensure_safe_to_write(target: &Path) -> Result<()> {
    if target.exists() && baked::read_from_exe(target).is_none() {
        return Err(Error::Invalid(format!(
            "{} already exists and is not an orbshacker fake. It was left untouched.",
            target.display()
        )));
    }
    Ok(())
}

pub fn create_fake(source_exe: &Path, target: &Path, cfg: &Baked) -> Result<()> {
    ensure_safe_to_write(target)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source_exe, target)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(target)?;
    f.write_all(&baked::encode(cfg))?;
    Ok(())
}

pub fn baked_for(settings: &Settings, manifest: Option<&Path>) -> Baked {
    Baked {
        chosen_folder: settings.chosen_folder.to_string_lossy().replace('\\', "/"),
        auto_delete: settings.auto_delete,
        timer_minutes: settings.timer_minutes,
        steam_manifest_path: manifest.map(|m| m.to_string_lossy().replace('\\', "/")),
    }
}

pub fn launch(exe: &Path) -> std::io::Result<std::process::Child> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_gets_extension_and_is_sanitised() {
        assert_eq!(normalize_exe_name("TslGame"), "TslGame.exe");
        assert_eq!(normalize_exe_name("Bin\\Game.EXE"), "Bin/Game.EXE");
        assert_eq!(normalize_exe_name("..\\..\\x"), "x.exe");
        assert_eq!(normalize_exe_name("  "), "unnamed.exe");
    }

    #[test]
    fn target_stays_inside_the_fake_dir() {
        let s = Settings {
            chosen_folder: PathBuf::from("C:/Users/x/Desktop"),
            fake_exe_dir: "Win64".into(),
            ..Settings::default()
        };
        let t = target_for(&s, "..\\..\\evil.exe");
        assert!(lexical(&t).ends_with("desktop/win64/evil.exe"));
    }

    #[test]
    fn limit_dir_is_steamapps_common_for_steam_targets() {
        let t = Path::new("C:/Program Files (x86)/Steam/steamapps/common/Game/Bin/g.exe");
        assert_eq!(
            limit_dir_for(t, Path::new("C:/Desktop")),
            PathBuf::from("C:/Program Files (x86)/Steam/steamapps/common")
        );
        assert_eq!(
            limit_dir_for(Path::new("C:/Desktop/Win64/g.exe"), Path::new("C:/Desktop")),
            PathBuf::from("C:/Desktop")
        );
    }

    #[test]
    fn dirs_to_clean_are_deepest_first_and_stop_at_limit() {
        let t = Path::new("C:/D/Win64/Bin/g.exe");
        let d = dirs_to_clean(t, Path::new("C:/D"));
        assert_eq!(d, vec![PathBuf::from("C:/D/Win64/Bin"), PathBuf::from("C:/D/Win64")]);
    }

    #[test]
    fn dirs_to_clean_terminates_when_limit_is_not_an_ancestor() {
        let d = dirs_to_clean(Path::new("C:/A/B/g.exe"), Path::new("C:/Elsewhere"));
        assert!(d.len() <= 3);
    }

    #[test]
    fn create_fake_copies_and_bakes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.exe");
        std::fs::write(&src, vec![5u8; 1000]).unwrap();
        let target = dir.path().join("out").join("Game.exe");
        let cfg = Baked { chosen_folder: "x".into(), auto_delete: true, timer_minutes: 9, steam_manifest_path: None };
        create_fake(&src, &target, &cfg).unwrap();
        assert_eq!(baked::read_from_exe(&target), Some(cfg.clone()));
        create_fake(&src, &target, &cfg).unwrap();
    }

    #[test]
    fn create_fake_refuses_to_overwrite_a_foreign_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.exe");
        std::fs::write(&src, b"orb").unwrap();
        let real = dir.path().join("RealGame.exe");
        std::fs::write(&real, b"the user's real game").unwrap();
        let cfg = Baked { chosen_folder: "x".into(), auto_delete: false, timer_minutes: 1, steam_manifest_path: None };
        assert!(matches!(create_fake(&src, &real, &cfg), Err(Error::Invalid(_))));
        assert_eq!(std::fs::read(&real).unwrap(), b"the user's real game");
    }
}
