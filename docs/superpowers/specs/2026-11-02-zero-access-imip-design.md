# Zero-access calendar: invitations by email (iMIP, release 5) design spec

Date: 2026-11-02. Status: revision 3, draft for review. Revision 3 answers
the re-reviews of revision 2 (Jay's codex review's R2-1 to R2-3 and an
internal review's G1 to G13): the attendee-side REPLY gate and the
internal-only delegating fan-out (4.3, 5.4, 5.5, 8.2, 8.6; G1, G2), the
mapping caveat of decision 11 (2; R2-2), the duplicate REPLY and CANCEL (3.2,
4.6; R2-3), a commit that fails inside the store and the chunked collection
DELETE (5.4, 8.2, 8.6, 10; R2-1, G5), the oracle's precondition and counts
(3.3, 8.3; G3, G4), group organizers (5.1, 6; G6), and corrections (3.2,
4.1, 4.3, 8.2, 8.5, 8.6; G7 to G13). Revision 2 answered
Jay's codex review (`2026-11-02-zero-access-imip-design-codex-review.md`) and
an internal review: the forged-item disclosure accepted as Jay's decision 11
(2, 3.4, 8, 9; codex P1), the non-participant residual (4.6, 8.2; codex
residual, F1), the collection DELETE's chunks (5.4, 5.5; codex 2, F19),
delegate tokens (4.3, 8.2, 9; codex 3, F15), the RCPT status (8.5; codex 4,
F2), the duplicate REQUEST (3.2, 4.6; F3), the extractions and imprecisions
(3.1, 4.1; F4, F19), the known-UID scan and race (4.1; F5, F10), the dispatch
order and the repeated fill (4.3, 4.4; F6, F12), the commit hooks (8.2,
8.6; F7), the oracle's RSVP page (3.3, 8.3; F8), the inventories (8.5, 10;
F9), the local refusals and disabled addresses (5.1, 5.4, 9; F11, F17),
lookup errors (7; F13), the composer test (8.1; F14), three inherited limits
(3.2; F16) and REFRESH cost (5.4; F18). Scope: release 5 of
the zero-access fork (jaylaney/stalwart), GitHub issue #18, milestone
"Release 5: email invitations (iMIP)", item 2 of release 1 spec section 1.2.
Base: the fork on upstream Stalwart v1.0.1 (PR #69's head).

The terms key account, master key (MK), data key (DEK), session keys, visible
by structure, the `pk` wrap, pending item, drain, fan-out, token pair,
withheld entry, grantee, `SENT-BY` placement and the second run are those of
the release 1 spec (`2026-10-06-zero-access-calendar-design.md`), the release
2 spec (`2026-10-11-zero-access-invitations-design.md`), the key pinning spec
(`2026-10-17-zero-access-key-pinning-design.md`), the mail spec
(`2026-10-20-zero-access-mail-design.md`) and the sharing spec
(`2026-10-22-zero-access-sharing-design.md`, revision 14), cited by release.
Every invariant of those specs (1 to 40) holds except where section 9 amends
one by name. Jay's decisions are stated as decided; decisions of this spec are
marked **D1** to **D17**, each with its reason.

## 1. Purpose

A key account handles invitations with people outside the server the way any
mail account does, from the native clients already in use, with nothing new
to learn: an invitation from someone you generally know arrives, and you reply
to it or not.

### 1.1 In scope

- **Invited from outside.** An outside organizer's REQUEST reaches the key
  account's calendar; accepting or declining in the client mails the REPLY
  back. Updates and cancellations follow.
- **Inviting outside people.** An outside attendee of a key organizer gets an
  ordinary invitation email; the attendee's emailed REPLY updates their status
  on the organizer's event. Updates and cancellations follow.
- **Plain local accounts** on the same server are reached the same way, by
  mail, in both directions (4.5, 5.6).
- **A grantee scheduling from a shared calendar** (release 4) mails outside
  recipients from the owner's address with `SENT-BY` naming the grantee.

### 1.2 Out of scope

- The HTTP RSVP page for a key organizer's invitations; outside people reply
  from their own mail client. The refusals in `http_rsvp_handle`
  (`crates/groupware/src/calendar/itip.rs:455-460`) and
  `http_rsvp_attendee_copy` (`:758-766`) stay.
- The sealed notification email for invitations between key accounts (issue
  #70, later); free-busy with outside parties; iSchedule; JMAP calendars
  (release 6).
- Invitations between key accounts on this server: unchanged (release 2, key
  pinning, release 4). They never travel by mail.

### 1.3 Assumptions

- Every product account is a key account from creation (release 1 section
  1.3); plain local accounts are admin and test accounts.
- Outbound mail leaves through the deployment's relay (SES for Jay), with SPF
  and DKIM aligned: deployment work, not this design.
- Scheduling settings are upstream's (`CalendarScheduling`, read into
  `GroupwareConfig`, `crates/common/src/config/groupware.rs:101-186`); no
  setting, permission or event type is added. `autoAddInvitations` keeps
  upstream's default (off); turning it on is a deployment choice.

## 2. Threat model delta

**Claim, unchanged** but for Jay's decision 11 (below). The operator cannot
read stored data, or data while the user is logged out. An inbound invitation
is stored sealed and reaches the calendar at the user's next sync. An outbound
invitation or reply is built in the user's own request, while the keys are
present, and handed straight to the SMTP queue; it never sits in the task
queue.

**Newly exposed, necessarily.**

- **The recipient** reads the invitation, update, cancellation or reply:
  upstream's iMIP email, whose body renders the summary, description, time,
  location, conference link and attendee list (names and addresses), and whose
  `text/calendar` part holds the whole message. A plain local recipient stores
  it in its mailbox in the clear.
- **The SMTP queue** holds it in plaintext until relay or local delivery, and
  its blob until the next blob purge: the mail spec's accepted queue residual
  (its section 2, invariant 28). The relay (SES) sees it in transit, as any
  mail a key account sends. **SMTP logs** record its envelope: who invited
  whom outside the server, and when (mail spec section 2, transit metadata).
- **Visible metadata of outside invitations.** Release 3 kept an inbound iMIP
  part inside the sealed mail blob; it now also becomes a pending item and an
  event, which show what release 1 and 2 list as visible (UID, METHOD, times,
  recurrence, SEQUENCE, STATUS, bundle size classes) and a creation time a
  store reader can match to the sealed mail delivered beside it. The UID,
  visible by structure since release 1, is the organizer's choice, and some
  systems build it from the organizer's address or domain.
- **Outside senders can create pending items**, bounded in 4.6.
- **Local lists and groups** (Jay's decision 10, 5.1): a key organizer's
  invitation to a local mailing list or group reaches the key members'
  mailboxes through the SMTP queue in plaintext, then sealed.
- **Forged mail.** Trust in an outside sender is upstream's: DMARC pass or an
  authenticated local submission (`is_authenticated`,
  `crates/email/src/message/delivery.rs:41-43`). Mail that fails DMARC
  (forwarding, many mailing lists) yields no calendar effect, as upstream.

**Not exposed.** The contents of a pending item, of the account's copy of the
event and of the mail (all sealed); the sender's address, which no visible
field holds (4.2); any token, which never leaves the sealed bundle.

**Integrity**, as in release 2 and the mail spec (plan 13, R17): a `pk`-sealed
item has no authenticity, so a store writer can forge a whole mail-borne item,
sender included, or drop one; tampering with an item that is present is
detected.

**Jay's decision 11: an exception to release 1's integrity statement,
accepted.** Release 1 section 2 says an operator who can write to the store
can corrupt sealed data but not read it. Such an operator can forge a whole
mail-borne item, sender included, with no SMTP delivery. At the user's next
session: (a) a forged REFRESH naming a listed attendee makes the drain mail
the event's current full state through the SMTP queue (D14), readable there
until relay and its blob until purge; (b) a forged delegating REPLY from a
listed attendee makes upstream's merge add the delegates as attendees
(`crates/groupware/src/scheduling/inbound.rs:556`, "Add unknown delegated
attendees"), the drain invites a key-account delegate at once (D15), and the
organizer's later edits go to every delegate. Both need a listed attendee
that is not a local key account (4.1 step 4 refuses that sender, and the drain
repeats it), so an event whose participants are all key accounts is not
exposed while the address mappings are unchanged. A store writer controls
address-to-account mappings (key pinning spec section 4.3): by disabling a
key attendee's address or remapping it to a plain principal, it brings an
originally all-key event within this accepted exposure. No further
mitigation is added. The delta: on-demand extraction of the current state of
events with an outside or plain-local attendee, whose sent versions already
cross the queue (those created before release 5 never did), and of any event
whose attendee mappings the store writer changes. D14 and D15 are unchanged.
Follow-up: issue #72 (verify DKIM provenance in the drain), which carries the
mapping qualification.

## 3. The contract, the oracle and the stopping rule

### 3.1 Parity by construction

Every mechanism this release adds reuses upstream's own code under different
inputs rather than re-deriving its output:

- **Builders.** `itip_create`, `itip_update`, `itip_cancel` and 1.0's
  per-recipient split already run on the plaintext tree in a key account's
  PUT and DELETE (`za_schedule`, `crates/dav/src/calendar/update.rs:1214-1305`;
  `ZaDeleteMessage`, `crates/groupware/src/calendar/fanout.rs:148-165`). Their
  messages are what is mailed. **Sender checks** are `itip_ingest`'s.
- **Composer.** `send_imip`'s MIME composition and template
  (`crates/services/src/task_manager/tasks/imip.rs:76-401`, `ItipTemplate`
  `:412-745`), minus the RSVP links, fed the `TaskCalendarItipContents` that
  `ItipMessages::new` (`crates/groupware/src/scheduling/itip.rs:491-504`)
  builds for the task, held in memory instead of queued.
- **Merge.** `itip_process_message`, `itip_merge_changes` and
  `itip_import_message` inside the drain, as the drain already runs them.

Three upstream-file changes expose what cannot be called as it stands, with no
behaviour change for ordinary accounts:

1. `imip.rs`: `send_imip`'s setup (logo, account info, formatter, `:82-126`;
   per message the summary, sender info and template, `:127-171`) and the body
   of its per-recipient loop (render, `MessageBuilder`, the local session's
   MAIL FROM, RCPT TO and DATA, `:172-343`) become two functions. The
   per-message step, where the sender resolution runs (`:133-162`), takes the
   sender-account resolution as a parameter, and the per-recipient function
   the session's redaction flag (D12). `send_imip` calls them in the
   same order with upstream's `account_id_from_email(from, true)`, RSVP links
   on and the flag off, and emits its `ItipMessageSent` and `ItipMessageError`
   events from the result, field for field as today. The fork passes the
   quiet `za_account_id_from_email`, so its path traces no sender address
   (D17, invariant 47), with RSVP links off and the flag on.
2. `crates/groupware/src/calendar/itip.rs`: the start of `itip_ingest`
   (`:117-182`) becomes two functions, parse with the Exchange
   missing-ORGANIZER fill (returning the owned tree), then the snapshot with
   the method's sender rule (taking `&ICalendar`), since one function cannot
   return a tree with snapshots that borrow it; its auto-add predicate
   (`:267-280`) becomes a third. `itip_ingest` calls them unchanged.
3. `crates/services/src/lib.rs`: one line in `SpawnServices::spawn_services`
   registers the fork's submitter (5.3).

### 3.2 Inherited limitations, accepted

Upstream 1.0.1 behaviours the fork would otherwise correct, recorded instead:

1. DMARC is the basis of trust for outside senders (section 2).
2. With auto-add off, an invitation for an unknown UID from a sender that is
   neither a local principal nor in a ContactCard reaches the mailbox only
   (`AutoAddDisabled`).
3. A REQUEST, ADD or CANCEL must come from the ORGANIZER's address: an
   assistant sending with `SENT-BY`, or a delegator forwarding a REQUEST
   (upgrade ruling 166), is refused.
4. PUBLISH, COUNTER and DECLINECOUNTER are never applied (`UnsupportedMethod`).
5. A delivery the queue retries re-runs the iMIP processing. An ordinary
   account's duplicate check stops it before iMIP; a key account skips that
   check, so a retried REQUEST, a status-bearing REPLY or a CANCEL can yield
   another event write and a second drained notification, a recorded residual
   rather than parity (4.6).
6. A CANCEL copies each ATTENDEE line with its `SCHEDULE-STATUS` (upgrade
   ruling 109), and the CANCEL to a removed attendee carries a summary of the
   new event (ruling 113); both now reach outside mailboxes.
7. An RSVP link in an organizer's email (a plain local organizer's, or a
   provider's own buttons) updates only the organizer's copy, never the key
   account's.
8. No `SCHEDULE-STATUS` follows a mail's fate; a remote bounce reaches the
   sender's mailbox as a DSN.
9. A REPLY is applied whatever its SEQUENCE, so a reply that crossed a
   reschedule in the mail updates PARTSTAT (D6, withdrawn).
10. `must_match_sender` compares MAIL FROM exactly with the account's
    addresses (`crates/smtp/src/inbound/mail.rs:215-241`): a key attendee
    invited at a sub-address (`key1+tag@example.com`) replies from it, and the
    REPLY is refused at MAIL FROM and never sent, as upstream's task refuses
    it.
11. A grantee who organizes under its own address from an owner's shared
    calendar receives outside REPLYs into its own account, whose UID index
    has no such event (`EventNotFound`), as upstream's `itip_ingest` fails.
12. `sender_is_organizer_or_attendee` (`snapshot.rs:388-396`) accepts the
    ORGANIZER as a REPLY sender, so an outside or plain-local organizer's own
    REPLY reaches the drain and, on the attendee's copy, ends as
    `UnsupportedMethod` (`inbound.rs:350-366`: the attendee-side REPLY arm
    admits only a delegate replying to its delegator), deleted (D7).
13. An authenticated local submission is trusted on its envelope: with
    `must_match_sender` off (`crates/smtp/src/inbound/mail.rs:215-241`), any
    authenticated local account can send iMIP from any address, as upstream's
    ingest trusts it.

### 3.3 The oracle

A differential test (8.3) runs one iMIP exchange against a plain local account
and against a key account: an outside organizer's REQUEST, an update, a REPLY
from the account, a CANCEL; then the account as organizer of an outside
attendee: REQUEST, the attendee's emailed REPLY, an update, a CANCEL. The key
account's unsealed calendar state, the message trees of its drained inbox items
and its outbound messages must equal the plain account's, up to these
permitted differences:

1. The sealing; the key account is read through its unsealed views.
2. Addresses, display names and hrefs, mapped between the accounts.
3. Volatile values: Message-ID, Date, MIME boundaries, the logo CID,
   DTSTAMP, imported file names. The oracle runs with `httpRsvpEnable` off
   (8.3), so the email bodies otherwise compare exactly.
4. `SCHEDULE-STATUS`, in stored copies and the CANCELs that copy it (Jay's
   decision 9).
5. Timing: the key account's inbound effects follow its next DAV request.
6. The inbox row: `changed_by` (D4), `calendar_ids` and flags (a sealed item
   is always `EVENT_NOTIFICATION_PRIVATE` with no calendar ids,
   `seal/item.rs:137-153`, while upstream's notification carries the event's
   audience flags and its calendar, `itip.rs:344-351`, `:984-993`). The
   oracle compares message trees, not rows, and requires
   `EVENT_NOTIFICATION_ZA_PENDING` clear on every key-side item once drained
   (8.3).
7. SEQUENCE values key pinning's reconciliation raised (its 5.1), the
   `SENT-BY` scrub (release 4 6.2, point 1) and a failed submission, which
   upstream's task retries and the fork only traces (Jay's decision 9), none
   of which the oracle exercises.

### 3.4 The stopping rule

The oracle is this release's stopping criterion for review: a later finding is
in scope only if it breaks a stated invariant (section 9) or shows the oracle
would pass on wrong behaviour. Anything else is recorded as a residual or
deferred. Findings about the residual accepted as Jay's decision 11 (forged
mail-borne items, section 2) are out of scope for review; issue #72 owns it.

