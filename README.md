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

<<<<<<< HEAD
## What zero access means here
=======
- **Email** server with complete protocol support:
  - JMAP: 
    * [JMAP for Mail](https://datatracker.ietf.org/doc/html/rfc8621) server.
    * [JMAP for Sieve Scripts](https://www.rfc-editor.org/rfc/rfc9661.html).
    * [JMAP Object Metadata](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04) for e-mails, mailboxes, calendars, contacts, files and Sieve scripts.
    * [WebSocket](https://datatracker.ietf.org/doc/html/rfc8887), [Blob Management](https://www.rfc-editor.org/rfc/rfc9404.html) and [Quotas](https://www.rfc-editor.org/rfc/rfc9425.html) extensions.
  - IMAP:
    * [IMAP4rev2](https://datatracker.ietf.org/doc/html/rfc9051) and [IMAP4rev1](https://datatracker.ietf.org/doc/html/rfc3501) server.
    * [ManageSieve](https://datatracker.ietf.org/doc/html/rfc5804) server.
    * Numerous [extensions](https://stalw.art/docs/development/rfcs#imap4-and-extensions) supported.
  - POP3:
    - [POP3](https://datatracker.ietf.org/doc/html/rfc1939) server.
    - [STLS](https://datatracker.ietf.org/doc/html/rfc2595) and [SASL](https://datatracker.ietf.org/doc/html/rfc5034) support as well as other [extensions](https://datatracker.ietf.org/doc/html/rfc2449).
  - SMTP:
    * SMTP server with built-in [DMARC](https://datatracker.ietf.org/doc/html/rfc7489), [DKIMv2](https://datatracker.ietf.org/doc/draft-ietf-dkim-dkim2-spec/) (including post-quantum signatures), [DKIMv1](https://datatracker.ietf.org/doc/html/rfc6376), [SPF](https://datatracker.ietf.org/doc/html/rfc7208) and [ARC](https://datatracker.ietf.org/doc/html/rfc8617) support for message authentication.
    * Strong transport security through [DANE](https://datatracker.ietf.org/doc/html/rfc6698), [MTA-STS](https://datatracker.ietf.org/doc/html/rfc8461) and [SMTP TLS](https://datatracker.ietf.org/doc/html/rfc8460) reporting.
    * Automated DKIM key rotation and management.
    * Inbound throttling and filtering with granular configuration rules, sieve scripting, MTA hooks and milter integration.
    * Distributed virtual queues with delayed delivery, priority delivery, quotas, routing rules and throttling support.
    * Envelope rewriting and message modification.
- **Collaboration** server:
  - Calendaring and scheduling:
    - [CalDAV](https://datatracker.ietf.org/doc/html/rfc4791) and [CalDAV Scheduling](https://datatracker.ietf.org/doc/html/rfc6638) support.
    - [JMAP for Calendars](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-calendars-32) support.
  - Contact management:
    - [CardDAV](https://datatracker.ietf.org/doc/html/rfc6352) support.
    - [JMAP for Contacts](https://datatracker.ietf.org/doc/html/rfc9610) support.
  - File storage:
    - [WebDAV](https://datatracker.ietf.org/doc/html/rfc4918) support.
    - [JMAP for File Storage](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-filenode-17) support.
  - Sharing with fine-grained access controls:
    - [WebDAV ACL](https://datatracker.ietf.org/doc/html/rfc3744) support.
    - [JMAP Sharing](https://datatracker.ietf.org/doc/html/rfc9670) support.
- **Spam** and **Phishing** built-in filter:
  - Comprehensive set of filtering **rules** on par with popular solutions.
  - AI spam classifier with support for OpenAI-compatible, Anthropic, Google Gemini and Ollama models.
  - Statistical **spam classifier** with collaborative filtering, automatic training capabilities and address book integration.
  - DNS Blocklists (**DNSBLs**) checking of IP addresses, domains, and hashes.
  - Collaborative digest-based spam filtering with **Pyzor**.
  - **Phishing** protection against homographic URL attacks, sender spoofing and other techniques.
  - Trusted **reply** tracking to recognize and prioritize genuine e-mail replies.
  - Sender **reputation** monitoring by IP address, ASN, domain and email address.
  - **Greylisting** to temporarily defer unknown senders.
  - **Spam traps** to set up decoy email addresses that catch and analyze spam.
- **Flexible**:
  - Pluggable storage backends with **RocksDB**, **FoundationDB**, **PostgreSQL**, **mySQL**, **SQLite**, **S3-Compatible**, **Azure** and **Redis** support.
  - Full-text search available in 17 languages using the built-in search engine or via **Meilisearch**, **ElasticSearch**, **OpenSearch**, **PostgreSQL** or **mySQL** backends.
  - Text extraction and indexing of **PDF**, **Microsoft Office**, **OpenDocument**, **EPUB** and **RTF** documents.
  - Sieve scripting language with support for all [registered extensions](https://www.iana.org/assignments/sieve-extensions/sieve-extensions.xhtml).
  - Email aliases, mailing lists, subaddressing and catch-all addresses support.
  - Automated DNS management.
  - Automatic account configuration and discovery with [PACC](https://datatracker.ietf.org/doc/draft-ietf-mailmaint-pacc/), [autoconfig](https://datatracker.ietf.org/doc/draft-ietf-mailmaint-autoconfig/) and [autodiscover](https://learn.microsoft.com/en-us/exchange/architecture/client-access/autodiscover?view=exchserver-2019). 
  - Multi-tenancy support with domain and tenant isolation.
  - Disk quotas per user and tenant.
- **Encryption** with **OpenPGP** and **S/MIME**:
  - Encryption at rest of each user's mailbox with their own OpenPGP key or S/MIME certificate.
  - Encryption gateway that encrypts outgoing mail for remote recipients, with opportunistic or required policies and no software needed on the sender's side.
  - Recipient key discovery from Web Key Directory, keys.openpgp.org, HKP keyservers and DNSSEC-validated OPENPGPKEY and SMIMEA records.
  - Key harvesting from S/MIME signatures and Autocrypt headers of authenticated incoming mail.
  - S/MIME certificate validation with bundled root certificates, custom trust anchors and revocation lists.
  - Post-quantum encryption with ML-KEM for both OpenPGP ([RFC 9980](https://www.rfc-editor.org/rfc/rfc9980)) and S/MIME ([RFC 9936](https://www.rfc-editor.org/rfc/rfc9936)).
  - OpenPGP v4 and v6 keys with RSA, ECDH (NIST P-256, P-384, P-521, Curve25519), X25519 and X448, and AES-OCB or AES-GCM authenticated encryption ([RFC 9580](https://www.rfc-editor.org/rfc/rfc9580)).
  - S/MIME with RSA (PKCS #1 v1.5 and RSA-OAEP), ECDH (NIST P-256, P-384, P-521), X25519 and X448 certificates, and AES-CBC or AES-GCM content encryption.
  - [Header protection](https://www.rfc-editor.org/rfc/rfc9788) with Subject hiding.
- **Secure and robust**:
  - Automatic TLS certificate provisioning with [ACME](https://datatracker.ietf.org/doc/html/rfc8555) using `TLS-ALPN-01`, `DNS-01`, `DNS-PERSIST-01` or `HTTP-01` challenges.
  - Automated blocking of IP addresses that attack, abuse or scan the server for exploits.
  - SSRF protection for outbound requests.
  - Rate limiting.
  - Security audited (read the [report](https://stalw.art/blog/security-audit)).
  - Memory safe (thanks to Rust).
- **Scalable and fault-tolerant**:
  - Designed to handle growth seamlessly, from small setups to large-scale deployments of thousands of nodes.
  - Built with **fault tolerance** and **high availability** in mind, recovers from hardware or software failures with minimal operational impact. 
  - Peer-to-peer cluster coordination or with **Kafka**, **Redpanda**, **NATS** or **Redis**.
  - **Kubernetes**, **Apache Mesos** and **Docker Swarm** support for automated scaling and container orchestration.
  - Read replicas, sharded blob storage and in-memory data stores for high performance and low latency.
  - Tiered caching with snapshots that can be swapped to disk, Redis or the blob store.
- **Authentication and Authorization**:
  - **OpenID Connect** authentication.
  - OAuth 2.0 authorization with [authorization code](https://www.rfc-editor.org/rfc/rfc8628) and [device authorization](https://www.rfc-editor.org/rfc/rfc8628) flows.
  - OAuth scopes, connected applications management and [Client ID Metadata Documents](https://datatracker.ietf.org/doc/draft-ietf-oauth-client-id-metadata-document/).
  - **LDAP**, **OIDC**, **SQL** or built-in authentication backend support.
  - System for Cross-domain Identity Management ([SCIM](https://www.rfc-editor.org/info/rfc7643/)) v2 for automated provisioning.
  - Two-factor authentication with Time-based One-Time Passwords (`2FA-TOTP`) 
  - Application passwords (App Passwords).
  - Roles and permissions.
  - Access Control Lists (ACLs).
- **Observability**:
  - Logging and tracing with **OpenTelemetry**, journald, log files and console support.
  - Metrics with **OpenTelemetry** and **Prometheus** integration.
  - Webhooks for event-driven automation.
  - Alerts with email and webhook notifications.
  - Live tracing and metrics.
- **Web-based administration**:
  - Dashboard with real-time statistics and monitoring.
  - Account, domain, group and mailing list management.
  - SMTP queue management for messages and outbound DMARC and TLS reports.
  - Report visualization interface for received DMARC, TLS-RPT and Failure (ARF) reports.
  - Configuration of every aspect of the mail server.
  - Log viewer with search and filtering capabilities.
  - Self-service portal for password reset and encryption-at-rest key management.
>>>>>>> v1.0.3

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

The fork is based on upstream Stalwart `v1.0.1` and has an independent release version,
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
