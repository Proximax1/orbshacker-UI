use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const MARKER: &[u8] = b"__ORBSHACKER_BAKED_CONFIG__";
const TAIL_BYTES: u64 = 65536;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baked {
    #[serde(rename = "CHOSEN_FOLDER", default)]
    pub chosen_folder: String,
    #[serde(rename = "AUTO_DELETE", default)]
    pub auto_delete: bool,
    #[serde(rename = "TIMER_MINUTES", default = "default_minutes")]
    pub timer_minutes: u32,
    #[serde(rename = "STEAM_MANIFEST_PATH", default, skip_serializing_if = "Option::is_none")]
    pub steam_manifest_path: Option<String>,
}

fn default_minutes() -> u32 {
    15
}

pub fn encode(cfg: &Baked) -> Vec<u8> {
    let json = serde_json::to_vec(cfg).expect("Baked always serialises");
    let mut out = Vec::with_capacity(json.len() + 2 * MARKER.len());
    out.extend_from_slice(MARKER);
    out.extend_from_slice(&json);
    out.extend_from_slice(MARKER);
    out
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut hits = Vec::new();
    if needle.is_empty() || haystack.len() < needle.len() {
        return hits;
    }
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            hits.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    hits
}

pub fn decode_tail(chunk: &[u8]) -> Option<Baked> {
    if !chunk.ends_with(MARKER) {
        return None;
    }
    let hits = find_all(chunk, MARKER);
    if hits.len() < 2 {
        return None;
    }
    let last = hits[hits.len() - 1];
    let prev = hits[hits.len() - 2];
    let json = &chunk[prev + MARKER.len()..last];
    serde_json::from_slice(json).ok()
}

pub fn read_from_exe(path: &Path) -> Option<Baked> {
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.seek(SeekFrom::End(0)).ok()?;
    let read = size.min(TAIL_BYTES);
    f.seek(SeekFrom::Start(size - read)).ok()?;
    let mut buf = Vec::with_capacity(read as usize);
    f.read_to_end(&mut buf).ok()?;
    decode_tail(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Baked {
        Baked {
            chosen_folder: "C:/Users/x/Desktop".into(),
            auto_delete: true,
            timer_minutes: 20,
            steam_manifest_path: Some("C:/Steam/steamapps/appmanifest_1.acf".into()),
        }
    }

    #[test]
    fn round_trip_through_a_fake_exe_tail() {
        let mut exe = vec![0u8; 5000];
        exe.extend(encode(&sample()));
        assert_eq!(decode_tail(&exe), Some(sample()));
    }

    #[test]
    fn plain_exe_has_no_config() {
        assert_eq!(decode_tail(&vec![7u8; 1000]), None);
    }

    #[test]
    fn marker_text_inside_a_plain_exe_is_not_a_config() {
        let mut exe = vec![1u8; 100];
        exe.extend_from_slice(MARKER);
        exe.extend(vec![2u8; 100]);
        assert_eq!(decode_tail(&exe), None);
    }

    #[test]
    fn corrupt_json_is_rejected() {
        let mut exe = Vec::new();
        exe.extend_from_slice(MARKER);
        exe.extend_from_slice(b"{broken");
        exe.extend_from_slice(MARKER);
        assert_eq!(decode_tail(&exe), None);
    }

    #[test]
    fn reads_what_python_writes() {
        let mut exe = vec![0u8; 10];
        exe.extend_from_slice(MARKER);
        exe.extend_from_slice(
            br#"{"CHOSEN_FOLDER": "C:/Users/x/Desktop", "AUTO_DELETE": false, "TIMER_MINUTES": 15}"#,
        );
        exe.extend_from_slice(MARKER);
        let b = decode_tail(&exe).unwrap();
        assert_eq!(b.timer_minutes, 15);
        assert!(!b.auto_delete);
        assert_eq!(b.steam_manifest_path, None);
    }

    #[test]
    fn missing_keys_use_defaults() {
        let mut exe = Vec::new();
        exe.extend_from_slice(MARKER);
        exe.extend_from_slice(b"{}");
        exe.extend_from_slice(MARKER);
        let b = decode_tail(&exe).unwrap();
        assert_eq!(b.timer_minutes, 15);
    }

    #[test]
    fn read_from_exe_uses_only_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.exe");
        let mut bytes = vec![9u8; 200_000];
        bytes.extend(encode(&sample()));
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_from_exe(&p), Some(sample()));
    }
}