## 4. Inbound

### 4.1 At delivery

The iMIP branch of `email_ingest_into` (`crates/email/src/message/ingest.rs:505-598`)
keeps upstream's gate for every account minus `!za_key` (`:508`):
`itip_enabled`, not spam, `sender_authentication.is_authenticated()`, and the
recipient's `CalendarSchedulingReceive`. For a key account (`za_key`, `:283`)
each qualifying `text/calendar` part with a `method` attribute goes to one fork
call, `za_imip_item` (new module `crates/email/src/message/za/imip.rs`),
instead of `itip_ingest`. The plaintext is in memory, the sender is upstream's
(`:527-541`: the envelope MAIL FROM for `Submission`, else the first From
address, sanitized), and the public key is the one `za_ingest_wrap` served for
the mail (`IngestWrap::Public`); no session key is read (mail spec invariant
29). An SMTP delivery's token never holds keys; should `za_ingest_wrap` return
`IngestWrap::Session` there, the key branch writes no item and traces it.
`za_imip_item` runs, in order, and writes nothing when a step refuses:

1. The size limit, `itip_inbound_max_ical_size` (`:525`).
2. Upstream's message checks, through the two functions extracted from
   `itip_ingest`: parse with the Exchange fill (the recipient's address), then
   the snapshot under the account's addresses and the method's sender rule
   (REPLY, REFRESH and COUNTER from a listed participant; REQUEST, ADD,
   CANCEL, PUBLISH and DECLINECOUNTER from the ORGANIZER).
3. **The method.** Only REQUEST, ADD, CANCEL, REPLY and REFRESH continue; the
   others end as `UnsupportedMethod`, upstream's merge's answer when an event
   exists. For an unknown UID upstream answers `AutoAddDisabled` or
   `EventNotFound` first, so the fork's fixed reason differs from upstream's
   trace there.
4. **A key account's mail.** A sender the quiet lookup
   (`za_account_id_from_email`, `crates/common/src/cache/principals.rs:736`)
   resolves to a local key account is refused: key accounts reach each other
   only through the fan-out.
5. **The known UID.** The UID is known when the account holds an event with it
   (the UID index) or a pending item whose root names it, both visible by
   structure. A REQUEST for an unknown UID continues only if the auto-add
   predicate allows it: `autoAddInvitations` on, a sender that resolves to a
   local principal, or a ContactCard of the account with the sender's address
   (`document_exists` on `ContactField::Email`). Any other method for an
   unknown UID ends as `EventNotFound`, upstream's answer.
6. **The item** (4.2), through the fan-out's item writer (`write_item_inner`,
   `fanout.rs:1485-1600`), generalized over the sender: it takes a `ChangedBy`
   and the sender carrier in place of its `sender_id: u32` and the hardcoded
   `ChangedBy::PrincipalId` (`:1557`), and `seal_pending_item` takes the
   carrier too. It keeps the inbound size check, the quota on the sealed size,
   the notification limit with its eviction of the oldest drained item (a
   pending item is never evicted; upgrade rulings 176, 187, 191), the text
   round-trip (`za_item_shape`), the seal to the served key, one batch in the
   account.

