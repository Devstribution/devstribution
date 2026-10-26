# Revocation: design

How a publisher key that should no longer be trusted (lost, stolen, or used to sign malware) stops
being trusted on every PC, without devstribution becoming a registry of publishers. Status:
**design, agreed October 2026, not built.** It replaces the "public revocation list" idea under
"Possibilities" in [`roadmap.md`](roadmap.md). Where [`plan.md`](plan.md) differs (the catalog
key signing the list), this page wins.

## Decisions

1. **Devstribution holds no publisher keys.** Publishers make and keep their own keys, as today.
   The list names only *bad* keys (and bad packages) by fingerprint. A registry of every good key
   would make us an authority that checks identities; that is the Phase 2 catalog, not this.
2. **The list is advice, and whoever runs the PC decides.** In free mode the launcher shows what
   was reported and the user decides, as with the code scan's red banner. In managed mode IT
   decides by policy, and can turn our list off or replace it with its own.
3. **Nothing goes on the list without evidence.** Reports are checked first. An entry describes
   what a *key* signed, never what a person is: most stolen keys belong to a victim.
4. **The owner's word needs no judgement.** A publisher who withdraws their own key, signed with
   that key, is listed as soon as the statement is checked, and the launcher can act on the
   statement directly without waiting for the list.
5. **Signed offline, hosted anywhere.** GitHub (`Devstribution/trust`, served by GitHub Pages)
   only carries the file. Whoever controls the repository or the network can hold the list back
   or replay an old one, which the serial number and the expiry date reveal, but cannot add or
   remove an entry.

## What an entry says

| Status | Who says so | Shown as | Free mode | Managed mode (default) |
|---|---|---|---|---|
| `withdrawn` | The key's owner, in a statement signed with that key | "The publisher says this key is no longer safe (lost / stolen / retired) since <date>." | Installs and updates signed with it are refused: the owner asked. Installed apps: an amber warning before the next run, once per app. | Refused, installed apps stop running |
| `malicious` | Us, with evidence | "A package signed with this key <did what>. <link>" | Red banner and "Install anyway". Installed apps: runs paused until the user confirms once, with the evidence shown | Refused, installed apps stop running |
| `cleared` | Us, after an appeal or a mistake | "Listed on <date>, cleared on <date>: <why>." | Nothing (kept for the record) | Nothing |

An entry names a key, or only some packages (by `content_sha256`), for a publisher whose key is
fine but who shipped one bad version. Unverified reports are never published: there is no "under
review" status.

**Dates prove nothing about signatures.** A minisign signature does not say when it was made (the
trusted comment says whatever the signer wrote), so "stolen since March" cannot clear packages
signed before March: a thief can backdate. Every package signed with a listed key is treated alike.
The way back for a publisher is to sign the good releases again with a new key.

## Files

`Devstribution/trust` holds:

| File | What it is |
|---|---|
| `root.pub` | The two root public keys, for people to compare with the ones built into the launcher. The launcher never reads it. |
| `list-key.json` + `.minisig` | The key that signs lists, certified by a root key, with an end date. |
| `revoked.json` + `.minisig` | The list, signed by the list key. |
| `statements/` | The owners' signed withdrawal and rotation statements, as received. |
| `evidence/` | What each `malicious` entry rests on: package hashes, analysis, the publisher's reply. |

```json
{
  "format": 1,
  "serial": 12,
  "issued": "2026-10-26",
  "expires": "2026-11-25",
  "entries": [
    {
      "id": "2026-0003",
      "status": "malicious",
      "key": "9cf3…(SHA-256 fingerprint of the full key)",
      "packages": [],
      "namespace": "acme-ops",
      "reason": "Version 1.4.0 of acme-ops/kpi-digest uploads browser passwords to 203.0.113.7.",
      "evidence": ["https://devstribution.github.io/trust/evidence/2026-0003.html"],
      "publisher_statement": null,
      "replacement": null,
      "listed": "2026-10-21",
      "updated": "2026-10-21"
    }
  ]
}
```

- `packages` empty means the whole key; otherwise only those `content_sha256` values.
- `namespace` is shown to people. The launcher matches on `key` (and `packages`) only.
- `replacement`, for a withdrawal or rotation: the owner's new key, signed by the old one.
- Entries are never deleted: a mistake becomes `cleared`, and git keeps the history.

### Keys

