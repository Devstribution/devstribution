# Code signing policy

**Not in effect yet.** devstribution's releases are not code-signed today; this is the policy
signed releases will follow, published ahead so the signing application can point at it (see
[`roadmap.md`](roadmap.md), A9 and B4).

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

## What gets signed

Only files built by this repository's release workflow
([`.github/workflows/release.yml`](../.github/workflows/release.yml)) from a version tag on `main`:
the Windows installer, the launcher inside it and the `devstribution` CLI. Nothing built on a PC
is signed, and nothing from outside this repository: the `uv` the installer ships is Astral's
program, pinned and checked against `launcher/uv-pin.json`, and shipped as Astral released it;
apps packaged as `.dsapp` are signed by their own authors with their own keys
([`security.md`](security.md)), never with this certificate.

How a release is made: [`releasing.md`](releasing.md).

## Team roles

| Role | Who | What they do |
|---|---|---|
| Committers and reviewers | [Members of the Devstribution organisation](https://github.com/orgs/Devstribution/people) with write access | Change the code; every change goes through a pull request |
| Approvers | [Owners of the Devstribution organisation](https://github.com/orgs/Devstribution/people?query=role%3Aowner) | Tag a release and approve each signing request on SignPath |

Today all three roles are [@edo-13](https://github.com/edo-13). Everyone with write access uses
two-factor authentication.

A signing request is approved only for a tag whose release workflow ran from `main` after CI
passed, and only after the draft release has been read as [`releasing.md`](releasing.md) says.

## Privacy

devstribution itself collects nothing and sends nothing about you or your apps anywhere: no
telemetry, no crash reports, no account. Its logs and the "Copy diagnostics" text stay on the PC
until you choose to send them to someone.

It connects to other systems only to do what you asked:

- **Installing an app** you chose: `uv` downloads the Python version the app needs from
  [python-build-standalone](https://github.com/astral-sh/python-build-standalone) releases on
  GitHub, and the app's dependencies from [PyPI](https://pypi.org) (`pypi.org`,
  `files.pythonhosted.org`). Those services see the request like any download: see
  [GitHub's](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)
  and the [Python Software Foundation's](https://policies.python.org/pypi.org/Privacy-Notice/)
  privacy policies.
- **Running an app** you installed: the app is the author's program and may use the network as
  its package declares. The runtime guard blocks network use it did not declare
  ([`security.md`](security.md)). What the app sends is up to its author, not devstribution.
- **Looking for a newer devstribution**, when it starts and from Settings: one request to
  `github.com` for this repository's `latest.json` (the newest version and its notes), and, if you
  choose to update, the installer from the release. Nothing about you or your apps is sent.
  Settings → "Look for updates when devstribution starts" turns the check at start off; IT can
  turn updates off entirely ([`managed-mode.md`](managed-mode.md), `LauncherUpdates`).
- **Later** (not built yet): checking the list of revoked publisher keys. When that comes, this
  section will say where it connects.

In managed mode, the IT department's policy can limit which authors' apps run
([`managed-mode.md`](managed-mode.md)); it sends nothing to IT either.
