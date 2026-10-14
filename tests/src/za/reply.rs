/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 5.5: an attendee's own PUT replies to the organizer
//! with its visible token, only to the ORGANIZER, and only when its copy
//! holds its pair; an attendee-originated delegate invitation is not
//! attempted.

use super::{
    STRONG,
    fanout::{CONTENT_TYPE, NO_REPLY, Party, stored_pairs},
    user_permissions,
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        inbox_ids, item_method, item_partstat, new_inbox_items, open_pending_item_as,
        plant_attendee_copy, schedule_statuses, za_setup_token, za_unlock,
    },
};
use ::registry::{
    schema::{
        enums::StorageQuota,
        prelude::{ObjectType, Property},
    },
    types::EnumImpl,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
use groupware::calendar::{
    EVENT_NOTIFICATION_ZA_PENDING,
    seal::{RSVP_TOKEN_PROP, TOKEN_LEN, TokenPairs},
};
use hyper::StatusCode;
use serde_json::json;

pub const KEY2_CAL: &str = "/dav/cal/key2%40example.com/default/";
const TOKEN: [u8; TOKEN_LEN] = *b"fo-reply-token01";
const KEY2_NEEDS: &str = "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com";
const KEY2_ACCEPTED: &str = "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:key2@example.com";

/// An invitation as key2's copy holds it. `attendees` are whole ATTENDEE
/// lines.
pub fn copy(uid: &str, organizer: &str, attendees: &[&str]) -> String {
    let mut ical = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990401T100000Z\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:fo-reply-title\r\n",
            "ORGANIZER:mailto:{organizer}\r\n"
        ),
        uid = uid,
        organizer = organizer
    );
    for attendee in attendees {
        ical.push_str(attendee);
        ical.push_str("\r\n");
    }
    ical.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ical
}

