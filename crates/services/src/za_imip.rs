/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork, release 5 (iMIP spec 3.1 item 1, 5.3): the submitter
//! that mails a key account's scheduling messages. It reuses upstream's
//! composer as extracted from the task's `send_imip`
//! (`task_manager::tasks::imip`: `imip_context`, `ImipContext::composer`,
//! `ImipComposer::submit`, `compose_imip_message`), called with the quiet
//! sender lookup, RSVP links off and the session's redaction flag on, so the
//! email is upstream's minus the RSVP block (D11, D12).

use crate::task_manager::tasks::imip::{ImipSubmitOptions, SenderLookup, imip_context};
use common::{
    Server,
    config::server::{DEFAULT_TLS_TIMEOUT, ServerProtocol},
    network::{ServerInstance, TcpAcceptor, limiter::ConcurrencyLimiter},
    za::imip::{ZA_IMIP_FAILED, ZaImipMail, ZaImipOutcome, za_register_imip_submitter},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
use tokio::sync::watch;
use utils::snowflake::SnowflakeIdGenerator;

/// Registers `submit` as the process's iMIP submitter (spec 5.3); called
/// from `spawn_services`, which the binary and the test harness run.
pub fn register() {
    za_register_imip_submitter(submit);
}

fn submit(server: Server, mail: ZaImipMail) -> Pin<Box<dyn Future<Output = ZaImipOutcome> + Send>> {
    Box::pin(async move { submit_one(&server, mail).await })
}

/// The dummy server instance of the local SMTP sessions, built as the task
/// manager builds its own (`task_manager::manager::spawn_task_manager`);
/// its `span_id_gen` gives each submission a session id of its own.
fn instance() -> Arc<ServerInstance> {
    static INSTANCE: OnceLock<Arc<ServerInstance>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            Arc::new(ServerInstance {
                id: "_local".to_string(),
                protocol: ServerProtocol::Smtp,
                acceptor: TcpAcceptor::Plain,
                limiter: ConcurrencyLimiter::new(100),
                tls_timeout: DEFAULT_TLS_TIMEOUT,
                shutdown_rx: watch::channel(false).1,
                proxy_networks: vec![],
                span_id_gen: Arc::new(SnowflakeIdGenerator::new()),
            })
        })
        .clone()
}

/// Upstream's composer for one (message, recipient): the setup with the
/// sender's domain (as `send_imip` derives it), the per-message step with
/// the quiet sender lookup (D17), and the per-recipient submission with RSVP
/// links off and the session's redaction flag on (D12). The SMTP response
/// text is not kept: only its stage and reply code.
async fn submit_one(server: &Server, mail: ZaImipMail) -> ZaImipOutcome {
    let ZaImipMail {
        account_id,
        document_id,
        contents,
    } = mail;
    let mut recipients = contents.to.iter();
    let (Some(recipient), None) = (recipients.next(), recipients.next()) else {
        return failed(trc::StoreEvent::UnexpectedError.into_err(), account_id);
    };
    let sender_domain = contents.from.rsplit('@').next().unwrap_or("localhost");

    let context = match imip_context(server, account_id, document_id, sender_domain).await {
        Ok(context) => context,
        Err(err) => return failed(err, account_id),
    };
    let Some(mut composer) = context
        .composer(server, &contents, SenderLookup::Quiet)
        .await
    else {
        return failed(trc::StoreEvent::UnexpectedError.into_err(), account_id);
    };
    let instance = instance();
    let session_id = instance.span_id_gen.generate();
    match composer
        .submit(
            server,
            &context,
            instance,
            recipient.as_str(),
            ImipSubmitOptions {
                rsvp: false,
                za_redact: true,
                session_id,
            },
        )
        .await
    {
        Ok(Ok(queue_id)) => ZaImipOutcome::Queued(queue_id),
        Ok(Err(rejection)) => ZaImipOutcome::Refused {
            stage: rejection.stage,
            code: reply_code(&rejection.response),
        },
        Err(err) => failed(err, account_id),
    }
}

/// A context, composer or submission error: traced with the account id and
/// the fixed details, never the message or an address.
fn failed(err: trc::Error, account_id: u32) -> ZaImipOutcome {
    trc::error!(
        err.account_id(account_id)
            .caused_by(trc::location!())
            .details(ZA_IMIP_FAILED)
    );
    ZaImipOutcome::Failed
}

