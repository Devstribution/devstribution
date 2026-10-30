//! The manifest parser, and the small parsers inside it.
#![no_main]

use std::str::FromStr;

use devstribution_core::manifest::{AppId, Entry, Manifest, Version};
use libfuzzer_sys::fuzz_target;

/// What a value prints must read back as the same value: these strings are stored and compared.
fn round_trip<T: FromStr + ToString>(text: &str) {
    if let Ok(value) = T::from_str(text) {
        let printed = value.to_string();
        let again = T::from_str(&printed).ok().expect("what it prints parses");
        assert_eq!(again.to_string(), printed);
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(manifest) = Manifest::parse(text) {
        round_trip::<AppId>(&manifest.id.to_string());
        round_trip::<Version>(&manifest.version.to_string());
    }
    round_trip::<AppId>(text);
    round_trip::<Entry>(text);
    round_trip::<Version>(text);

    // Versions decide what counts as an update: their order has to be a real order.
    if let Some((a, b)) = text.split_once('\n')
        && let (Ok(a), Ok(b)) = (Version::from_str(a), Version::from_str(b))
    {
        assert_eq!(a.cmp(&b), b.cmp(&a).reverse());
        assert_eq!(a == b, a.cmp(&b).is_eq());
    }
});
