# Security policy

This repository is a personal fork of Stalwart that adds zero-access
(encrypted-at-rest) calendar accounts. It has no releases yet, so there are
no supported versions.

## Reporting a vulnerability

<<<<<<< HEAD
Report vulnerabilities privately through GitHub: open the repository's
**Security** tab and choose **Report a vulnerability**
(<https://github.com/jaylaney/stalwart/security/advisories/new>). Please do
not open a public issue.
=======
| Version | Supported          | End of Support |
| ------- | ------------------ | -------------- |
| 1.0.x   | :white_check_mark: | TBD            |
| 0.16.x  | :white_check_mark: | 2027-06-01     |
| 0.15.x  | :white_check_mark: | 2026-12-01     |
| < 0.14  | :x:                | Ended          |
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184

Reports about the fork's zero-access code are in scope: the `vault` crate,
the `/api/vault` endpoints, sealing, key-account gating and the related
tests. A vulnerability in code this fork inherits unchanged from upstream
Stalwart should be reported to Stalwart Labs through their own process at
<https://stalw.art>; this fork does not forward reports upstream.

There is no bug bounty.
