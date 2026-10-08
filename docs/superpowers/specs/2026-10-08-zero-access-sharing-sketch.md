# Zero-access calendar: sharing and invitations, design sketch

Date: 2026-10-08. Status: sketch, not binding. Scope: releases after 1.

Release 1 (spec revision 6, plans 1 to 3) refuses sharing on calendars owned
by key accounts and sends no invitations. This note records the intended
direction so that later spec revisions start from it instead of from a blank
page. Nothing here changes release 1 behaviour, and the terms (key account,
master key, EWK, DEK, `X-ZA-KEY`, wrap type) are those of
`2026-10-06-zero-access-calendar-design.md`.

## 1. Why sharing needs a new key layer

In release 1 every event DEK is wrapped only under the owner's master key
through the EWK (`X-ZA-KEY`, wrap type `mk`), and a collection's bundle is
wrapped the same way. No other principal can ever unwrap them. The server
holds a key only while a session that supplied it is running, and background
code never needs one (invariant 6). Sharing therefore cannot be an ACL change
alone: granting a second principal access to a sealed calendar without a key
for them would hand over ciphertext, and the only alternative, unsealing on
the server for the grantee, would put the owner's plaintext in a session the
owner does not control.

## 2. The keypair already exists

Section 3 of the spec gives every key account an X25519 keypair in the vault
record. The public key is stored in the clear and the private key is wrapped
under the master key, so it is rotated with the other wraps when recovery
installs a new master key. Section 1.2 item 1 already plans to wrap event
keys to internal attendees' public keys at invite time, and section 7.1
reserves a public-key wrap type, `pk`, for exactly that.

Sharing adds no new primitive. It reuses this keypair, and the server's
capability with it is write-only: given a public key it can seal a key to a
user without being able to read what it sealed. The one algorithm choice
still open is the composition for the wrap, either an HPKE profile or an
X25519 agreement feeding the existing XChaCha20-Poly1305. That is settled at
planning time and affects only the format byte of the new wrap.

## 3. Collection keys

A shared calendar gets its own collection key. Event DEKs are wrapped under
the collection key instead of the EWK (a new wrap type), and the collection
key is wrapped once for each principal who may read the calendar, under that
principal's public key. The wrap is stored with that principal's ACL grant
or with their preferences entry. The owner's own access is one such wrap, so
the owner's path differs from a grantee's only in which key does the
unwrapping.

Unsealing costs one extra unwrap per collection. It is done once at session
start and cached with the session keys, so per-event cost is unchanged. The
associated data of every wrap gains the collection identity alongside the
account id and purpose, so that a wrap cannot be swapped between
collections by someone with write access to the store. This is stricter than
the release 1 collection bundle, whose associated data names only account
and purpose so that a copy or move needs no resealing; a shared collection
that is copied becomes a new collection and gets a new key.

## 4. Sharing as a key exchange

A grant is one request from the owner's session. The session unwraps the
collection key it holds, wraps it to the grantee's public key, and writes the
ACL entry and the wrap together, in one conditional write. No background job
needs a key, and the server never sees a key outside a request.

A revoke removes the wrap and the ACL entry and rotates the collection key
for future writes. Events sealed before the rotation stay sealed under the
old key, which the revoked principal may have kept, so they remain
protected only by the server's ACL check. That is the guarantee upstream
gives today, and it is the weakest honest statement available without
rewriting history.

Open decision: whether revocation must also reseal existing events, at the
cost of a session-time rewrite of the whole collection, or whether ACL
enforcement suffices for data the grantee could already read. The answer
depends on what the product claims to a user who removes a person from a
calendar, and the claim should be written before the mechanism.

## 5. Grantee sessions and writes

A grantee unwraps the collection key at login with their own private key and
seals new events under it, so writes by grantees need no owner involvement
and no owner session.

Two release 1 rules relax. Plan 2 requires a sealed collection to carry
exactly one preferences entry, the owner's; that becomes one sealed entry
per principal, each sealed under that principal's own key, so each grantee
keeps a private name, colour and sort order for a calendar they did not
create. And the DAV gate's `za_session_keys` must resolve keys per
collection, not per account, because one request path can now cross a
collection the caller owns and one they only read.

## 6. Who can be a grantee

Sharing to a non-key account is refused. The grantee's wrap would have
nowhere safe to live, and their view of the calendar would be plaintext at
rest on their side, which would make the owner's guarantee depend on an
account the product cannot vouch for.

Groups have two options. The first keeps them as they are today:
operator-managed plaintext calendars, which a key account may read, and
which are not zero-access. The second gives a group a keypair whose private
key is wrapped to each member, with every membership change requiring a
member session to re-wrap, because the server cannot do it alone. Both are
recorded here. The first is the release 1 state and needs no work; the
second is a feature in its own right and should be decided on demand, not
assumed.

## 7. Free-busy as the first step

Times, durations and transparency are visible metadata by design (sections 2
and 6). Busy-time sharing to other users therefore needs no key at all, only
an ACL rule and the plan 2 withheld-content path (`ZaFreeBusy::Withheld`),
which already answers with busy intervals and nothing else. Release 1
returns 403 for free-busy on key accounts' calendars (section 9); relaxing
that gate to the withheld path is a policy change, not a format change, and
it can ship before any key exchange exists.

## 8. Invitations ride on the same primitive

At delivery the server seals the attendee's copy of the event to the
attendee's public key (wrap type `pk`) without reading it. The attendee's
next session rewraps it under their master key, or under the target
collection's key, on first write. The invitation is the case where the
server must act with no session of the recipient present, and the
public-key wrap is what lets it do that without holding anything it could
later be made to disclose.

This is why the release order in spec section 1.2 puts invitations first and
leaves sharing for after them: invitations need only the `pk` wrap, while
sharing also needs collection keys, per-principal preferences and per-
collection session resolution. Several release 1 gates must be reopened for
invitations: inbound iMIP ingest, iTIP send, the scheduling inbox and
notification collection, and the auto-schedule advertisement in the
`OPTIONS` response header. Each reopening needs its own coverage in the leak
test, since each is a place where plaintext could leave the sealed form.

## 9. What this changes in the stored format

New wrap types appear in `X-ZA-KEY` and in the collection bundle: the `pk`
wrap for invitations and the collection-key wrap for events in a shared
collection. Per-principal wraps live in the ACL grant or in the preferences
entry, in existing fields, so no struct layout changes (invariant 1). The
policy version stays 1 unless the visible set changes, and nothing in this
sketch changes it.

The leak scanner gains a check that every wrap present belongs to a current
grantee, so that a stale wrap left behind by a revoke, or a wrap for a
principal who was never granted, is reported as a failure and not carried
silently.

## 10. Open decisions

- The wrap algorithm: an HPKE profile or X25519 plus the existing AEAD.
- Revocation semantics: rotate for future writes only, or reseal existing
  events in a session.
- The group model: operator-managed plaintext groups, or group keypairs
  wrapped to each member.
- How a grantee's public key is trusted: on first use, or pinned by the
  account page so that a substituted key is visible to the owner.
- How a recovered account, whose master key is new, re-wraps its private key,
  and whether grantees must be re-shared. The private key itself need not
  change, which would leave grantees' wraps valid; if recovery has to rotate
  it, every collection key the account owns must be re-wrapped to the new
  public key by a session before sharing works again.
