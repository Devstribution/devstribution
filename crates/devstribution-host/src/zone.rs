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
pub fn origin_of(path: &Path) -> Option<Origin> {
    if !cfg!(windows) {
        return None;
    }
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let text = std::fs::read_to_string(stream).ok()?;
    parse(&text)
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
    }
}