A refusal or a failed write is traced (section 7) and never fails the
delivery: the mail is stored sealed whatever happens to the item, as upstream
stores it whatever `itip_ingest` answers. The key branch pushes nothing to
`itip_messages`, so `ItipMessages::queue` (`ingest.rs:933-936`) is unreached.

**D1: delivery decides from the message and keyless indexes only, and the
drain repeats every check it can** (4.4). Reason: delivery has the mail and no
keys, the drain the keys and no mail; an item is written only where upstream
would have acted.

**D2: auto-add is decided at delivery and again in the drain.** ContactCards
and their email index are plaintext for key accounts (no seal code in
`crates/groupware/src/contact/` or `crates/dav/src/card/`; delivery reads the
index for `card_is_ham` today, `ingest.rs:429-446`). Reason: a stranger's
invitation creates no item, as it creates no event upstream; the drain's
repeat gives upstream's answer for the state it finds.

**D3: the known UID includes pending items.** Reason: an invitation and its
cancellation that both arrive while the user is logged out would otherwise
lose the CANCEL (no event yet) and import the event as active; upstream,
importing at ingest, finds the event. The cost: no index holds pending items
by UID, so for a UID the event index lacks, delivery scans the account's
pending notification rows and their content archives once per inbound part,
bounded by the notification limit when one is configured and otherwise only
by the inbox's size. The race left: a CANCEL delivered before
its REQUEST's item commits ends `EventNotFound`, as upstream drops a CANCEL
that arrives before its REQUEST.

### 4.2 The item format delta

A mail-borne item is a release 2 pending item (release 2 section 9) with one
more root carrier and a fixed row:

- **`X-ZA-SENDER`**: the sender's address, sealed with the item's DEK by the
  bundle helpers of `crates/groupware/src/calendar/seal/tree.rs` (format byte
  1, padded to 256 bytes), associated data
  `za/v1|itip-sender|<recipient account id>|<UID>`. Root order: the
  `X-ZA-SEALED` entries, then exactly one of `X-ZA-RSVP-TOKEN`, `X-ZA-EXTRA`
  or `X-ZA-SENDER`, then `X-ZA-KEY`; anywhere else, or beside another of the
  three, it is a stray carrier. `seal_pending_item` and `open_pending_item`
  (`seal/item.rs:223-309`) take and return it (`PendingCarriers` gains
  `sender`, without `Debug`). Only `za_imip_item` writes it; the drained form
  carries none of the three.
- **The row**: `changed_by = ChangedBy::CalendarAddress("")` for every
  mail-borne item, pending and drained; the other fields as
  `sealed_item_record` (`seal/item.rs:137-162`) sets them.

**D4: provenance and sender ride in the sealed carrier; the row names no
sender.** Reason: no stored struct changes layout (invariant 1); the carrier
is bound to the item's DEK, recipient and UID, so it cannot move. The visible
`changed_by` holds neither address nor principal id (plain local senders
included): upstream's non-principal variant with the address withheld, read as
`NO_ID` by upstream's notification visibility. It shows that the item came by
mail, as the sealed mail beside it does; DAV never serves `changed_by`.

### 4.3 The drain's mail arm

`drain_item` (`crates/groupware/src/calendar/drain.rs:539-623`) opens the item,
reads its method (ADD and REFRESH accepted only with the carrier), computes its
UID and checks `pass.blocks_uid` (`:594-623`, release 2 section 6.2's skipped
items) as for every item; only then does it dispatch on the carrier, so a
mail-borne CANCEL behind a skipped REQUEST waits with it and the failure limit
counts mail-borne items too. An item with `X-ZA-SENDER` goes to the mail arm,
every other item to `apply` (`:651-751`), whose release 2 arms keep their
behaviour exactly; so an item whose carrier a store writer removed fails
closed there (`SenderIsNotOrganizerNorAttendee` for a `CalendarAddress`).

**D5: `Incoming` gains `Mail { sender }`, and `Method` gains `Add` and
`Refresh`, accepted from mail-borne items only** (a fan-out item with any
method but REQUEST, CANCEL or REPLY stays a permanent error). The mail arm:

1. Repeats 4.1 steps 2 to 4 on the opened tree with the carrier's sender, the
   Exchange fill included (REPLY and REQUEST only, as upstream applies it,
   `itip.rs:132-141`; idempotent: it adds an ORGANIZER only where a
   scheduling component has ATTENDEEs and none); there is no text to parse.