/// The reply code of an SMTP response: its leading three digits, else 0.
pub(crate) fn reply_code(response: &str) -> u16 {
    response
        .get(..3)
        .filter(|code| code.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::reply_code;
    use crate::task_manager::tasks::imip::{ItipTemplate, RenderedBody, compose_imip_message};
    use calcard::icalendar::ICalendarProperty;
    use common::DEFAULT_LOGO_BASE64;
    use groupware::scheduling::{ItipField, ItipSummary, ItipValue, format::TextFormatter};
    use mail_builder::{
        MessageBuilder,
        headers::{HeaderType, content_type::ContentType},
        mime::{BodyPart, MimePart},
    };
    use utils::template::Template;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:imip-unit-1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:Sync\r\nORGANIZER;CN=Org:mailto:imip-org@remote.test\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    /// Upstream's default logo part (`send_imip`'s `else` branch, the one
    /// every build without an enterprise logo takes), under a pinned CID.
    fn default_logo(cid: &str) -> MimePart<'static> {
        MimePart::new(
            ContentType::new("image/png"),
            BodyPart::Binary(DEFAULT_LOGO_BASE64.as_bytes().into()),
        )
        .transfer_encoding("base64")
        .inline()
        .cid(cid.to_string())
    }

    fn summary_field(summary: &str) -> ItipField {
        ItipField {
            name: ICalendarProperty::Summary,
            value: ItipValue::Text(summary.to_string()),
        }
    }

    /// v1.0.1's `imip.rs:192-243`, the `MessageBuilder` expression of
    /// `send_imip`'s per-recipient loop, copied verbatim from the tag. The
    /// shims below give its free names (`sender_info`, `itip_message`,
    /// `recipient`, `server`, `template`, `summary`) the values the
    /// parameters carry, so the expression itself is not edited.
    #[allow(clippy::too_many_arguments, clippy::let_and_return)]
    fn upstream_builder(
        sender_name: &str,
        from: &str,
        to: &str,
        message_id: String,
        subject: &str,
        body: &RenderedBody,
        method: &str,
        i_calendar_data: &str,
        logo: MimePart<'static>,
    ) -> Vec<u8> {
        struct SenderInfo<'a>(&'a str);
        impl SenderInfo<'_> {
            fn description(&self) -> Option<&str> {
                Some(self.0)
            }
            fn name(&self) -> &str {
                self.0
            }
        }
        struct ItipMessage {
            from: String,
            i_calendar_data: String,
        }
        struct Network(String);
        impl Network {
            fn message_id(&self) -> String {
                self.0.clone()
            }
        }
        struct Core {
            network: Network,
        }
        struct Server {
            core: Core,
        }
        struct UpstreamTemplate {
            subject: String,
        }
        struct Summary<'a>(&'a str);
        impl Summary<'_> {
            fn method(&self) -> &str {
                self.0
            }
        }
        let sender_info = SenderInfo(sender_name);
        let itip_message = ItipMessage {
            from: from.to_string(),
            i_calendar_data: i_calendar_data.to_string(),
        };
        let recipient = to.to_string();
        let server = Server {
            core: Core {
                network: Network(message_id),
            },
        };
        let template = UpstreamTemplate {
            subject: subject.to_string(),
        };
        let summary = Summary(method);

        // The tag's expression, unedited.
        let message = MessageBuilder::new()
            .from((
                sender_info.description().unwrap_or(sender_info.name()),
                itip_message.from.as_str(),
            ))
            .to(recipient.as_str())
            .header("Auto-Submitted", HeaderType::Text("auto-generated".into()))
            .header(
                "Reply-To",
                HeaderType::Text(itip_message.from.as_str().into()),
            )
            .message_id(server.core.network.message_id())
            .subject(template.subject.as_str())
            .body(MimePart::new(
                ContentType::new("multipart/mixed"),
                BodyPart::Multipart(vec![
                    MimePart::new(
                        ContentType::new("multipart/related"),
                        BodyPart::Multipart(vec![
                            MimePart::new(
                                ContentType::new("multipart/alternative"),
                                BodyPart::Multipart(vec![
                                    MimePart::new(
                                        ContentType::new("text/plain"),
                                        BodyPart::Text(body.text.as_str().into()),
                                    ),
                                    MimePart::new(
                                        ContentType::new("text/html"),
                                        BodyPart::Text(body.html.as_str().into()),
                                    ),
                                    MimePart::new(
                                        ContentType::new("text/calendar")
                                            .attribute("method", summary.method())
                                            .attribute("charset", "utf-8"),
                                        BodyPart::Text(
                                            itip_message.i_calendar_data.as_str().into(),
                                        ),
                                    ),
                                ]),
                            ),
                            logo.clone(),
                        ]),
                    ),
                    MimePart::new(
                        ContentType::new("application/ics").attribute("name", "event.ics"),
                        BodyPart::Text(itip_message.i_calendar_data.as_str().into()),
                    )
                    .attachment("event.ics"),
                ]),
            ))
            .write_to_vec()
            .unwrap_or_default();
        message
    }

    /// Replaces the generated values, the Date header and the three MIME
    /// boundaries (`multipart/mixed`, `related`, `alternative`), by
    /// placeholders numbered in order of appearance, and asserts that each
    /// was found, so the comparison cannot pass on a message that lost them.
    fn normalize(message: &[u8]) -> String {
        let text = String::from_utf8_lossy(message);
        let mut dates = 0;
        let mut out = text
            .split("\r\n")
            .map(|line| {
                if line.starts_with("Date: ") {
                    dates += 1;
                    "Date: <date>"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(dates, 1, "one Date header");

        let mut boundaries = Vec::new();
        let mut rest = out.as_str();
        while let Some(at) = rest.find("boundary=\"") {
            rest = &rest[at + "boundary=\"".len()..];
            let end = rest.find('"').expect("a closed boundary parameter");
            boundaries.push(rest[..end].to_string());
            rest = &rest[end..];
        }
        assert_eq!(boundaries.len(), 3, "three multipart boundaries");
        for (n, boundary) in boundaries.iter().enumerate() {
            assert!(!boundary.is_empty());
            out = out.replace(boundary.as_str(), &format!("<boundary-{n}>"));
        }
        out
    }

    /// Spec 8.1: the extracted composer writes the bytes v1.0.1's `send_imip`
    /// wrote. `upstream_builder` is the tag's `imip.rs:192-243` expression copied
    /// verbatim into the test; `normalize` replaces MIME boundaries and the Date
    /// header by placeholders in both; Message-ID and the logo CID are pinned.
    #[test]
    fn the_extracted_composer_writes_the_tasks_bytes() {
        let body = RenderedBody {
            html: "<p>Sync</p>".into(),
            text: "Sync".into(),
        };
        let logo = || default_logo("logo.1@remote.test"); // upstream's DEFAULT_LOGO_BASE64 part
        let ours = compose_imip_message(
            "Org",
            "imip-org@remote.test",
            "imip-att@remote.test",
            "pinned@example.com".into(),
            "Invitation: Sync",
            &body,
            "REQUEST",
            ICS,
            logo(),
        );
        let upstream = upstream_builder(
            "Org",
            "imip-org@remote.test",
            "imip-att@remote.test",
            "pinned@example.com".into(),
            "Invitation: Sync",
            &body,
            "REQUEST",
            ICS,
            logo(),
        );
        assert_eq!(normalize(&ours), normalize(&upstream));
        let text = String::from_utf8_lossy(&ours);
        // mail-builder quotes every Content-Type parameter (`write_parameter`).
        assert!(text.contains("method=\"REQUEST\"") && text.contains("event.ics"));
        assert!(text.contains("Auto-Submitted: auto-generated"));
        assert!(text.contains("<pinned@example.com>") && text.contains("logo.1@remote.test"));
    }

    /// Spec 5.3, 8.1: a body rendered without `set_recipient` has no RSVP block.
    #[test]
    fn a_template_without_rsvp_renders_no_rsvp_block() {
        let template = Template::parse(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-invite.html.min"
        )))
        .unwrap();
        let formatter = TextFormatter::new("en").unwrap();
        let summary = ItipSummary::Invite(vec![summary_field("Sync")]);
        let rendered = ItipTemplate::from_template(
            &template,
            &formatter,
            "imip-org@remote.test",
            &summary,
            "logo.1@remote.test",
        )
        .render();
        assert!(rendered.html.contains("Sync"), "the template rendered");
        let reply_as = formatter
            .locale
            .calendar_reply_as
            .replace("$name", "imip-att@remote.test");
        assert!(!rendered.html.contains(&reply_as) && !rendered.html.contains("?i="));
    }

    #[test]
    fn reply_codes_are_read_from_the_response() {
        assert_eq!(reply_code("550 5.1.2 Mailbox does not exist."), 550);
        assert_eq!(reply_code("451 4.3.0 try later"), 451);
        assert_eq!((reply_code(""), reply_code("abc")), (0, 0));
    }
}
