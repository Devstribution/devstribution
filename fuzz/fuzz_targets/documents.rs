//! The small documents a package or a user brings: the signed file list, signatures, keys, a
//! publisher's key file, key statements and revocation lists.
#![no_main]

use std::sync::OnceLock;

use devstribution_core::files::FilesList;
use devstribution_core::paths::validate_relative_path;
use devstribution_core::revocation::{self, RevocationList};
use devstribution_core::signing::{
    Signature, Signer, TrustedComment, generate_keys, parse_public_key,
};
use devstribution_core::trust::TrustedAuthor;
use libfuzzer_sys::fuzz_target;

fn signer(cell: &'static OnceLock<Signer>) -> &'static Signer {
    cell.get_or_init(|| {
        let keys = generate_keys("fuzz", None).unwrap();
        Signer::from_unencrypted(&keys.secret_key).unwrap().unwrap()
    })
}

fuzz_target!(|data: &[u8]| {
    // What a file list accepts it prints the one way it accepts: the signature covers those bytes.
    if let Ok(list) = FilesList::parse(data) {
        assert_eq!(list.to_canonical_string().as_bytes(), data);
    }

    // A list that reads is signed and checked as a launcher would.
    if let Ok(list) = serde_json::from_slice::<RevocationList>(data) {
        static ROOT: OnceLock<Signer> = OnceLock::new();
        static LIST_KEY: OnceLock<Signer> = OnceLock::new();
        let (root, key) = (signer(&ROOT), signer(&LIST_KEY));
        let files = revocation::sign_list(root, key, "2099-12-31", &list).unwrap();
        if let Ok(verified) = files.verify(std::slice::from_ref(root.public_key()), "2026-10-30") {
            assert_eq!(verified, list);
            let _ = verified.expired("2026-10-30");
            let _ = verified.find("key", "content");
        }
    }

    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = validate_relative_path(text);
    let _ = parse_public_key(text);
    let _ = revocation::read_statement(text);
    if let Ok(signature) = Signature::parse(text) {
        let _ = signature.unverified_comment();
    }
    let _ = TrustedComment::parse(text);
    if let Ok(author) = TrustedAuthor::parse(text) {
        TrustedAuthor::parse(&author.to_file()).expect("what it writes it reads");
    }
});