2. Looks the UID up (`find_event`, `drain.rs:753`).
3. Acts as `itip_ingest` does, through the drain's existing commits:
   - **REQUEST, no event:** the auto-add predicate again, then `import`
     (`:892`) under the creation lease (release 2 section 6.3), with no pair.
   - **REQUEST, ADD or CANCEL with an event:** `merge` (`:1125`) with
     `Incoming::Mail`: `itip_process_message` with the carrier's address as
     `sender`, whose own checks (`OrganizerMismatch`, `SenderIsNotOrganizer`,
     SEQUENCE) run against the unsealed stored event.
   - **REPLY with an event:** `merge` with `Incoming::Mail`, authenticated as
     upstream authenticates it: `handle_reply` updates only the attendee whose
     address equals the sender; no token is read or required. The delegating
     fan-out (the `Incoming::Reply` arm, `drain.rs:1309-1458`) runs only when
     the stored event's ORGANIZER is one of the account's own addresses, that
     is, on the organizer's copy: when the merge adds delegates there, it runs as
     release 2 section 6.4 says (D15). A delegate's REPLY into a delegator's
     copy (an attendee's copy) takes upstream's guarded arm, applies
     upstream's merge and fans out nothing, as upstream sends nothing then.
     The gate is new because before release 5 no REPLY item reached an
     attendee's copy: release 2's routing delivers a REPLY only to the
     ORGANIZER.
   - **REFRESH with an event:** upstream answers `MergeResult::Message`, which
     is mailed (5.5).
   - **ADD, CANCEL, REPLY or REFRESH with no event:** `EventNotFound`.
4. The mail arm never adopts, replaces or removes a pair because of mail: an
   outside party supplies none, and a mail-borne REQUEST or CANCEL takes
   `Incoming::Request(None)`'s rule, leaving the attendee's copy's pairs as
   stored. The organizer-side delegating fan-out (D15) maintains the
   organizer's own pairs for new internal delegates, as release 2 section 6.4
   does: it adds a pair for each and stores the returned pairs.

**D6 (withdrawn): release 2's stale-reply check does not apply to a mail
REPLY**; the mail arm applies upstream's merge without it. Reason: upstream's
REPLY handling compares no SEQUENCE (in
`crates/groupware/src/scheduling/inbound.rs` only the REQUEST paths do, `:141`
and `:233`); a REPLY without SEQUENCE, which RFC 5546 allows at 0, would never
apply once the event passed SEQUENCE 0; and the drain runs in
`handle_dav_request` before the request is dispatched
(`crates/dav/src/request.rs:670-679`), so a mail REPLY that arrived before the
organizer's own later write is applied before that write (3.2, item 9).

**D7: a mail-borne item ends as upstream's ingest would leave the inbox.**
After an applied import or merge (upstream writes a notification) the item is
rewritten drained, as release 2 section 6.4 step 5 says. Otherwise (an
iTIP-level rejection, the key-account refusal, `AutoAddDisabled`,
`EventNotFound`, a merge that changes nothing, a REFRESH) it is deleted, with
the trace of section 7. Reason: upstream writes no inbox item then, and the
sealed mail keeps the invitation readable. Release 2's history rule stays for
fan-out items; release 2 section 6.5's error classes apply to both kinds.

### 4.4 Where each check runs

| Check | Delivery (4.1) | Drain (4.3) |
|---|---|---|
| `itip_enabled`, spam skip, DMARC or authenticated submission, `CalendarSchedulingReceive` | yes | no (needs the mail) |
| `text/calendar` part with `method`; inbound size | yes | size, at the item write |
| Parse, Exchange fill | yes, before sealing | the fill repeated on the opened tree (idempotent) |
| Snapshot, method's sender rule, supported method | yes | repeated |
| Sender is a local key account | refused | repeated |
| Known UID (event or pending item) | yes | the event lookup |
| Auto-add (setting, local principal, ContactCard) | REQUEST, unknown UID | REQUEST, no event |
| Stored ORGANIZER, participant, SEQUENCE (REQUEST), quota, event size | no (sealed) | upstream's merge, the drain's checks |

### 4.5 Plain local senders

A plain local organizer's invitation reaches a key account by upstream's path
(a task, `send_imip`, local delivery as `SenderAuthentication::Submission`,
`crates/smtp/src/outbound/local.rs:56-62`), its sender the envelope MAIL FROM
that `must_match_sender` bound. The key branch writes an item (a local
principal passes auto-add) and the drain imports it; the mail, RSVP links
included, is stored sealed. A plain local attendee's REPLY arrives the same
way.

### 4.6 Abuse bounds

**D8: the bounds are upstream's for an ordinary account, plus recorded
residuals.**
They are the spam skip, DMARC or authenticated submission, the size limit, the
known-UID and auto-add gates, the quota, the notification limit
(`MaxCalendarEventNotifications`: a new item evicts the oldest drained one, and
is refused once every stored item is pending) and the drain (`DRAIN_PASS_CAP`
items a pass, deleting what upstream would not have kept, D7); the inbox expiry
keeps pending items (upgrade ruling 174). Upstream's same senders change the
same events at once under the same limit. The residual: delivery checks
participation only in the message itself (`itip.rs:163-182` compares the
sender with the message's own snapshot), so any sender upstream's
`is_authenticated()` accepts that knows a UID (an event's or a pending item's)
can write mail-borne items. They occupy pending slots of a logged-out account
until its next session, never evicted; at the limit each evicts the oldest
drained history, and once only pending items remain other deliveries are
refused (release 2 items record `5.1`). Mail-borne writes keep upstream's
eviction of the oldest drained item, because refusing to evict would refuse
legitimate invitations once an inbox at its limit holds only drained history.
So a
non-participant can now evict drained history, which upstream's ingest-time
check against the stored event does not allow.

A second accepted residual: a delivery the queue retries can write a second
mail-borne item. Key accounts skip the thread lookup, so ingest's SMTP
duplicate check (`ingest.rs:379-405`) never fires for them, and the item
commits before the mail's batch. A retried REQUEST yields a second drained
notification, because upstream's REQUEST merge always records `Sequence` among
its changed entries (`inbound.rs:141-170`), so it never changes nothing; an
ordinary account's duplicate check stops a retried delivery before iMIP, so
this is not parity. A duplicate REPLY or CANCEL is deleted (D7) only when
upstream's merge returns no actions: a REPLY carrying `REQUEST-STATUS`
(`inbound.rs:513-515`, `:542-554`) and a repeated CANCEL (`:229-249`) produce
actions even when nothing differs, so they too can yield another event write
and drained notification. Each item counts toward the notification limit
until drained.

## 5. Outbound

### 5.1 The recipient classes

`resolve_inner` (`fanout.rs:1377-1433`) gains a third outcome,
**`Recipient::Mail`**, where it returns `NotInternal` today for an address
that resolves to no local account (`za_account_id_from_email` is `None`, or
the delegation resolution's `NotFound`) and for a local account that is not a
key account, whatever its permissions, as upstream mails it.

These stay `NotInternal`, not attempted (`5.3`), and are never mailed (Jay's
decision 5): a local key account without `CalendarSchedulingReceive` or a
served public key; the sender's own account (an unlisted sub-address); the
event owner on the delegated-organizer path (release 4 section 6.2). A lookup
failure stays `Failed` (`5.1`); an address that does not normalize is not
attempted and keeps upstream's `3.7`.

**D9: the mail class is exactly "no local account, or a local account that is
not a key account".** Reason: those are the recipients upstream mails and the
fork cannot reach otherwise; an active local key account's own address is
reached only through the sealed fan-out, whatever its state. A disabled key
account's address resolves to `None` (`EmailCache::DisabledAccountAddress`),
so it is attempted: the SMTP layer refuses it at RCPT, before DATA, so nothing
is queued; its address appears in the SMTP layer's traces; and its status is
`1.1` (5.4).

**Jay's decision 10: local mailing lists and groups are mailed, as upstream
mails them.** D9's class already covers them: `za_account_id_from_email`
answers `None` for a local mailing list, for a disabled address (an account's
or a list's) and for a domain that is not local, a relay domain included (only
`EmailCache::Account` resolves, `crates/common/src/cache/principals.rs:841-850`),
and a group is a local account that is never a key account. The SMTP layer
then expands a list, delivers to a group's own mailbox and refuses a disabled
address at RCPT, as upstream's task meets them. The invitation crosses the
SMTP queue in plaintext to every member, key members included; a key member
receives it as sealed mail and gets no calendar effect, since 4.1 step 4
refuses iMIP whose sender (the organizer) is a local key account (section 6).
That holds only when the ORGANIZER is the key account's own address. A
consequence, recorded and not changed: a key organizer that organizes under a
group's address is mailed as the group (D12), so a key member's
delivery sees a sender that is not a key account, passes 4.1 step 4 (and
auto-add, the group being a local principal) and gets an item, which its
drain applies, as an ordinary member's ingest does.

### 5.2 Routing

Phase 1 of `za_fan_out` (`fanout.rs:370-470`) resolves every address of every
message, not only those `route` (`:1180-1215`) admits, then routes by class.
The wider resolution only adds the mail class: only attempted internal
recipients reach `decide` (`fanout.rs:831`), so no address that is not
attempted is pinned or given a withheld entry.

**D10: release 2's routing restrictions apply to internal recipients only; a
mail recipient receives every message upstream's builders address to it.** The
restrictions (a REPLY only to the ORGANIZER and only with the replier's token;
no attendee-originated REQUEST, since delegates take their token from the
organizer's drain) exist for token authentication, and mail carries no token.
So a mail recipient gets an organizer's REQUEST or CANCEL, a key attendee's
REPLY to an outside or plain-local ORGANIZER (whose imported copy holds no
pair), a delegate's REPLY to a mail-class delegator, and a key delegator's
REQUEST to a mail-class delegate, as upstream sends them. Internal recipients
keep release 2's routes (a token-less REPLY to a key organizer stays `5.3`).
Mail recipients take no part in key pinning; a mail-class attendee's pair from
`token_union` never leaves the sealed bundle; the SEQUENCE reconciliation and
`SENT-BY` placement run before routing, so mail carries them.

### 5.3 Composition and submission

**D11: upstream's composer, behind a seam the fork registers.** The `dav` and
`groupware` crates cannot reach `services` or `smtp` (`smtp` depends on
`email`, which depends on `groupware`). So:

- `common::za::imip` (new fork module) holds a `OnceLock` with the submitter,
  a function pointer (code, not state, so one process may host several
  servers), and `za_imip_submit`, which answers a failure before registration.
- `services::za_imip` (new fork module) implements it with the two functions
  of 3.1 item 1, RSVP links off, registered in `spawn_services`, which the
  binary and the test harness run (`tests/src/utils/server.rs:408`).
- Its input is one (message, recipient) as upstream's task holds it, the
  `TaskCalendarItipContents` of `ItipMessages::new` with `to` the one
  recipient, plus the event's account and document id, in process memory for
  the call and never written to the task queue or any store location.
- The email is upstream's (headers, template, `text/calendar` part,
  `event.ics` attachment), except that `ItipTemplate::set_recipient`
  (`imip.rs:681-735`) is not called: no RSVP token is minted and the body
  lacks the RSVP block.
- The submission is upstream's: `Session::<NullIo>::local` with
  `SessionData::local` (`crates/smtp/src/core/mod.rs:303-342`) on a dummy
  server instance built as the task manager's
  (`crates/services/src/task_manager/manager.rs:61-70`), then MAIL FROM, RCPT
  TO and `queue_message(MessageOrigin::Internal)`, returning the queue id or
  the rejection's stage and reply code.

**D12: the sender identity is upstream's rule.** MAIL FROM and From are
`from`: the organizer's address for organizer messages, the replying
attendee's for a REPLY. The session is authenticated as the account `from`
resolves to, else the event's account (`imip.rs:133-162`), and
`must_match_sender` (`crates/smtp/src/inbound/mail.rs:215-241`) holds as for
upstream's task. A grantee's write in a shared calendar mails from the owner's
address, authenticated as the owner, the naming property carrying `SENT-BY`
the grantee; the write-time ACL is the authority, as for sealed items. Every
SMTP transaction the outbound path opens is redacted as a key account's
submission (mail spec invariant 30), including one whose `from` resolves to
another account (a key account organizing under a group's address). The
submitter reuses `SessionData::za_key_rcpt` rather than add a field: every
redaction read already checks it beside `za_key_sender()` (raw input,
`crates/smtp/src/inbound/session.rs:657`; the DATA stage, `data.rs:591`; the
spam filter, `spam.rs:100`; milters, `milter/message.rs:46`; scripts of every
stage, `scripts/exec.rs:166`). The per-recipient function sets it on each
session right after construction (3.1 item 1): `SessionData::local` starts it
false, a local session never calls `reset()` (`session.rs:579-591`, reached
only from a stream's commands), and its one other clear, `data.rs:817`,
follows the DATA stage's last read. The flag is reused for its effect, not its
name (a key-account recipient), and `crates/smtp/src/core/mod.rs` is
unchanged. Reason: MAIL FROM, `must_match_sender` and DSN routing stay an
ordinary account's, and a key account's event stays out of traces whichever
account signs.

### 5.4 Order and status

**D13 (superseded by Jay's decision 9).** It had mail submitted in phase 3 of
the fan-out, before the sender's commit.

**Jay's decision 9: outbound mail goes out after the commit.** Mail is
submitted only once the sender's commit has returned `Ok`: after the PUT's
final commit, and after the DELETE's commit, as D14's REFRESH answer already
is. A collection DELETE has one commit (`delete.rs:450-506`), which the store
may split into chunks at the events' commit points; it too mails only on
`Ok`. A change whose commit returns an error (a lost assertion, a 412, the
create's lost lease fence, a store error) or never completes (a crash) mails
nothing, as upstream never mails a lost change. The internal fan-out keeps
its phases (key pinning 5.2: resolve, decide and pin, deliver, retire) and
its place before the commit, unchanged;
it resolves the mail class in phase 1 and returns the mail attempts (one per
message and address, one recipient per email, as upstream) with its result,
for the caller to submit after its commit.

- **Status.** A new `ITIP_STATUS_SENT = "1.1"` (RFC 6638 "sent, delivery not
  confirmed") in `fanout.rs`, rank 0 in `status_rank`, is written into the
  tree before the commit (`za_set_schedule_status`, `fanout.rs:1108-1146`) for
  every mail-class recipient a message addresses: handed to the mailer. In an
  internal-only fan-out (D15) such a recipient records `5.3` instead. `1.1`
  lands on the recipient's property (the ATTENDEE, or the ORGANIZER for a
  REPLY); a later REPLY merge replaces it with upstream's `2.0`. Writing
  nothing, as upstream, would leave an earlier fan-out's `5.3` or `5.1` in
  place.
- **Residual: a submission that fails after the commit** is traced
  (`ItipMessageError`, section 7) and leaves `1.1` in place, with no corrective
  write.
- **Residual: a recipient the local SMTP layer refuses after the commit** (a
  typo on a local domain, a disabled address, an RCPT-stage milter) keeps
  `1.1`, which a client shows as sent; upstream leaves the property bare.
- **Residual: a crash between the commit and the submissions** loses those
  emails silently.
- **Residual: a collection DELETE whose commit fails in a later chunk** loses
  the CANCELs of the events an earlier chunk already deleted (the store returns
  only the error, `crates/store/src/dispatch/store.rs:299-311`), and the
  failure is traced. Like the others it loses an email, never sends a false
  one (Jay's decision 9's direction); upstream's per-event tasks commit with
  their events.
- None is retried (there is no task). For an event that still exists, the
  next change that builds a message for the recipient resends. A DELETE's
  lost message (the organizer's CANCEL or the attendee's declined REPLY; a
  crash after the commit, a failed or unregistered submission, a collection
  DELETE's later chunk failing) is permanent: the event is gone, so no later
  change to it exists, and the outside parties keep a stale state.
  Upstream's task retries for three days, and commits with the event.
- **Cost.** One SMTP queue write per mail attempt inside the request, bounded
  by `itip_outbound_max_recipients` (default 100, `za_schedule`'s 412) per
  event. Mail is submitted outside the create's UID lease, so a slow
  submission no longer risks the fence. A collection DELETE runs one fan-out
  per event with no overall cap
  (upstream queues one task per event, also uncapped). A local submission runs
  the operator's configured MAIL, RCPT and DATA-stage milters, hooks and
  scripts as upstream's task does, with the latency borne by the request. A
  drain pass sends up to `DRAIN_PASS_CAP` (32) REFRESH answers (D14), each a
  submission, inside one DAV request before the request is dispatched. The
  tests' DAV client times out at 500 ms (`tests/src/utils/webdav.rs:100`),
  which the outbound tests must respect.

### 5.5 The callers

- **PUT** by an organizer or an attendee (`update.rs:717`, `:1404`); a
  grantee's (D12), whose second-run self items stay internal.
- **DELETE** of an event or a collection (`crates/dav/src/calendar/delete.rs:480-496`):
  the CANCEL or declined REPLY is mailed after the deletion's one commit
  returns `Ok` (5.4); the statuses are discarded with the event.
- **D14: the drain's REFRESH answer** (upstream's, one REQUEST to the sender)
  goes to `za_imip_submit` directly, after the item's deletion commits, with no
  item, pin or status. Reason: upstream answers REFRESH, and the deletion's
  assertion makes one session process an item, so mailing after it sends one
  answer per REFRESH; a failure is traced and the attendee can ask again.
- **D15: the drain's delegating merge keeps release 2 section 6.4's rule**:
  its fan-out attempts internal delegates only, so a mail-class delegate added
  by a merged REPLY keeps today's `5.3` until the organizer's next update. It
  runs only on the organizer's copy (4.3). The mechanism: `za_fan_out` gains an
  `internal_only: bool` parameter, beside `kind`, under which a mail-class
  recipient is not attempted, records `5.3`, never `1.1`, and yields no mail
  attempt. A flag rather than a `ZaFanOutKind` variant, because the delegating
  merge must keep `Change`'s pin behaviour (the full state, retirement). Key
  pinning's redelivery (`contacts/pin`, `ZaFanOutKind::Redelivery`,
  `crates/http/src/api/contacts.rs:681-813`) passes it too, so it is internal
  only: a recipient that resolves to the mail class there is not attempted
  (`5.3`). Reason: upstream's
  organizer sends nothing after merging a REPLY, the delegator forwards the
  REQUEST (RFC 5546; a key delegator now mails it, D10), and a REQUEST from the
  organizer would double it; redelivery is a key-recipient mechanism.
- The drain never calls `ItipMessages::queue` or writes a task (release 2
  invariant 12).

### 5.6 Plain local attendees

A plain local attendee of a key organizer receives upstream's iMIP email,
composed in the organizer's request without RSVP links and delivered locally
(`Submission`). Its ingest is upstream's: `itip_ingest` imports the invitation
(a local principal always adds) under the mail spec's provisioning fence; its
REPLY returns through its own task and mail to the inbound path (4.5).

**D16: no new write needs the provisioning fence, and `za_has_tasks` does not
change.** The pending item goes into an account read as a key account, whose
path asserts nothing (mail spec 6.1); the outbound path writes only the
sender's own event and the SMTP queue; an ordinary recipient's writes are
upstream's, already fenced. No task is written, so `za_has_tasks`
(`crates/common/src/auth/vault.rs:852`) needs no arm.

### 5.7 Events created before release 5

A limit, recorded: nothing re-invites automatically. An event's outside and
plain-local attendees that hold `5.3` from before this release are mailed on
the next change that builds a message for them; an edit that builds none (an
alarm, a change without scheduling content) mails nobody.

## 6. What stays refused

- The HTTP RSVP page and its attendee-copy rewrite for key accounts (1.2).
- Mail between key accounts: an active key account's address is never mailed
  (5.1), and mail whose sender is a local key account never yields an item
  (4.1 step 4), even from client-side scheduling. So a key member of a local
  list or group a key organizer invites under its own address receives the
  email sealed, with no calendar effect; a group receives it into the group's
  own mailbox (Jay's decision 10). An organizer address that is a group's is
  mailed as the group, and its key members' deliveries get items their drains
  apply (5.1): a recorded consequence.
- A local key account that cannot receive: not attempted, never mailed.
- The task queue for a key account's scheduling; JMAP calendars.

## 7. Error handling and traces

- **Delivery.** A refusal or a failed item write (size, quota, notification
  limit, a store error) is traced and the delivery proceeds; a served key that
  cannot be read already fails the delivery temporarily (mail spec 6.1).
- **Drain.** A store error leaves the item pending and counts toward
  `DRAIN_ITEM_FAILURE_LIMIT`; an undecodable item or a stray carrier is
  deleted as permanent; an iTIP-level outcome ends as D7 says.
- **Outbound.** A submission that fails after the commit, or an unregistered
  submitter, is traced (`ItipMessageError`) and leaves the recipient's `1.1`
  in place (Jay's decision 9); it never fails the PUT or DELETE, which has
  already committed.
- **Sender-keyed lookups.** A store error from the ContactCard lookup or the
  directory lookup keyed by the sender's address (4.1 steps 4 and 5, 4.3) is
  traced with its type and the account id only, never the key it looked up.

**D17: the new paths' events carry no address and no content**; no event type
is added, and reasons are fixed names (`itip_reason`, `drain.rs:1847`, and
`zero-access:` details). Reason: release 2 section 10 and mail spec section 10
already require it of the fan-out, the drain and a key account's transactions.

| Event | Where | Carries |
|---|---|---|
| `Calendar(ItipMessageReceived)` | an item written | span id, account id; `From` `[redacted]` |
| `Calendar(ItipMessageError)` | a delivery refusal, the size limit | account id, fixed reason (`zero-access: iMIP from a key account` for step 4), `Limit`, `Size`; `From` `[redacted]` |
| `Calendar(ItipMessageError)` | a mail-borne item the drain deletes | account id, document id, fixed reason |
| `Calendar(ItipMessageSent)` | a mail attempt accepted | account id, document id, queue id |
| `Calendar(ItipMessageError)` | a mail attempt failed | account id, document id, stage, reply code |
| `Calendar(ItipMessageError)` | a DELETE's mail dropped after its commit erred (5.4) | account id, document id, fixed reason |

`From` stays as a key so dashboards keep their counts. The SMTP session and
queue that carry a mailed invitation trace its envelope as they trace every
message (transit metadata); the fork's own lookups stay quiet
(`za_account_id_from_email`). Ordinary accounts' iMIP events are upstream's.

## 8. Testing

### 8.1 Unit

Seal tests: an item with `X-ZA-SENDER` round-trips; the carrier fails under
another recipient, UID or item DEK; beside `X-ZA-EXTRA` or `X-ZA-RSVP-TOKEN`,
or out of place, it is stray; the drained form carries none. `fanout.rs`:
`route` gives a mail recipient every message (a token-less REPLY to the
ORGANIZER, an attendee-originated REQUEST, a REPLY to a delegator) and keeps
release 2's routes for internal ones; `1.1` ranks below `5.1`. `imip.rs`: the
extraction composes the task's bytes unchanged once Message-ID, Date, MIME
boundaries and the logo CID are pinned or normalized; RSVP links off renders
no RSVP block and mints no token.

### 8.2 The za suite

New modules under `tests/src/za/imip/`, run after `pinning::test` and before
the sharing modules:

- `imip::inbound`: an outside REQUEST (MAIL FROM `dmarc-...`, the test-mode
  DMARC pass, `crates/smtp/src/inbound/data.rs:820-825`) yields one pending
  item with `X-ZA-SENDER` and `CalendarAddress("")` beside the sealed mail,
  which the drain imports; update, ADD and CANCEL merge, the account's alarm
  surviving; a REPLY updates only its sender, a non-participant's is deleted,
  one below the event's SEQUENCE still applies (3.2 item 9). No item for no
  DMARC, junk, oversized, PUBLISH,
  a key account's own mail, or an unknown sender's new UID with auto-add off;
  that sender gets one as a ContactCard and as a local principal. REQUEST then
  CANCEL while logged out ends cancelled (D3); a ContactCard deleted before
  the drain deletes the item (D2); a REFRESH is answered by mail; an outside
  delegating REPLY invites an internal delegate and mails no outside one,
  whose line holds `5.3` (D15), and
  the case continues through that delegate's REPLY and its successful merge
  into the organizer's event (its pair, added by the delegating fan-out,
  authenticates it); the organizer's pairs are read before and after a
  mail-borne REPLY merge and a mail-borne REQUEST merge that add no delegate,
  and are unchanged; a non-participant's REPLY for a known UID shows its
  pending slot, then the drain's rejection and deletion (4.6); an outside
  delegate's REPLY into a key delegator's copy naming a key sub-delegate:
  the delegator's copy merges and no item reaches the sub-delegate (4.3); the
  plain local organizer and attendee of 4.5.
- `imip::outbound`: a key organizer's PUT mails each outside and plain-local
  attendee once with `1.1` and writes no task (the consumer held); `1.2` for a
  key attendee and `5.3` for `imip-pending`, a key account without the
  permission and the sender's sub-address; an update and a DELETE mail REQUEST
  and CANCEL; a key attendee's ACCEPTED, DECLINED and DELETE mail REPLYs to an
  outside organizer; a delegation mails the delegator's REQUEST to an outside
  delegate; a grantee's write mails from the owner's address with `SENT-BY`;
  `IMIP_SUBMIT_FAIL_ONCE` (a new hook in `common::za::test_hooks`): the status
  stays `1.1` and the failure is traced; a commit failure after the fan-out,
  on each path: nothing reaches the sink. `COMMIT_FAIL_ONCE` exists only on
  the create path (`update.rs:1503`, `calendar/mod.rs:692`), so two sibling
  hooks join it, `UPDATE_COMMIT_FAIL_ONCE` at the update PUT's commit and
  `DELETE_COMMIT_FAIL_ONCE` at the DELETE's, beside the existing
  `update_commit_pause_point` and `delete_commit_pause_point`: three cases,
  create, update and DELETE, kept for path coverage. They return before
  `commit_batch`, so a case also fails the store's commit itself after every
  precommit step has run: an update PUT and a single-event DELETE, each
  carrying a distinct change, are parked at those pause points; a competing
  revision of the event commits; the loser is released, loses its assertion
  inside `commit_batch`, and its distinct message never reaches the sink. The
  handlers' bounded retry runs only when a calendar context changed
  (`za_refresh_event`, `calendar/mod.rs:803-822`; the DELETE's retry at
  `delete.rs:506-553`), which an event revision does not change, so the
  released loser fails on its first commit. The create's counterpart: a create
  PUT parked at `commit_pause_point` while another session takes over its UID
  lease (the lease-duration override in `common::za::test_hooks`), then
  released, fails through `za_lost_create_fence` (`za_refresh_event` returns
  `None`), and no mail reaches the sink. The REFRESH answer's: two sessions
  drain the same mail-borne REFRESH item and exactly one answer reaches the
  sink, the item deletion's assertion admitting one session (D14). A
  collection DELETE whose commit is forced to split, by the existing
  `FORCE_COMMIT_SPLIT` (`crates/store/src/za_test.rs:113`, read by
  `BatchBuilder::commit_point`), and whose second chunk fails, by a new test
  hook `SECOND_CHUNK_FAIL_ONCE` in `za_test.rs` that `write_batch` consults
  before its second chunk (`crates/store/src/dispatch/store.rs:299-311`),
  mails nothing and traces the loss (5.4). The hook is one-shot and keyed to
  the batch's current account, like the other za hooks (`za_test.rs:9-12`),
  and is armed before the DELETE, since that path has no pause point
  (`delete_commit_pause_point` runs only with `retry_event`). The case uses at
  least two events with outside attendees and asserts that the first chunk's
  event is gone, the later one remains, and nothing is mailed. A key
  organizer inviting a local mailing list with
  a key member and a plain member: the list is mailed once with `1.1`, the
  plain member's ingest imports it, and the key member's mailbox holds the
  sealed email while its inbox gets no item (Jay's decision 10); the
  plain local attendee of 5.6 gets the invitation without RSVP links.
- `imip::oracle` (8.3) and `imip::traces`: section 7's events carry no address
  or canary; only the SMTP layer's envelope events name the recipient.

No test forges a mail-borne item in the store: Jay's decision 11 accepts that
exposure, and issue #72 owns its qualification test. The new modules turn the
drain on with the `DrainOn` guard (`tests/src/za/drain.rs`), since the suite
sets `DRAIN_SKIP_ALL` after setup (`tests/src/za/mod.rs:132`), and so does
any existing module whose changed assertions need a drain (`gating.rs`).

This release's outside parties live at `remote.test`. At setup, before any
module, the suite routes every non-local domain (a catch-all
`MtaOutboundStrategy` route) to an `MtaRoute::Relay` on a local port (the
routing of `tests/src/smtp/outbound/lmtp.rs`), where a test SMTP sink records
each message it accepts, so `wait_for_delivery` completes for every module's
outside attendees and the oracle reads what left. The plan checks key-mode
`webdav_tests`, key-mode `imap_tests` and `jmap_tests`, the caldav-client
job's key mode and the fresh-store harness for outside attendees, and either
routes them to a sink or records why queued retries are harmless there.
Fixtures reserve the `imip-*` family at
`example.com` (`imip-key`, `imip-key2`, `imip-plain`, `imip-pending`,
`imip-oracle-key`, `imip-oracle-plain`, `imip-grantee`) and at `remote.test`
(`imip-org`, `imip-att`, `imip-delegate`, `imip-nodmarc`); none is allocated
today, and the `keymail27` and `plainmail23` next-free names are unchanged.
Every module deletes what it creates.

### 8.3 The differential oracle test

`imip::oracle` runs 3.3's exchange against `imip-oracle-plain` (upstream's
`itip_ingest`, task and `send_imip`) and `imip-oracle-key`, with the same UID,
content and outside parties (`imip-org@remote.test`, `imip-att@remote.test`).
It turns `httpRsvpEnable` off for its run and restores it after, so both
accounts' emails take upstream's body without RSVP links and compare after
normalizing volatile values only. Its precondition: both oracle accounts hold
a ContactCard for `imip-org@remote.test` (created before the first step,
deleted at the end), rather than auto-add on, so the first REQUEST imports on
both sides. After each step it triggers the key account's drain and waits for
delivery, asserts the expected object on both sides (present after each
inbound step; absent on both after a DELETE), asserts every key-side inbox
item has `EVENT_NOTIFICATION_ZA_PENDING` clear, and compares the counts of
inbox items and sink messages each account produced in that step, then
compares, normalized as 3.3 lists: the stored events
(DAV GET), each drained item's tree against the matching plain notification's
message, and each message the sink got (From, To, Subject, Auto-Submitted,
Reply-To, the method, the decoded text, HTML and ICS parts). Any other
difference fails the test.

### 8.4 Leak coverage

`leak::test_imip` joins the leak modules, with canaries in an outside
organizer's invitation (summary, location, description, attendee names and
addresses, the sender's address, and the sender canary again as the From
header's display name, so the row and blob checks keep their teeth) and in a
key organizer's invitation to an outside attendee. The scan excludes the SMTP
layer's envelope events and their keys, accepted transit metadata (section 7):

- **Task queue:** with the consumer held across every outbound path (PUT,
  DELETE, an attendee REPLY, the REFRESH answer), `itip_tasks_for` finds no
  `CalendarItipMessage` task for any key account of the module.
- **SMTP queue:** empty once the sink has the mail; queue blobs purged
  (`expire_and_purge_blobs`) before the whole-store scan, which finds no canary
  in any subspace, blob, task, search record or trace; the sink's copies hold
  them (the positive control).
- **Visible rows:** every notification row's `changed_by` is a principal id or
  `CalendarAddress("")`, and no decoded row holds the sender canary; a
  mail-borne pending item is `pk`-sealed and shows no ORGANIZER or ATTENDEE.
- **Tokens and carriers:** each captured message is MIME-decoded (every part,
  base64 and quoted-printable) before the scan, since `token_hits` does not
  decode MIME (plan 10, "Facts for release 3"); no token of the organizer's
  pairs appears in base64url, base64 at any alignment or hex, and no `X-ZA-`
  string appears.

### 8.5 Tests that change

- `tests/src/za/fanout.rs:271, 275`: plain and outsider become `1.1`, mailed
  once; `:340` (sender's sub-address) and `:994` (no permission) stay `5.3`.
- `fanout.rs:1164-1270` (`test_task_queue`): still no task for the key
  organizer; the plain organizer's invitation now reaches key2 as a pending
  mail-borne item beside the mail.
- `tests/src/za/gating.rs` (`test_scheduling`): the plain attendee is mailed
  and gets a copy; a plain organizer's invitation yields a pending item, then
  a copy after a drain; the RSVP refusal stays.
- `tests/src/za/mail/fence.rs:740-770`: the retry after provisioning also
  writes a `pk`-sealed pending item and no plaintext calendar data.
- `tests/src/za/tracing.rs:871-895`: the external attendee is mailed; the two
  unknown local attendees, `LOOKUP_LOCAL_ATTENDEE` and `SUBADDR_ATTENDEE`
  (whose sub-addressing rule resolves to a local part no account has), both
  store `1.1` (written before the commit), and their RCPT rejections are
  verified through section 7's stage-and-code trace. `5.1` stays
  only for failures found before the commit (a lookup failure). Events
  carrying the submission's SMTP span id (the cache lookup, the
  sub-addressing evaluation, RCPT) may name them; no other event may,
  including any emitted before the PUT's commit returns. The module's
  existing never-appears controls stay for every event outside that span.
- `tests/src/za/leak.rs` (canary events with `remote.test` attendees): mail
  sunk and queue blobs purged before the scan. `itip_tasks_for`
  (`tests/src/utils/za.rs:1508`) stays.
- `tests/src/za/pinning.rs:1212` (`dave@external.test`) and
  `tests/src/za/sharing.rs:5673` (`task9-unknown@not-local.test`): these
  outside attendees are now mailed, to the sink.
- `tests/src/za/sharing.rs:7721-7729`: the group `sharing-life-team@example.com`
  (label `sole-group`) becomes `1.1` instead of `5.3` (Jay's decision 10).
- `docs/zero-access/manual-checklist.md` item 13: the external address is
  mailed, `SCHEDULE-STATUS` `1.1`.

### 8.6 Mutation sample

Each row is a mutation and the test that catches it (plan 7's style; outcomes
go in the plan's outcome note):

| Mutation | Caught by |
|---|---|
| `!za_key` restored in the iMIP gate | `imip::inbound`, no item |
| The item written without `X-ZA-SENDER` | `imip::inbound`, the REPLY rejected |
| The sender's address in `changed_by` | `leak::test_imip`, visible rows |
| No key-account sender refusal | `imip::inbound`, a key account's mail yields an item |
| The authentication check dropped | `imip::inbound`, the no-DMARC sender |
| The known UID without pending items | `imip::inbound`, REQUEST then CANCEL ends active |
| Auto-add not repeated in the drain | `imip::inbound`, the deleted ContactCard |
| Rejected mail-borne items kept as history | `imip::inbound`, a rejected item deleted (the non-participant's REPLY, the deleted ContactCard) |
| Mail recipients sent to `ItipMessages::queue` | `imip::outbound`, `leak::test_imip`: a task with the consumer held |
| RSVP links on | `imip::outbound`, an RSVP URL in the body |
| Plain local accounts left `NotInternal` | `imip::outbound`, no mail and `5.3` |
| A key account without a public key mailed | `imip::outbound`, mail to `imip-pending` |
| Mail submitted before the commit (create, update, DELETE; the REFRESH answer before its item's deletion) | `imip::outbound`, the update and single-event DELETE case whose store commit fails (the loser's message at the sink), the create's lost-lease case and the two-session REFRESH case (two answers at the sink), with the create, update and DELETE hook cases for path coverage |
| Mail sent for a chunked collection DELETE whose later chunk fails | `imip::outbound`, the `SECOND_CHUNK_FAIL_ONCE` case: mail at the sink |
| A local mailing list left `NotInternal` | `imip::outbound`, the list case: no mail and `5.3` |
| The token rule applied to mail recipients | `imip::outbound`, no REPLY to the outside organizer |
| The delegating merge mails outside delegates | `imip::inbound`, a second REQUEST at the sink, or `1.1` on the delegate's line |
| The organizer's-copy gate removed from the delegating fan-out | `imip::inbound`, the sub-delegate case: an item reaches the sub-delegate |
| `From` traced in the key branch | `imip::traces` |

### 8.7 Manual checklist: real providers

`docs/zero-access/manual-checklist.md` gains a section "Invitations by email
(release 5)", which Jay runs against real Gmail, Outlook.com and iCloud
accounts. Preconditions: a deployment those providers reach, outbound through
the relay with SPF and DKIM aligned for the key account's domain, and a key
account with Apple Calendar, Thunderbird or DAVx5 connected. Record client,
provider and versions, and what the client shows at once and after a refresh.
For each provider:

1. Add the provider's address as a contact (CardDAV), then invite the key
   account from the provider: the email reads and the event appears after the
   next refresh. From an address that is not a contact: the email only.
2. Accept, then decline, then answer tentative in the calendar client: the
   provider follows each.
3. The provider moves the event, changes its title, then cancels it: the
   client shows each change and the cancellation.
4. A weekly series from the provider; decline one occurrence: the provider
   shows it declined.
5. From the client, invite the provider's address: the invitation shows the
   provider's own buttons and no Stalwart buttons; record how the client
   shows `1.1`. Accept at the provider: the client shows accepted. Move, then
   delete, the event: the provider updates, then shows it cancelled.
6. Open an invitation in Apple Mail and Thunderbird: record whether the mail
   client offers to add the event; the calendar ends with one copy.
7. Invite the provider with a second key account: that account gets no email
   and sees the invitation in its calendar (release 2).
8. An event created before release 5 with the provider as attendee (`5.3`):
   add an alarm, and the provider gets nothing; move the event, and the
   provider gets the invitation (5.7).

Server-side, after the run: `za::za_tests` passes against the same build, the
SMTP queue is empty once the providers have accepted the mail, and the leak
scanner's output shows the key account's items and events sealed.

## 9. Invariants for implementers

Release 1's ten, release 2's five, key pinning's eight, mail's nine and
release 4's eight hold, with these amendments by name:

- **Release 1 section 9's** "Inbound iMIP ingest from mail" row, **release 2
  sections 1.2, 5.2 ("Plain organizers") and 8**, and **mail spec 6.1 step
  5**: superseded by section 4. The HTTP RSVP rows stand.
- **Release 2 invariant 12** gains one exception: the iMIP email of section 5
  to a mail-class recipient, plaintext in the SMTP queue (mail spec invariant
  28's queue exception) and at the recipient. Its task-queue part holds without
  exception.
- **Release 2 invariant 14** (when the drain deletes an item) gains one case:
  a mail-borne item upstream's ingest would not have kept (D7).
- **Release 1 section 2's integrity statement** (a store writer can corrupt
  but not read) gains the exception of Jay's decision 11: a forged mail-borne
  item can make the drain mail an event's current state (a REFRESH answer) or
  add delegates who receive later edits (section 2).

This release adds no invariant 9 exception: the extractions of 3.1 change no
ordinary account's behaviour, and every new branch is a key account's. It adds:

41. A key account's outbound iMIP message is composed in a request holding the
    sender's session keys and handed to the SMTP queue in that request; it is
    never written to the task queue or any store location before it, and
    carries no fork carrier, token or RSVP link.
42. Only the mail class is mailed (no local account, which includes local
    mailing lists, or a local account that is not a key account, which
    includes groups); an active local key account's address is never mailed,
    and is reached only through the sealed fan-out, whatever its state. A local
    list or group is mailed as upstream mails it; its key members receive the
    email sealed, with no calendar effect, unless the ORGANIZER is a group's
    address, whose invitation key members receive as from a plain local sender
    (5.1, a recorded consequence).
43. A mail-borne item is written only at delivery, for a sender upstream's
    `is_authenticated()` accepts and that is not a local key account, sealed
    to the served public key, with the sender only in `X-ZA-SENDER`. No field
    the fork writes holds the sender's address or principal id; the visible
    UID is the organizer's own choice and may embed either (section 2).
44. Delivery decides from the message and keyless indexes only (UID index,
    pending items' UIDs, the address book's email index, the directory); the
    drain repeats every check it can.
45. The mail arm applies upstream's merge with the carrier's sender, without
    release 2's stale-reply check (D6, withdrawn); it never authenticates by
    token, never adopts, replaces or removes a pair because of mail (an
    outside party supplies none), and ends each item as upstream's ingest
    leaves the inbox. The organizer-side delegating fan-out (D15) maintains
    the organizer's own pairs for new internal delegates, as release 2
    section 6.4 does. The release 2 arms are unchanged and never accept
    `X-ZA-SENDER`.
46. A mailed recipient's status is `1.1`, written before the commit; its mail
    is submitted only after that commit succeeds; a failed submission is
    traced and the status is not corrected; mail recipients are never pinned.
47. The fork's iMIP events carry no address or content; only the SMTP layer's
    envelope traces name a mailed invitation's addresses, and every
    transaction the outbound path opens is redacted as a key account's.

## 10. Merge hotspots and silent dependencies

Hotspots, fork insertions in upstream files: `crates/email/src/message/ingest.rs`
(the gate without `!za_key`, the call to `za_imip_item`);
`crates/services/src/task_manager/tasks/imip.rs` (the two extracted functions:
a change to the composer flows through, a change to its loops needs the
extraction redone); `crates/groupware/src/calendar/itip.rs` (the three
functions extracted from `itip_ingest`); `crates/services/src/lib.rs` (the
registration line); `crates/dav/src/calendar/update.rs` and `delete.rs`
(already merge hotspots: each submits the fan-out's returned mail attempts
after its commit returns `Ok`, and gains its commit-failure test hook);
`crates/common/src/za/mod.rs` (declares the new `imip` module);
`crates/store/src/dispatch/store.rs` (a test-mode line in `write_batch` for
`SECOND_CHUNK_FAIL_ONCE`, 8.2, beside the fork-owned `za_test.rs`).

Fork-owned files that change: `fanout.rs`, `drain.rs`, `seal/item.rs`, and the
new `crates/email/src/message/za/imip.rs`, `crates/common/src/za/imip.rs` and
`crates/services/src/za_imip.rs`.

Silent dependencies on upstream code:

- The mail arm mirrors `itip_ingest`'s order (UID lookup, auto-add, a
  REQUEST-only import, `MergeResult::Message` for REFRESH), as `import` and
  `merge` mirror `commit_itip_merge`: a change there needs a re-read.
- 4.1 step 3 relies on upstream's merge applying only REQUEST, ADD, CANCEL,
  REPLY and REFRESH (`crates/groupware/src/scheduling/inbound.rs:64-370`).
- The delivery sender (`ingest.rs:527-541`), `is_authenticated()` (set at
  `crates/smtp/src/inbound/data.rs:817-833`), `TaskCalendarItipContents` as
  `ItipMessages::new` fills it, and `send_imip`'s `account_id_from_email(from)`
  rule with `must_match_sender` are taken from upstream as they stand.
- `route`'s split by class relies on token authentication being the only
  reason for release 2's restrictions.
- D12's reuse of `za_key_rcpt` relies on a local session never calling
  `reset()` and on `data.rs:817` clearing it only after the DATA stage's last
  redaction read; an upstream change to either needs a re-read.

## 11. Open questions for Jay

None beyond the decisions above.
