# Stalwart: zero-access email and calendars

A personal, experimental fork of [Stalwart](https://github.com/stalwartlabs/stalwart)
for self-hosted email and calendars, with server-side zero-access encryption for stored
mail and private calendar content. Use existing mail and CalDAV clients; no client
extensions are required.

## Current support

- **Email:** SMTP delivery and submission, with IMAP, POP3 and JMAP mail access. Stored
  messages and their private metadata are encrypted for zero-access accounts, including
  mail delivered while the user is logged out.
- **Calendars:** CalDAV events, tasks and journals, with private content and calendar
  names, descriptions, colours and custom time zones encrypted at rest.
- **Invitations by email:** invite people on other servers and receive their invitations,
  replies, updates and cancellations through ordinary invitation email (iMIP). Incoming
  invitations are processed at the next authenticated CalDAV sync, subject to the account's
  scheduling settings.
- **Local scheduling and sharing:** invitations between zero-access accounts on this server
  use sealed internal delivery. View/edit calendar grants use pinned recipient keys and the
  [sharing API](docs/zero-access/calendar-sharing-api.md); an editor can also schedule on
  the owner's behalf.
- **Account security:** passwords, recovery keys, TOTP and app passwords. An administrator
  issues a setup token; the user completes enrolment on a separate account web app.

Ordinary accounts retain upstream behavior. Contacts and files retain upstream storage
behavior and are outside this fork's zero-access encryption scope.

For zero-access accounts, server-side searches and sorts that need sealed mail content are
refused, and calendar full-text search stays gated. JMAP calendars, calendar re-sharing,
free/busy-only grants, cross-server calendar sharing and mail sharing remain deferred or
refused. Off-server invitations exchange event copies; they do not grant access to a calendar.

Native-client and real-provider qualification for email invitations remain pending; see the
[manual checklist](docs/zero-access/manual-checklist.md) for current client verification status.

## What zero access means here

Stored mailbox content and sealed calendar fields are designed to be unreadable from
database dumps, disks and backups without user credentials or an unlocked key.

Encryption and decryption happen on the server. The running server receives credentials and
handles plaintext while serving requests, and unlocked keys remain in a bounded in-memory
cache. A compromised running server can capture those credentials or keys, so this is not
end-to-end encryption against the server. Logout does not promise immediate key erasure.

**Mail in transit is outside this guarantee.** Ordinary email and emailed invitations exist
in plaintext in SMTP queues until delivery or relay, with queue blobs retained until purge.
Relays and external recipients can read their copies. SMTP logs record who sent mail to whom
and when. Invitations between local zero-access accounts use sealed delivery instead.

Some metadata remains visible, including sizes, event times, recurrence rules, identifiers,
standard event classifications, filenames and sync history. Revoking a calendar grant stops
fresh authorized access and rotates keys for future writes; it does not reseal existing
events or retract copies. Prompt cache invalidation across nodes requires cluster pubsub.

See the [calendar threat model](docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md),
[mail design](docs/superpowers/specs/2026-10-20-zero-access-mail-design.md) and
[email invitation design](docs/superpowers/specs/2026-11-02-zero-access-imip-design.md)
for sealed fields, visible metadata and security boundaries.

## Development

The fork is based on upstream Stalwart `v1.0.3` and has an independent release version,
recorded in [fork-release.toml](fork-release.toml). `stalwart --version` prints both
versions and the source commit. See the [fork changelog](CHANGELOG-fork.md) for releases.

Fresh stores only: a build on the 1.0 base cannot open a store written by upstream 0.16 or
by the fork's earlier 0.16-based releases (`0.1.x`), and there is no conversion. Those stores
are not supported. Moving an installation means a fresh store and new account setup (new
setup tokens and enrolment), keeping the old store and binary for rollback.

Build the fork from source with stable Rust:

```sh
cargo build --release -p stalwart --no-default-features --features rocks
```

The product build excludes enterprise features. Do not deploy builds with the `test_mode`
feature, which weakens password hashing for tests.

- [Developer guide](docs/README-dev.md): prerequisites, builds, tests, account setup and
  upstream integration.
- [Local server scripts](scripts/local-server/README.md): run a local development server.
- [Release policy](docs/releases.md): versioning, provenance and release checks.
- [Manual client checklist](docs/zero-access/manual-checklist.md): client and provider verification.
- [Designs](docs/superpowers/specs/) and [plans and outcomes](docs/superpowers/plans/): scope,
  decisions and implementation records.

## Relationship to upstream

Stalwart provides the underlying mail and collaboration server. Credit for that work belongs
to Stalwart Labs and its contributors. The upstream website and documentation are at
[stalw.art](https://stalw.art).

Development is independent, with no plan to submit these changes upstream. Fork-specific bugs
and questions belong here; upstream's support channels and roadmap do not cover this work.
The inherited [CONTRIBUTING.md](CONTRIBUTING.md) describes upstream's contribution policy,
not a requirement or plan to submit this fork upstream.

## Upstream funding acknowledgments

Upstream Stalwart acknowledges funding through:

- [NGI0 Entrust Fund](https://nlnet.nl/entrust), established by [NLnet](https://nlnet.nl/)
  with European Commission funding through the [Next Generation Internet](https://ngi.eu/)
  programme, under grant agreement No 101069594.
- [NGI Zero Core](https://nlnet.nl/NGI0/), established by [NLnet](https://nlnet.nl/)
  with European Commission funding, under grant agreement No 101092990.

## License

Upstream Stalwart is dual-licensed under the [GNU Affero General Public License v3.0](./LICENSES/AGPL-3.0-only.txt) and the [Stalwart Enterprise License v2](./LICENSES/LicenseRef-SEL.txt), and inherited files keep upstream's license notices. This fork offers no enterprise license: its own changes are published under AGPL-3.0 only, and code that is available only under the enterprise license (the `enterprise` feature and the `scim` crate) is excluded from the product build.

Each file carries a license notice at the top following the [REUSE guidelines](https://reuse.software/); the full text of each license is in the [LICENSES](./LICENSES/) directory.

## Copyright

Copyright (C) 2020, Stalwart Labs LLC

Changes in this fork: Copyright (C) 2026 Jay Laney