/// The SCHEDULE-STATUS values of the ORGANIZER property in key2's copy.
pub async fn organizer_statuses(
    client: &DummyWebDavClient,
    path: &str,
    organizer: &str,
) -> Vec<String> {
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    schedule_statuses(&body, ICalendarProperty::Organizer, organizer)
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access reply and delegation tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let start1 = inbox_ids(test, key1.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let mut pairs = TokenPairs::new();
    pairs.insert("key2@example.com", TOKEN);

    // 3. key2's copy holds its pair; key2 accepts. key1's inbox gets one
    // pending REPLY whose visible token is key2's.
    let reply = format!("{KEY2_CAL}fo-reply.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-reply.ics",
        &copy("za-fo-reply", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    key2.client
        .request_with_headers(
            "PUT",
            &reply,
            [CONTENT_TYPE],
            copy("za-fo-reply", "key1@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key1.id, &start1).await;
    assert_eq!(items.len(), 1, "key1's inbox holds one reply");
    let (item_id, item) = &items[0];
    assert_ne!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    let visible = item.event.components[0]
        .entries
        .iter()
        .find(|entry| {
            matches!(&entry.name, ICalendarProperty::Other(name) if name.eq_ignore_ascii_case(RSVP_TOKEN_PROP))
        })
        .and_then(|entry| entry.values.first())
        .and_then(|value| value.as_text())
        .map(str::to_string);
    assert_eq!(
        visible,
        Some(URL_SAFE_NO_PAD.encode(TOKEN)),
        "the visible token"
    );
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, *item_id).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(
        item_partstat(&tree, "key2@example.com").as_deref(),
        Some("ACCEPTED")
    );
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
    assert!(carriers.pair.is_none());
    assert_eq!(
        organizer_statuses(&key2.client, &reply, "key1@example.com").await,
        ["1.2"]
    );
    // An attendee's rewrite keeps its stored pairs: `token_union` runs only
    // for the organizer.
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let kept = stored_pairs(test, &keys2, key2.id, "default/fo-reply.ics")
        .await
        .expect("key2's copy keeps its pair");
    assert_eq!(kept.len(), 1, "an attendee's rewrite handed out a token");
    assert_eq!(kept.get("key2@example.com"), Some(&TOKEN));
    // The attendee's DELETE of its copy yields upstream's declined REPLY,
    // delivered the same way and with the same token (spec 5.5).
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request("DELETE", &reply, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key1.id, &before1).await;
    assert_eq!(items.len(), 1, "one declined REPLY");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(
        item_partstat(&tree, "key2@example.com").as_deref(),
        Some("DECLINED")
    );
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));

    // A REPLY that fails at the ORGANIZER (spec 5.3 step 5): key1's quota
    // cannot hold the item. key2's copy records 5.1 on the ORGANIZER and
    // nothing reaches key1. 4(a) below delivers a REPLY to key1 again, which
    // proves the restore.
    let admin = test.account("admin@example.com").clone();
    let key1_account = test.account("key1@example.com").id();
    let over_quota = format!("{KEY2_CAL}fo-reply-quota.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-reply-quota.ics",
        &copy("za-fo-reply-quota", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    admin
        .registry_update_object(
            ObjectType::Account,
            key1_account,
            json!({ Property::Quotas: { StorageQuota::MaxDiskQuota.as_str(): 1 } }),
        )
        .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &over_quota,
            [CONTENT_TYPE],
            copy("za-fo-reply-quota", "key1@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        organizer_statuses(&key2.client, &over_quota, "key1@example.com").await,
        ["5.1"],
        "a failed reply was not recorded on the ORGANIZER"
    );
    assert_eq!(
        inbox_ids(test, key1.id).await,
        before1,
        "a reply reached key1 past its quota"
    );
    admin
        .registry_update_object(
            ObjectType::Account,
            key1_account,
            json!({ Property::Quotas: {} }),
        )
        .await;

    // A copy whose bundle holds no pair for key2: the PUT succeeds, nothing
    // is delivered (the organizer's drain would drop it as a forgery), and
    // the ORGANIZER property says not attempted.
    let nopair = format!("{KEY2_CAL}fo-nopair.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-nopair.ics",
        &copy("za-fo-nopair", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &nopair,
            [CONTENT_TYPE],
            copy("za-fo-nopair", "key1@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        inbox_ids(test, key1.id).await,
        before1,
        "a reply without a token went out"
    );
    assert_eq!(
        organizer_statuses(&key2.client, &nopair, "key1@example.com").await,
        ["5.3"]
    );

    // 4(a). key2 delegates to key3: key1 gets the REPLY only; key2's
    // attendee-originated REQUEST to key3 is not attempted.
    let delegate = format!("{KEY2_CAL}fo-delegate.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-delegate.ics",
        &copy("za-fo-delegate", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &delegate,
            [CONTENT_TYPE],
            copy(
                "za-fo-delegate",
                "key1@example.com",
                &[
                    "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\":mailto:key2@example.com",
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        inbox_ids(test, key3.id).await,
        start3,
        "key2's delegate invitation reached key3"
    );
    let items = new_inbox_items(test, key1.id, &before1).await;
    assert_eq!(items.len(), 1, "key1 gets the REPLY");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(
        item_partstat(&tree, "key2@example.com").as_deref(),
        Some("DELEGATED")
    );
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
    let body = key2
        .client
        .request("GET", &delegate, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(
        schedule_statuses(&body, ICalendarProperty::Attendee, "key3@example.com"),
        ["5.3"]
    );

    // 4(b). key2 is key3's delegate and accepts: upstream addresses the
    // REPLY to key1 and to the delegator key3; only the ORGANIZER gets it.
    let delegated = format!("{KEY2_CAL}fo-delegated.ics");
    let delegator = "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key2@example.com\":mailto:key3@example.com";
    plant_attendee_copy(
        test,
        key2.id,
        "fo-delegated.ics",
        &copy(
            "za-fo-delegated",
            "key1@example.com",
            &[
                delegator,
                "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key3@example.com\":mailto:key2@example.com",
            ],
        ),
        STRONG,
        &pairs,
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &delegated,
            [CONTENT_TYPE],
            copy(
                "za-fo-delegated",
                "key1@example.com",
                &[
                    delegator,
                    "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE;DELEGATED-FROM=\"mailto:key3@example.com\":mailto:key2@example.com",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        inbox_ids(test, key3.id).await,
        start3,
        "an item reached the delegator key3"
    );
    let items = new_inbox_items(test, key1.id, &before1).await;
    assert_eq!(items.len(), 1, "key1 gets the REPLY");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(
        item_partstat(&tree, "key2@example.com").as_deref(),
        Some("ACCEPTED")
    );
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
    let body = key2
        .client
        .request("GET", &delegated, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(
        schedule_statuses(&body, ICalendarProperty::Attendee, "key3@example.com"),
        ["5.3"]
    );

    // Review Focus 5: a reply to an organizer whose vault record has no
    // public key yet (setup pending).
    let key11 = admin
        .create_passwordless_user_account(
            "key11@example.com",
            STRONG,
            "Key Eleven",
            &[],
            user_permissions(),
        )
        .await;
    za_setup_token(&admin, "key11@example.com").await;
    let key11_id = key11.id().document_id();
    assert!(
        test.server.za_is_key_account(key11_id).await.unwrap(),
        "key11 is not a key account"
    );
    let pending = format!("{KEY2_CAL}fo-pending.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-pending.ics",
        &copy("za-fo-pending", "key11@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    key2.client
        .request_with_headers(
            "PUT",
            &pending,
            [CONTENT_TYPE],
            copy("za-fo-pending", "key11@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        inbox_ids(test, key11_id).await.is_empty(),
        "an item reached an account without a public key"
    );
    assert_eq!(
        organizer_statuses(&key2.client, &pending, "key11@example.com").await,
        ["5.3"]
    );

    // Cleanup.
    for name in [
        "fo-reply-quota",
        "fo-nopair",
        "fo-delegate",
        "fo-delegated",
        "fo-pending",
    ] {
        key2.client
            .request_with_headers("DELETE", &format!("{KEY2_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    admin.destroy_account(key11).await;
    test.wait_for_tasks().await;
}
