# Frozen format-1 package

`hello.dsapp` was packed with `devstribution pack` when package format 1 was frozen
(see `docs/format.md`). `publisher.pub` is the public key it was signed with; the secret key was
thrown away, so these bytes can never be re-signed or regenerated.

`tests/format_v1.rs` checks that this version still accepts it exactly as it did then. If that
test fails, the change broke format 1 for packages already handed out: fix the change, never the
fixture.
