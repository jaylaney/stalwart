# Stalwart: personal zero-access fork

This is a fork of [Stalwart](https://github.com/stalwartlabs/stalwart) for personal
exploration. The goal is a version of Stalwart that offers server-side zero-access
encryption, starting with calendar data and compatibility with existing CalDAV clients.

Development is independent of the original project. There is no intention to open a pull
request against upstream or merge these changes into the original Stalwart branch.

## Goal and current scope

The fork explores encrypting private calendar content at rest with per-user keys unlocked
by the user's credentials. The current work focuses on:

- Zero-access accounts with password, recovery key, TOTP and app-password support.
- Encryption of private event, task and journal content, plus calendar names, descriptions, colours and custom time zones.
- Decryption during authenticated CalDAV requests so clients can use ordinary calendar data.
- Local invitations and scheduling between key accounts, with delegated `SENT-BY` delivery
  when an edit grantee schedules from the owner's calendar.
- View/edit calendar grants to another setup-complete key account on this server, using
  pinned recipient keys and the [vault sharing API](docs/zero-access/calendar-sharing-api.md).
  CalDAV access uses the grantee's own credentials: view grants read, edit grants also write.
- JMAP calendars, calendar re-sharing, free/busy-only grants, cross-server sharing and
  key-account mail sharing remain refused or deferred. Calendar full-text search stays gated.

Ordinary accounts continue to use Stalwart's existing behavior. The fork also encrypts
key-account mail at rest (release 3); contacts and files retain upstream storage behavior. Calendar sharing native-client qualification and fresh whole-branch
review remain pending; see the [outcome](docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md).

An account becomes zero-access through a setup token issued by an administrator, completed on
an account web page that lives in a separate repository and is not part of this one.

## What zero access means here

The design aims to make sealed calendar fields unreadable from stored data alone without
user credentials or an unlocked key. This includes database dumps, disks and backups.

Encryption and decryption happen on the server. The running server receives credentials and
handles plaintext while serving requests, and unlocked keys remain in a bounded in-memory
cache. A compromised running server can capture those credentials or keys, so this model does
not provide end-to-end encryption against the server itself.

The fork redacts authentication exchanges in mail-protocol raw traces and keeps key-account
IMAP/POP3 sessions redacted. Sharing request/response bodies and key material are not traced;
existing authentication account-name metadata remains. These trace controls do not protect
against a compromised running server. Logout alone does not promise immediate key erasure:
bounded cache residency and its hard cap still apply.

Removing a grantee stops fresh authorized access and rotates keys for future writes. Existing
events are not resealed, and material the grantee could already read may have been copied.
Cluster pubsub is required for prompt invalidation; a missed broadcast is not synchronous
eviction of already resident keys.

Some metadata remains visible, including event times, recurrence rules, identifiers, plaintext sizes,
filenames and sync history. The
[design and threat model](docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md)
describes the sealed fields, visible metadata, key lifetime and security boundaries in detail.

## Development

Build the fork from source with stable Rust:

```sh
cargo build --release -p stalwart --no-default-features --features rocks
```

On macOS with Homebrew's keg-only rustup, first run:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
```

RocksDB needs the Xcode command line tools on macOS.

The product build excludes enterprise features. Do not deploy builds with the `test_mode`
feature, which weakens password hashing for tests.

- [Developer notes](docs/superpowers/plans/README-dev.md): build, test and account setup details.
- [Manual client checklist](docs/zero-access/manual-checklist.md): CalDAV client verification.
- [Designs](docs/superpowers/specs/) and [plans and outcomes](docs/superpowers/plans/): scope,
  decisions and implementation records.

## Relationship to upstream

Stalwart provides the underlying mail and collaboration server. Credit for that work belongs
to Stalwart Labs and its contributors. The upstream website and documentation are at
[stalw.art](https://stalw.art).

This repository has its own experimental scope and development direction. Fork-specific bugs
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
