//! Mark-of-the-Web: where a downloaded file came from, from its `Zone.Identifier` stream.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Origin {
    /// 3 = internet, 4 = restricted; 0-2 are local, intranet and trusted sites.
    pub zone_id: u32,
    pub host_url: Option<String>,
    pub referrer_url: Option<String>,
}

impl Origin {
    pub fn is_internet(&self) -> bool {
        self.zone_id >= 3
    }
}

/// Reads the mark of a file. Always `None` outside Windows (other systems have no such stream).
///
/// A mark that is there but cannot be understood counts as "from the internet": a warning too
/// many is better than a missing one.
pub fn origin_of(path: &Path) -> Option<Origin> {
    if !cfg!(windows) {
        return None;
    }
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let data = std::fs::read(stream).ok()?;
    Some(parse(&decode(&data)).unwrap_or(Origin {
        zone_id: 3,
        host_url: None,
        referrer_url: None,
    }))
}

/// The stream is normally ANSI/UTF-8 text, but some tools write it in UTF-16.
fn decode(data: &[u8]) -> String {
    let utf16 = |bytes: &[u8], le: bool| {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match data {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

pub fn parse(text: &str) -> Option<Origin> {
    let mut origin = Origin {
        zone_id: 0,
        host_url: None,
        referrer_url: None,
    };
    let mut found = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "ZoneId" => {
                origin.zone_id = value.parse().ok()?;
                found = true;
            }
            "HostUrl" if !value.is_empty() => origin.host_url = Some(value),
            "ReferrerUrl" if !value.is_empty() => origin.referrer_url = Some(value),
            _ => {}
        }
    }
    found.then_some(origin)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_zone_identifier() {
        let o = super::parse(
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://github.com/a/b\r\nHostUrl=https://objects.githubusercontent.com/x.dsapp\r\n",
        )
        .unwrap();
        assert!(o.is_internet());
        assert_eq!(
            o.host_url.as_deref(),
            Some("https://objects.githubusercontent.com/x.dsapp")
        );
        assert!(super::parse("[ZoneTransfer]\n").is_none());
        assert!(super::parse("[ZoneTransfer]\nZoneId=x\n").is_none());
    }

    #[test]
    fn reads_utf16_marks() {
        let text = "[ZoneTransfer]\r\nZoneId=3\r\n";
        let mut data = vec![0xFF, 0xFE];
        data.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(super::parse(&super::decode(&data)).unwrap().zone_id, 3);
    }
}
