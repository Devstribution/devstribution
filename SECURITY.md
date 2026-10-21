# Security policy

devstribution verifies and runs code on people's computers, so security reports are very welcome.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
**Security → Report a vulnerability** on this repository
([private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)).

Include what you found, how to reproduce it and what an attacker could do with it. We will
acknowledge the report, keep you informed and credit you in the advisory unless you prefer not.

## Scope

The project's promise is that what a user runs is exactly what a verified publisher published,
and that an app cannot damage the platform or other apps (see `docs/plan.md`, "Principles").
Anything that breaks it is in scope, for example:

- a package that passes verification although its files, manifest or signature were changed;
- extraction writing outside the target folder, or links/special files getting through;
- a signature accepted for the wrong app id, version or key;
- an app reaching another app's files, secrets or environment through the platform;
- code running at install time.

The project is pre-alpha: there is no supported release yet, so reports apply to `main`.
