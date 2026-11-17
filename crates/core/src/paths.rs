const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_illegal(c: char) -> bool {
    (c as u32) < 0x20
        || matches!(
            c,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '™' | '®' | '©' | '℠' | '℗'
        )
}

pub fn sanitize_filename(name: &str) -> String {
    if name.is_empty() {
        return "unnamed".into();
    }
    let stripped: String = name.chars().filter(|c| !is_illegal(*c)).collect();
    let cleaned = stripped
        .trim()
        .trim_end_matches(|c: char| c.is_whitespace() || c == '.')
        .to_string();
    if cleaned.is_empty() {
        return "unnamed".into();
    }
    let stem = cleaned.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("_{cleaned}");
    }
    cleaned
}

pub fn sanitize_path_segment(segment: &str) -> String {
    sanitize_filename(segment)
}

pub fn sanitize_relative_path(path: &str) -> String {
    if path.is_empty() {
        return "unnamed".into();
    }
    let normalized = path.replace('\\', "/");
    let without_drive = strip_drive(&normalized);
    let segments: Vec<String> = without_drive
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(sanitize_filename)
        .collect();
    if segments.is_empty() {
        "unnamed".into()
    } else {
        segments.join("/")
    }
}

fn strip_drive(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        s[2..].trim_start_matches(['/', '\\'])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_strips_illegal_and_trademarks() {
        assert_eq!(sanitize_filename("Game™: The *Sequel*?"), "Game The Sequel");
        assert_eq!(sanitize_filename("a\u{1}b"), "ab");
    }

    #[test]
    fn filename_trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("  name. . "), "name");
    }

    #[test]
    fn filename_empty_becomes_unnamed() {
        assert_eq!(sanitize_filename(""), "unnamed");
        assert_eq!(sanitize_filename("???"), "unnamed");
        assert_eq!(sanitize_filename(" . "), "unnamed");
    }

    #[test]
    fn filename_reserved_names_get_prefix() {
        assert_eq!(sanitize_filename("con"), "_con");
        assert_eq!(sanitize_filename("NUL.exe"), "_NUL.exe");
        assert_eq!(sanitize_filename("com1.txt"), "_com1.txt");
        assert_eq!(sanitize_filename("console"), "console");
    }

    #[test]
    fn relative_path_blocks_traversal_and_drives() {
        assert_eq!(sanitize_relative_path("..\\..\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_relative_path("C:\\Windows\\x.exe"), "Windows/x.exe");
        assert_eq!(sanitize_relative_path("c:///a/./b/../c.exe"), "a/b/c.exe");
        assert_eq!(sanitize_relative_path("Bin/Game.exe"), "Bin/Game.exe");
    }

    #[test]
    fn relative_path_degenerate_inputs() {
        assert_eq!(sanitize_relative_path(""), "unnamed");
        assert_eq!(sanitize_relative_path("../.."), "unnamed");
        assert_eq!(sanitize_relative_path("/"), "unnamed");
    }
}
