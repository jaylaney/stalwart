/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    smtp::{
        inbound::{TestMessage, TestQueueEvent},
        session::{TestSession, VerifyResponse},
    },
    utils::server::TestServerBuilder,
};
use ahash::AHashSet;
use common::{
    config::smtp::queue::QueueName,
    ipc::{QueueEvent, QueueEventStatus},
};
use registry::{
    schema::structs::{
        Expression, ExpressionMatch, MtaDeliveryExpiration, MtaDeliveryExpirationTtl,
        MtaDeliverySchedule, MtaDeliveryScheduleInterval, MtaDeliveryScheduleIntervals,
        MtaDeliveryScheduleIntervalsOrDefault, MtaExtensions, MtaOutboundStrategy, MtaVirtualQueue,
    },
    types::list::List,
};
use smtp::queue::spool::{QUEUE_REFRESH, SmtpSpool};
use std::time::Duration;
use store::write::now;

#[tokio::test]
async fn queue_retry() {
    let mut local = TestServerBuilder::new("smtp_queue_retry")
        .await
        .with_http_listener(19041)
        .await
        .disable_services()
        .capture_queue()
        .build()
        .await;

    let local_admin = local.account("admin");
    local_admin.mta_allow_relaying().await;
    local_admin.mta_allow_non_fqdn().await;
    local_admin.mta_no_auth().await;
    local_admin
        .registry_create_object(MtaOutboundStrategy {
            schedule: Expression {
                match_: List::from_iter([ExpressionMatch {
                    if_: "sender_domain == 'test.org'".into(),
                    then: "'sender-test'".into(),
                }]),
                else_: "'sender-default'".into(),
            },
            ..Default::default()
        })
        .await;
    local_admin
        .registry_create_object(MtaExtensions {
            deliver_by: Expression {
                else_: "1h".into(),
                ..Default::default()
            },
            future_release: Expression {
                else_: "1h".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .await;
    let queue_id = local_admin
        .registry_create_object(MtaVirtualQueue {
            name: "default".into(),
            threads_per_node: 25,
            description: None,
        })
        .await;
    local_admin
        .registry_create_object(MtaDeliverySchedule {
            name: "sender-default".into(),
            retry: MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
                intervals: List::from_iter([
                    MtaDeliveryScheduleInterval {
                        duration: 1_000u64.into(),
                    },
                    MtaDeliveryScheduleInterval {
                        duration: 2_000u64.into(),
                    },
                    MtaDeliveryScheduleInterval {
                        duration: 3_000u64.into(),
                    },
                ]),
            }),
            notify: MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
                intervals: List::from_iter([MtaDeliveryScheduleInterval {
                    duration: (15 * 60 * 60 * 1000u64).into(),
                }]),
            }),
            expiry: MtaDeliveryExpiration::Ttl(MtaDeliveryExpirationTtl {
                expire: 86_400_000u64.into(),
            }),
            queue_id,
            description: None,
        })
        .await;
    local_admin
        .registry_create_object(MtaDeliverySchedule {
            name: "sender-test".into(),
            retry: MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
                intervals: List::from_iter([
                    MtaDeliveryScheduleInterval {
                        duration: 1_000u64.into(),
                    },
                    MtaDeliveryScheduleInterval {
                        duration: 2_000u64.into(),
                    },
                    MtaDeliveryScheduleInterval {
                        duration: 3_000u64.into(),
                    },
                ]),
            }),
            notify: MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
                intervals: List::from_iter([
                    MtaDeliveryScheduleInterval {
                        duration: 1_000u64.into(),
                    },
                    MtaDeliveryScheduleInterval {
                        duration: 2_000u64.into(),
                    },
                ]),
            }),
            expiry: MtaDeliveryExpiration::Ttl(MtaDeliveryExpirationTtl {
                expire: 6_000u64.into(),
            }),
            queue_id,
            description: None,
        })
        .await;
    local_admin.reload_settings().await;
    local.reload_core();
    local.expect_reload_settings().await;

    let mut session = local.new_mta_session();
    session.data.remote_ip_str = "10.0.0.1".into();
    session.eval_session_params().await;
    session.ehlo("mx.test.org").await;
    session
        .send_message("john@test.org", &["bill@foobar.org"], "test:no_dkim", "250")
        .await;
    let attempt = local.expect_message_for_queue_then_deliver("default").await;

    // Expect a failed DSN
    attempt.try_deliver(local.server.clone());
    let message = local.expect_message().await;
    assert_eq!(message.message.return_path.as_ref(), "");
    assert_eq!(
        message.message.recipients.first().unwrap().address(),
        "john@test.org"
    );
    message
        .read_lines(&local)
        .await
        .assert_contains("Content-Type: multipart/report")
        .assert_contains("Final-Recipient: rfc822;bill@foobar.org")
        .assert_contains("Action: failed");
    local.read_event().await.assert_done();
    local.clear_queue().await;

    // Expect a failed DSN for foobar.org, followed by two delayed DSN and
    // a final failed DSN for _dns_error.org.
    session
        .send_message(
            "john@test.org",
            &["bill@foobar.org", "jane@_dns_error.org"],
            "test:no_dkim",
            "250",
        )
        .await;
    let mut in_fight = AHashSet::new();
    let attempt = local.expect_message_for_queue_then_deliver("default").await;
    let mut dsn = Vec::new();
    // Each retry is checked against the queue's own clock read (issue #107).
    // The queue sets a recipient's retry due time to `now() + interval` in the
    // delivery task, and `now()` has whole-second resolution, so a second
    // boundary between that read and the test's own makes a reading taken
    // later than the attempt come out short. Read the clock immediately
    // before each attempt starts and immediately after its WorkerDone arrives
    // (the stored recipient is read after it): the queue's read falls between
    // the two, so `start + interval <= due <= finish + interval` holds on any
    // sub-second phase.
    let original_id = attempt.queue_id;
    let mut attempt_start = now();
    let mut retry_records: Vec<(u32, u64, u64, u64)> = Vec::new();
    let mut last_retry = 0;
    in_fight.insert(attempt.queue_id);
    attempt.try_deliver(local.server.clone());

    loop {
        match local.try_read_event().await {
            Some(QueueEvent::WorkerDone {
                queue_id, status, ..
            }) => {
                in_fight.remove(&queue_id);
                match &status {
                    QueueEventStatus::Completed | QueueEventStatus::Deferred => (),
                    _ => panic!("unexpected status {queue_id}: {status:?}"),
                }
                if queue_id == original_id {
                    let attempt_finish = now();
                    // None after the final, expired attempt: the message is gone.
                    if let Some(message) = local
                        .server
                        .read_message(queue_id, QueueName::default())
                        .await
                        .unwrap()
                    {
                        let retry = &message
                            .message
                            .recipients
                            .iter()
                            .find(|rcpt| rcpt.address() == "jane@_dns_error.org")
                            .unwrap()
                            .retry;
                        // An attempt that only sent a notification leaves
                        // `inner` unchanged and records nothing.
                        if retry.inner > last_retry {
                            last_retry = retry.inner;
                            retry_records.push((
                                retry.inner,
                                retry.due,
                                attempt_start,
                                attempt_finish,
                            ));
                        }
                    }
                }
            }
            Some(QueueEvent::Refresh(_)) | Some(QueueEvent::ReloadSettings) => (),
            None | Some(QueueEvent::Stop) | Some(QueueEvent::Paused(_)) => break,
        }

        let now = now();
        let mut events = local.all_queued_messages().await;
        if events.messages.is_empty() {
            if events.next_refresh < now + QUEUE_REFRESH {
                tokio::time::sleep(Duration::from_secs(events.next_refresh - now)).await;
                events = local.all_queued_messages().await;
            } else if in_fight.is_empty() {
                break;
            }
        }

        for event in events.messages {
            if in_fight.contains(&event.queue_id) {
                continue;
            }

            let message = local
                .server
                .read_message(event.queue_id, QueueName::default())
                .await
                .unwrap()
                .unwrap();
            if message.message.return_path.is_empty() {
                message
                    .clone()
                    .remove(&local.server, event.due.into())
                    .await;
                dsn.push(message);
            } else {
                in_fight.insert(event.queue_id);
                // `now` is shadowed by this pass's reading above.
                attempt_start = store::write::now();
                event.try_deliver(local.server.clone());
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    local.assert_queue_is_empty().await;
    assert_eq!(
        retry_records
            .iter()
            .map(|(inner, ..)| *inner)
            .collect::<Vec<_>>(),
        vec![1, 2, 3],
        "retry records (inner, due, start, finish): {retry_records:?}"
    );
    for &(inner, due, start, finish) in &retry_records {
        let interval = [1u64, 2, 3][inner as usize - 1];
        assert!(
            start + interval <= due && due <= finish + interval,
            "retry {inner} due {due} outside [{start} + {interval}, {finish} + {interval}]; \
             retry records (inner, due, start, finish): {retry_records:?}"
        );
    }
    assert_eq!(dsn.len(), 4);
    let mut dsn = dsn.into_iter();

    dsn.next()
        .unwrap()
        .read_lines(&local)
        .await
        .assert_contains("<bill@foobar.org> (failed to lookup 'foobar.org'")
        .assert_contains("Final-Recipient: rfc822;bill@foobar.org")
        .assert_contains("Action: failed");

    dsn.next()
        .unwrap()
        .read_lines(&local)
        .await
        .assert_contains("<jane@_dns_error.org> (failed to lookup '_dns_error.org'")
        .assert_contains("Final-Recipient: rfc822;jane@_dns_error.org")
        .assert_contains("Action: delayed");

    dsn.next()
        .unwrap()
        .read_lines(&local)
        .await
        .assert_contains("<jane@_dns_error.org> (failed to lookup '_dns_error.org'")
        .assert_contains("Final-Recipient: rfc822;jane@_dns_error.org")
        .assert_contains("Action: delayed");

    dsn.next()
        .unwrap()
        .read_lines(&local)
        .await
        .assert_contains("<jane@_dns_error.org> (failed to lookup '_dns_error.org'")
        .assert_contains("Final-Recipient: rfc822;jane@_dns_error.org")
        .assert_contains("Action: failed");

    // Test FUTURERELEASE + DELIVERBY (RETURN)
    session.data.remote_ip_str = "10.0.0.2".into();
    session.eval_session_params().await;
    session
        .send_message(
            "<bill@foobar.org> HOLDFOR=60 BY=3600;R",
            &["john@test.net"],
            "test:no_dkim",
            "250",
        )
        .await;
    let now_ = now();
    let message = local.expect_message().await;
    assert!([59, 60].contains(&(local.message_due(message.queue_id).await - now_)));
    assert!([59, 60].contains(&(message.message.next_delivery_event(None).unwrap() - now_)));
    assert!(
        [3599, 3600].contains(
            &(message
                .message
                .recipients
                .first()
                .unwrap()
                .expiration_time(message.message.created)
                .unwrap()
                - now_)
        )
    );
    assert!(
        [54059, 54060].contains(&(message.message.recipients.first().unwrap().notify.due - now_)),
        "diff: {}",
        message.message.recipients.first().unwrap().notify.due - now_
    );

    // Test DELIVERBY (NOTIFY)
    session
        .send_message(
            "<bill@foobar.org> BY=3600;N",
            &["john@test.net"],
            "test:no_dkim",
            "250",
        )
        .await;
    let schedule = local.expect_message().await;
    assert!(
        [3599, 3600].contains(&(schedule.message.recipients.first().unwrap().notify.due - now())),
        "diff: {}",
        schedule.message.recipients.first().unwrap().notify.due - now()
    );
}