- **Two root keys** are built into the launcher, a primary and a backup kept apart, offline (hardware
  keys). Either can certify a new list key, so a lost or leaked list key is replaced without a new
  launcher. A root key that leaks is retired by the other in the next launcher release.
- **The list key** signs lists. It is offline too, never in CI secrets, and certified for at most
  one year at a time.
- Later: two of three maintainers sign each list, so one person cannot list or clear a key alone.
- None of these keys sign apps or the launcher.

## Owners' statements

A publisher handles their own key without us:

- **Withdrawal** (`devstribution keys withdraw`): a small file stating the key, a reason (`lost`,
  `stolen`, `retired`) and optionally the new key, signed with the key being withdrawn. Anyone who
  holds the key can sign it, a thief included. That is fine: a thief can only switch the key
  off, never back on.
- **Rotation** (`devstribution keys rotate`): the old key names the new one and the new key signs
  back. Launchers that pinned the old key accept the next package signed with the new one, with a
  notice instead of the "new publisher" warning.

The launcher accepts a statement opened directly (double-click, or IT puts it on the policy share)
and acts on it at once. We copy received statements into `statements/` and list them as
`withdrawn`, after checking only the signature.

## How the list reaches a PC

The list is checked by the launcher, never by apps. It arrives in four ways:

1. **Bundled** with every launcher release. This works offline, and Settings shows "revocations
   known up to <date>".
2. **Fetched**, in free mode, by default at most once a day, when the launcher starts or before an
   install. It is a plain HTTPS GET of `revoked.json` and its signature: no identifiers or app
   names, the same request from every launcher, the system proxy, a short timeout. A failure is
   silent, and the newest list already held applies.
3. **IT's copy**: policy points at a share or intranet address, or turns fetching off.
4. **Imported by hand**: `devstribution revocations import <file>`, or dropping the file on the
   launcher.

**What the launcher accepts:**
- a signature by a list key certified by a built-in root, within that certificate's end date;
- a `serial` higher than the one it holds; an equal serial is ignored and a lower one refused, so
  a replayed old list changes nothing.

**When the list expires:**
- **Free mode:** Settings says so, with nothing in the way.
- **Managed mode:** it installs nothing new until a fresh list arrives (`RevocationMaxAge`, which
  IT can turn off for air-gapped sites). Installed apps keep running.

**Where it is checked** (always locally): at inspect, again at install, and before every run,
against the key and `content_sha256` recorded for the installed app.

## Policy (managed mode)

| Value | Meaning | Default |
|---|---|---|
| `RevocationList` | `default` (our GitHub list), a path or URL to a copy, or `off` | `default` |
| `RevocationAction` | `follow` (the free-mode behaviour above), `block` (refuse everything listed, no override) | `block` |
| `RevokedKeys` | IT's own fingerprints, always refused, whatever the list says | none |
| `RevocationMaxAge` | Days after `expires` before installs stop; `0` turns the check off | 14 |

IT's approved-authors list already revokes on its own: removing an author stops its apps at the
next run. Our list is the alert that tells IT to do so, and enforces it where IT chose `follow` or
`block`.

## Process

1. **Report:** privately, through SECURITY.md or a private GitHub form, with the package or its
   hash.
2. **Check:** we reproduce what the package does and keep the analysis.
3. **Contact:** we write to the publisher (the `.trust` contact, the namespace's GitHub owner) and
   allow a few days for a reply. When the package is actively spreading, it is listed at once and
   the reply is added later.
4. **Publish** with the evidence, worded as what the signed package did. Two maintainers approve
   it, then the list key signs offline.
5. **Appeal:** through the same channel. A mistake becomes `cleared`, with the reason, and the
   record stays.

## Building it, in order

1. **The list check in the launcher and CLI:** the format, the signature chain, serial and expiry,
   the bundled copy, `revocations import`, the banners. Nothing is fetched yet.
2. **Owners' statements:** `keys withdraw`, `keys rotate`, and the launcher accepting them.
3. **The `Devstribution/trust` repository:** root and list keys made offline, the first list
   empty at serial 1, the GitHub Pages address.
4. **Fetching and the policy values.**
5. **Later:** two-of-three signing, and the catalog's "known publisher" binding (Phase 2).

## Open questions

- **Carrying a rotation inside the next package.** It would spare users a separate file, but
  format 1 refuses unlisted files, so it waits for format 2.
- **How a paused app is shown in the app list.** A quarantine-like state seems right; the guard's
  quarantine already has the screens.
