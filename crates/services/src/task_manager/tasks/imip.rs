/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::task_manager::{
    outcome::{Failure, Outcome, RetryClass},
    tasks::{SmtpRejection, append_error},
};
use calcard::icalendar::{ICalendarParticipationStatus, ICalendarProperty};
use common::{
    DEFAULT_LOGO_BASE64, Server,
    auth::AccountInfo,
    config::groupware::CalendarTemplateVariable,
    i18n::Locale,
    network::{ServerInstance, stream::NullIo},
};
use groupware::{
    calendar::itip::ItipIngest,
    scheduling::{
        ItipSummary, ItipValue,
        format::{DateStyle, TextFormatter, hyperlink},
    },
};
use mail_builder::{
    MessageBuilder,
    headers::{HeaderType, content_type::ContentType},
    mime::{BodyPart, MimePart},
};
use mail_parser::decoders::html::html_to_text;
use registry::{
    schema::structs::{Task, TaskCalendarItipContents, TaskCalendarItipMessage},
    types::{EnumImpl, list::List, map::Map},
};
use smtp::{
    core::{Session, SessionData},
    inbound::data::MessageOrigin,
};
use smtp_proto::{MailFrom, RcptTo};
use std::{borrow::Cow, sync::Arc, time::Duration};
use store::{ahash::AHashMap, write::now};
use trc::AddContext;
use utils::template::{Template, Variable, Variables};

pub(crate) trait SendImipTask: Sync + Send {
    fn send_imip(
        &self,
        task: &TaskCalendarItipMessage,
        server_instance: Arc<ServerInstance>,
    ) -> impl Future<Output = Outcome> + Send;
}

impl SendImipTask for Server {
    async fn send_imip(
        &self,
        task: &TaskCalendarItipMessage,
        server_instance: Arc<ServerInstance>,
    ) -> Outcome {
        match send_imip(self, task, server_instance).await {
            Ok(outcome) => outcome,
            Err(err) => {
                let result = Outcome::error(&err);
                trc::error!(
                    err.account_id(task.account_id.document_id())
                        .document_id(task.document_id.document_id())
                        .caused_by(trc::location!())
                        .details("Failed to send iMIP message")
                );
                result
            }
        }
    }
}

async fn send_imip(
    server: &Server,
    imip: &TaskCalendarItipMessage,
    server_instance: Arc<ServerInstance>,
) -> trc::Result<Outcome> {
    // Obtain iMIP payload
    let account_id = imip.account_id.document_id();
    let document_id = imip.document_id.document_id();

    let sender_domain = imip
        .messages
        .iter()
        .next()
        .and_then(|msg| msg.from.rsplit('@').next())
        .unwrap_or("localhost");

    // Zero-access fork (iMIP spec 3.1 item 1): the setup, the per-message
    // step and the per-recipient submission are the functions below, called
    // here with upstream's arguments; this body keeps the loop, the events
    // and the outcome.
    let context = imip_context(server, account_id, document_id, sender_domain).await?;
    let mut pending = List::default();
    let mut errors = String::new();

    for itip_message in imip.messages.iter() {
        let Some(mut composer) = context
            .composer(server, itip_message, SenderLookup::Traced)
            .await
        else {
            return Ok(Outcome::fatal("Failed to parse iMIP message summary."));
        };
        let mut retry_to = Vec::new();

        for recipient in itip_message.to.iter() {
            let result = composer
                .submit(
                    server,
                    &context,
                    server_instance.clone(),
                    recipient.as_str(),
                    ImipSubmitOptions {
                        rsvp: true,
                        za_redact: false,
                        session_id: 0,
                    },
                )
                .await?;

            match result {
                Ok(queue_id) => {
                    trc::event!(
                        Calendar(trc::CalendarEvent::ItipMessageSent),
                        From = itip_message.from.to_string(),
                        To = recipient.to_string(),
                        AccountId = account_id,
                        DocumentId = document_id,
                        QueueId = queue_id,
                    );
                }
                Err(rejection) => {
                    // Upstream's key order differs between the envelope
                    // stages and DATA; both are kept.
                    let is_data = rejection.stage == "DATA";
                    let rejection = rejection.into_smtp_rejection();
                    if is_data {
                        trc::event!(
                            Calendar(trc::CalendarEvent::ItipMessageError),
                            From = itip_message.from.to_string(),
                            To = recipient.to_string(),
                            AccountId = account_id,
                            DocumentId = document_id,
                            Reason = rejection.reason.clone(),
                        );
                    } else {
                        trc::event!(
                            Calendar(trc::CalendarEvent::ItipMessageError),
                            AccountId = account_id,
                            DocumentId = document_id,
                            From = itip_message.from.to_string(),
                            To = recipient.to_string(),
                            Reason = rejection.reason.clone(),
                        );
                    }

                    if rejection.is_temporary {
                        append_error(
                            &mut errors,
                            format_args!("{recipient}: {}", rejection.reason),
                        );
                        retry_to.push(recipient.clone());
                    }
                }
            }
        }

        if !retry_to.is_empty() {
            pending.push(TaskCalendarItipContents {
                from: itip_message.from.clone(),
                to: Map::new(retry_to),
                is_from_organizer: itip_message.is_from_organizer,
                i_calendar_data: itip_message.i_calendar_data.clone(),
                summary: itip_message.summary.clone(),
            });
        }
    }

    if pending.is_empty() {
        Ok(Outcome::done())
    } else {
        Ok(Outcome::Partial {
            remaining: Box::new(Task::CalendarItipMessage(TaskCalendarItipMessage {
                messages: pending,
                account_id: imip.account_id,
                document_id: imip.document_id,
                status: imip.status.clone(),
            })),
            failure: Failure::new(RetryClass::Transient, errors),
        })
    }
}

/// Zero-access fork (iMIP spec 3.1 item 1): `send_imip`'s task-level setup,
/// the logo part under its CID, the event account's info and its formatter.
pub(crate) struct ImipContext {
    pub(crate) logo: MimePart<'static>,
    pub(crate) logo_cid: String,
    pub(crate) account_info: AccountInfo,
    pub(crate) formatter: TextFormatter,
    pub(crate) account_id: u32,
    pub(crate) document_id: u32,
}

/// How the per-message step resolves the sender's account: `Traced` is
/// upstream's `account_id_from_email(from, true)`; `Quiet` is the fork's
/// `za_account_id_from_email(from)`, the same lookup without the cache
/// events that carry the address (iMIP spec D17).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SenderLookup {
    Traced,
    #[expect(
        dead_code,
        reason = "constructed by the fork's submitter, services::za_imip (iMIP spec 5.3)"
    )]
    Quiet,
}

/// One message's composer: its template, the account the session is
/// authenticated as (the sender's own account when `from` resolves to one
/// other than the event's, else the event account), the message, its summary
/// and the body shared by every recipient without a personal RSVP block.
pub(crate) struct ImipComposer<'x> {
    template: ItipTemplate<'x>,
    sender_info: Cow<'x, AccountInfo>,
    message: &'x TaskCalendarItipContents,
    summary: ItipSummary,
    shared_body: Option<RenderedBody>,
}

/// The per-recipient options: `rsvp` personalizes the body with the RSVP
/// block (`ItipTemplate::set_recipient`), `za_redact` sets the session's
/// `SessionData::za_key_rcpt` (iMIP spec D12), `session_id` is the local
/// session's id.
pub(crate) struct ImipSubmitOptions {
    pub(crate) rsvp: bool,
    pub(crate) za_redact: bool,
    pub(crate) session_id: u64,
}

/// A submission the local session refused: the SMTP stage and its response.
pub(crate) struct ImipRejection {
    pub(crate) stage: &'static str,
    pub(crate) response: String,
}

impl ImipRejection {
    pub(crate) fn into_smtp_rejection(self) -> SmtpRejection {
        SmtpRejection::new(self.stage, &self.response)
    }
}

/// `send_imip`'s setup: the logo with its error trace, its CID
/// `logo.<now>@<domain>`, the event account's info and the formatter.
pub(crate) async fn imip_context(
    server: &Server,
    account_id: u32,
    document_id: u32,
    sender_domain: &str,
) -> trc::Result<ImipContext> {
    // Obtain logo image
    let logo = match server.logo_resource(sender_domain).await {
        Ok(logo) => logo,
        Err(err) => {
            trc::error!(
                err.caused_by(trc::location!())
                    .details("Failed to fetch logo image")
            );
            None
        }
    };
    let logo_cid = format!("logo.{}@{sender_domain}", now());
    // The part owns its bytes (upstream's borrowed them), so the context
    // can hold it; the bytes written are the same.
    let logo = if let Some(logo) = &logo {
        MimePart::new(
            ContentType::new(logo.content_type.clone()),
            BodyPart::Binary(logo.contents.to_vec().into()),
        )
    } else {
        MimePart::new(
            ContentType::new("image/png"),
            BodyPart::Binary(DEFAULT_LOGO_BASE64.as_bytes().into()),
        )
        .transfer_encoding("base64")
    }
    .inline()
    .cid(logo_cid.clone());

    let account_info = server
        .account_info(account_id)
        .await
        .caused_by(trc::location!())?;
    let formatter = TextFormatter::new(account_info.locale().as_str())?;

    Ok(ImipContext {
        logo,
        logo_cid,
        account_info,
        formatter,
        account_id,
        document_id,
    })
}

impl ImipContext {
    /// `send_imip`'s per-message step: the summary (`None` when it does not
    /// parse), the sender's account resolved by `lookup`, and the template.
    pub(crate) async fn composer<'x>(
        &'x self,
        server: &'x Server,
        message: &'x TaskCalendarItipContents,
        lookup: SenderLookup,
    ) -> Option<ImipComposer<'x>> {
        let account_id = self.account_id;
        let document_id = self.document_id;
        let Ok(summary) = serde_json::from_str::<ItipSummary>(&message.summary) else {
            return None;
        };

        let sender_id = match lookup {
            SenderLookup::Traced => {
                server
                    .account_id_from_email(message.from.as_str(), true)
                    .await
            }
            SenderLookup::Quiet => server.za_account_id_from_email(message.from.as_str()).await,
        };
        let organizer_info = match sender_id {
            Ok(Some(sender_id)) if sender_id != account_id => {
                match server.account_info(sender_id).await {
                    Ok(info) => Some(info),
                    Err(err) => {
                        trc::error!(
                            err.account_id(account_id)
                                .document_id(document_id)
                                .caused_by(trc::location!())
                                .details("Failed to load organizer account for iMIP sender")
                        );
                        None
                    }
                }
            }
            Ok(_) => None,
            Err(err) => {
                trc::error!(
                    err.account_id(account_id)
                        .document_id(document_id)
                        .caused_by(trc::location!())
                        .details("Failed to resolve organizer account for iMIP sender")
                );
                None
            }
        };
        let sender_info = match organizer_info {
            Some(info) => Cow::Owned(info),
            None => Cow::Borrowed(&self.account_info),
        };
        let template = ItipTemplate::new(
            server,
            &self.formatter,
            message.from.as_str(),
            &summary,
            &self.logo_cid,
        );

        Some(ImipComposer {
            template,
            sender_info,
            message,
            summary,
            shared_body: None,
        })
    }
}

impl ImipComposer<'_> {
    /// `send_imip`'s per-recipient step without its events: the body
    /// (personal when `options.rsvp` and `set_recipient` adds an RSVP block,
    /// else the shared one), the message, and the local session's MAIL FROM,
    /// RCPT TO and DATA, in a task of its own as upstream runs it. Answers
    /// the queue id, or the stage and response that refused it.
    pub(crate) async fn submit(
        &mut self,
        server: &Server,
        context: &ImipContext,
        instance: Arc<ServerInstance>,
        recipient: &str,
        options: ImipSubmitOptions,
    ) -> trc::Result<Result<u64, ImipRejection>> {
        // Build template
        let personal_body;
        let body = if options.rsvp
            && self
                .template
                .set_recipient(
                    server,
                    &context.account_info,
                    context.account_id,
                    context.document_id,
                    recipient,
                )
                .await
        {
            personal_body = self.template.render();
            &personal_body
        } else {
            &*self
                .shared_body
                .get_or_insert_with(|| self.template.render())
        };

        // Build message
        let message = compose_imip_message(
            self.sender_info
                .description()
                .unwrap_or(self.sender_info.name()),
            self.message.from.as_str(),
            recipient,
            server.core.network.message_id(),
            self.template.subject.as_str(),
            body,
            self.summary.method(),
            self.message.i_calendar_data.as_str(),
            context.logo.clone(),
        );

        // Send message
        let server_ = server.clone();
        let sender_info = self.sender_info.as_ref().clone();
        let from = self.message.from.to_string();
        let to = recipient.to_string();
        let ImipSubmitOptions {
            za_redact,
            session_id,
            ..
        } = options;
        tokio::spawn(async move {
            let mut session = Session::<NullIo>::local(
                server_,
                instance,
                SessionData::local(sender_info, None, vec![], vec![], session_id),
            );
            session.data.za_key_rcpt = za_redact;

            // MAIL FROM
            let _ = session
                .handle_mail_from(MailFrom {
                    address: from.as_str().into(),
                    ..Default::default()
                })
                .await;
            if let Some(error) = session.has_failed() {
                return Err(ImipRejection {
                    stage: "MAIL-FROM",
                    response: error,
                });
            }

            // RCPT TO
            session.params.rcpt_errors_wait = Duration::from_secs(0);
            let _ = session
                .handle_rcpt_to(RcptTo {
                    address: to.as_str().into(),
                    ..Default::default()
                })
                .await;
            if let Some(error) = session.has_failed() {
                return Err(ImipRejection {
                    stage: "RCPT-TO",
                    response: error,
                });
            }

            // DATA
            session.data.message = message;
            let response = session.queue_message(MessageOrigin::Internal).await;
            if let smtp::core::State::Accepted(queue_id) = session.state {
                Ok(queue_id)
            } else {
                Err(ImipRejection {
                    stage: "DATA",
                    response: std::str::from_utf8(&response)
                        .unwrap_or_default()
                        .to_string(),
                })
            }
        })
        .await
        .map_err(|_| {
            trc::Error::new(trc::EventType::Server(trc::ServerEvent::ThreadError))
                .caused_by(trc::location!())
        })
    }
}

/// `send_imip`'s `MessageBuilder` expression (v1.0.1 `imip.rs:192-243`),
/// with the Message-ID passed in.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compose_imip_message(
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
    MessageBuilder::new()
        .from((sender_name, from))
        .to(to)
        .header("Auto-Submitted", HeaderType::Text("auto-generated".into()))
        .header("Reply-To", HeaderType::Text(from.into()))
        .message_id(message_id)
        .subject(subject)
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
                                        .attribute("method", method)
                                        .attribute("charset", "utf-8"),
                                    BodyPart::Text(i_calendar_data.into()),
                                ),
                            ]),
                        ),
                        logo,
                    ]),
                ),
                MimePart::new(
                    ContentType::new("application/ics").attribute("name", "event.ics"),
                    BodyPart::Text(i_calendar_data.into()),
                )
                .attachment("event.ics"),
            ]),
        ))
        .write_to_vec()
        .unwrap_or_default()
}

pub struct Details {
    pub subject: String,
    pub body: String,
}

pub struct RenderedBody {
    pub html: String,
    pub text: String,
}

pub struct ItipTemplate<'x> {
    template: &'x Template<CalendarTemplateVariable>,
    locale: &'static Locale,
    variables: Variables<CalendarTemplateVariable, String>,
    has_rsvp: bool,
    pub subject: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn build_itip_template(
    server: &Server,
    account_info: &AccountInfo,
    account_id: u32,
    document_id: u32,
    from: &str,
    to: &str,
    summary: &ItipSummary,
    logo_cid: &str,
) -> trc::Result<Details> {
    let formatter = TextFormatter::new(account_info.locale().as_str())?;
    let mut template = ItipTemplate::new(server, &formatter, from, summary, logo_cid);
    template
        .set_recipient(server, account_info, account_id, document_id, to)
        .await;
    Ok(Details {
        body: template.template.eval(&template.variables),
        subject: template.subject,
    })
}

impl<'x> ItipTemplate<'x> {
    pub fn new(
        server: &'x Server,
        formatter: &TextFormatter,
        from: &str,
        summary: &ItipSummary,
        logo_cid: &str,
    ) -> Self {
        // SPDX-SnippetBegin
        // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
        // SPDX-License-Identifier: LicenseRef-SEL
        #[cfg(feature = "enterprise")]
        let template = server
            .core
            .enterprise
            .as_ref()
            .and_then(|e| e.template_scheduling_email.as_ref())
            .unwrap_or(&server.core.groupware.itip_template);
        // SPDX-SnippetEnd
        #[cfg(not(feature = "enterprise"))]
        let template = &server.core.groupware.itip_template;
        Self::from_template(template, formatter, from, summary, logo_cid)
    }

    /// Zero-access fork (iMIP spec 3.1 item 1): `new` after its template
    /// selection, so the template renders without a `Server`.
    pub fn from_template(
        template: &'x Template<CalendarTemplateVariable>,
        formatter: &TextFormatter,
        from: &str,
        summary: &ItipSummary,
        logo_cid: &str,
    ) -> Self {
        let locale = formatter.locale;

        let mut variables = Variables::new();
        let mut subject;
        let (fields, old_fields) = match summary {
            ItipSummary::Invite(fields) => {
                subject = format!("{}: ", locale.calendar_invitation);

                (fields, None)
            }
            ItipSummary::Update {
                current, previous, ..
            } => {
                subject = format!("{}: ", locale.calendar_updated_invitation);
                variables.insert_single(
                    CalendarTemplateVariable::Header,
                    locale.calendar_event_updated.to_string(),
                );
                variables.insert_single(CalendarTemplateVariable::Color, "info".to_string());
                (current, Some(previous))
            }
            ItipSummary::Cancel(fields) => {
                subject = format!("{}: ", locale.calendar_cancelled);
                variables.insert_single(
                    CalendarTemplateVariable::Header,
                    locale.calendar_event_cancelled.to_string(),
                );
                variables.insert_single(CalendarTemplateVariable::Color, "danger".to_string());
                (fields, None)
            }
            ItipSummary::Rsvp { part_stat, current } => {
                let (color, value) = match part_stat {
                    ICalendarParticipationStatus::Accepted => {
                        subject = format!("{}: ", locale.calendar_accepted);

                        (
                            "info",
                            locale.calendar_participant_accepted.replace("$name", from),
                        )
                    }
                    ICalendarParticipationStatus::Declined => {
                        subject = format!("{}: ", locale.calendar_declined);
                        (
                            "danger",
                            locale.calendar_participant_declined.replace("$name", from),
                        )
                    }
                    ICalendarParticipationStatus::Tentative => {
                        subject = format!("{}: ", locale.calendar_tentative);
                        (
                            "warning",
                            locale.calendar_participant_tentative.replace("$name", from),
                        )
                    }
                    ICalendarParticipationStatus::Delegated => {
                        subject = format!("{}: ", locale.calendar_delegated);
                        (
                            "warning",
                            locale.calendar_participant_delegated.replace("$name", from),
                        )
                    }
                    _ => {
                        subject = format!("{}: ", locale.calendar_reply);
                        (
                            "info",
                            locale.calendar_participant_reply.replace("$name", from),
                        )
                    }
                };

                variables.insert_single(CalendarTemplateVariable::Header, value);
                variables.insert_single(CalendarTemplateVariable::Color, color.to_string());

                (current, None)
            }
        };

        let mut when_detail: Option<(usize, &ItipValue)> = None;
        let mut details: Vec<AHashMap<CalendarTemplateVariable, String>> = Vec::with_capacity(4);
        for field in [
            ICalendarProperty::Summary,
            ICalendarProperty::Description,
            ICalendarProperty::Dtstart,
            ICalendarProperty::Rrule,
            ICalendarProperty::Location,
            ICalendarProperty::Conference,
        ] {
            let mut old_entries = old_fields.into_iter().flatten().filter(|e| e.name == field);

            for entry in fields.iter().filter(|e| e.name == field) {
                let field_name = match &field {
                    ICalendarProperty::Summary => locale.calendar_summary,
                    ICalendarProperty::Description => locale.calendar_description,
                    ICalendarProperty::Dtstart | ICalendarProperty::Rrule => locale.calendar_when,
                    ICalendarProperty::Location => locale.calendar_location,
                    ICalendarProperty::Conference => locale.calendar_conference,
                    _ => continue,
                };
                let value = formatter.field_to_string(&entry.value, DateStyle::Long);

                let old_entry = old_entries.next();

                match &field {
                    ICalendarProperty::Summary => {
                        subject.push_str(&value);
                    }
                    ICalendarProperty::Dtstart if when_detail.is_none() => {
                        subject.push_str(" @ ");
                        subject.push_str(&value);
                    }
                    ICalendarProperty::Rrule if when_detail.is_none() => {
                        subject.push_str(" @ ");
                        subject.push_str(&value);
                    }
                    _ => (),
                }

                if let ICalendarProperty::Summary | ICalendarProperty::Description = &field {
                    let variable = if matches!(field, ICalendarProperty::Summary) {
                        CalendarTemplateVariable::EventTitle
                    } else {
                        CalendarTemplateVariable::EventDescription
                    };

                    if old_entry.is_none() {
                        variables.insert_single(variable, value);
                        continue;
                    }
                    variables.insert_single(variable, value.clone());
                }

                if matches!(field, ICalendarProperty::Rrule)
                    && let Some((index, start_value)) = when_detail
                    && let Some(detail) = details.get_mut(index)
                {
                    if let Some(when_value) = detail.get_mut(&CalendarTemplateVariable::Value) {
                        when_value.push_str(", ");
                        when_value.push_str(&value);
                    }

                    if let Some(old_entry) = old_entry {
                        detail.insert(
                            CalendarTemplateVariable::Changed,
                            locale.calendar_changed.to_string(),
                        );
                        let old_value = detail
                            .entry(CalendarTemplateVariable::OldValue)
                            .or_insert_with(|| {
                                formatter.field_to_string(start_value, DateStyle::Short)
                            });
                        old_value.push_str(", ");
                        old_value.push_str(
                            &formatter.field_to_string(&old_entry.value, DateStyle::Short),
                        );
                    }

                    continue;
                }

                let mut detail = AHashMap::with_capacity(4);
                detail.insert(CalendarTemplateVariable::Key, field_name.to_string());
                if matches!(field, ICalendarProperty::Conference)
                    && let Some(link) = hyperlink(&value)
                {
                    detail.insert(CalendarTemplateVariable::Link, link.to_string());
                }
                detail.insert(CalendarTemplateVariable::Value, value);
                if let Some(old_entry) = old_entry {
                    detail.insert(
                        CalendarTemplateVariable::Changed,
                        locale.calendar_changed.to_string(),
                    );
                    detail.insert(
                        CalendarTemplateVariable::OldValue,
                        formatter.field_to_string(&old_entry.value, DateStyle::Short),
                    );
                }
                if matches!(field, ICalendarProperty::Dtstart) && when_detail.is_none() {
                    when_detail = Some((details.len(), &entry.value));
                }
                details.push(detail);
            }
        }
        if !details.is_empty() {
            variables.items.insert(
                CalendarTemplateVariable::EventDetails,
                Variable::Block(details),
            );
        }
        variables.insert_single(CalendarTemplateVariable::PageTitle, subject.clone());
        variables.insert_single(CalendarTemplateVariable::Lang, locale.name.to_string());
        variables.insert_single(CalendarTemplateVariable::Dir, locale.direction.to_string());
        variables.insert_single(CalendarTemplateVariable::LogoCid, format!("cid:{logo_cid}"));

        if let Some(guests) = fields
            .iter()
            .find(|e| e.name == ICalendarProperty::Attendee)
            && let ItipValue::Participants(guests) = &guests.value
        {
            variables.insert_single(
                CalendarTemplateVariable::AttendeesTitle,
                locale.calendar_attendees.to_string(),
            );
            variables.insert_block(
                CalendarTemplateVariable::Attendees,
                guests.iter().map(|guest| {
                    [
                        (
                            CalendarTemplateVariable::Key,
                            if guest.is_organizer {
                                if let Some(name) = guest.name.as_ref() {
                                    format!("{name} - {}", locale.calendar_organizer)
                                } else {
                                    locale.calendar_organizer.to_string()
                                }
                            } else {
                                guest.name.as_deref().unwrap_or_default().to_string()
                            },
                        ),
                        (CalendarTemplateVariable::Value, guest.email.to_string()),
                    ]
                }),
            );
        }

        // Add footer
        variables.insert_block(
            CalendarTemplateVariable::Footer,
            [
                [(
                    CalendarTemplateVariable::Key,
                    locale.calendar_imip_footer_1.to_string(),
                )],
                [(
                    CalendarTemplateVariable::Key,
                    locale.calendar_imip_footer_2.to_string(),
                )],
            ],
        );

        ItipTemplate {
            template,
            locale,
            variables,
            has_rsvp: matches!(summary, ItipSummary::Invite(_) | ItipSummary::Update { .. }),
            subject,
        }
    }

    pub async fn set_recipient(
        &mut self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        to: &str,
    ) -> bool {
        let locale = self.locale;
        let variables = &mut self.variables;
        variables.items.remove(&CalendarTemplateVariable::Rsvp);
        variables.items.remove(&CalendarTemplateVariable::Actions);

        // Add RSVP buttons
        if self.has_rsvp
            && let Some(rsvp_url) = server
                .http_rsvp_url(account_id, account_info.name(), document_id, to)
                .await
        {
            variables.insert_single(
                CalendarTemplateVariable::Rsvp,
                locale.calendar_reply_as.replace("$name", to),
            );
            variables.insert_block(
                CalendarTemplateVariable::Actions,
                [
                    (
                        ICalendarParticipationStatus::Accepted,
                        locale.calendar_yes.to_string(),
                        "info",
                    ),
                    (
                        ICalendarParticipationStatus::Declined,
                        locale.calendar_no.to_string(),
                        "danger",
                    ),
                    (
                        ICalendarParticipationStatus::Tentative,
                        locale.calendar_maybe.to_string(),
                        "warning",
                    ),
                ]
                .into_iter()
                .map(|(status, title, color)| {
                    [
                        (CalendarTemplateVariable::ActionName, title.to_string()),
                        (CalendarTemplateVariable::ActionUrl, rsvp_url.url(&status)),
                        (CalendarTemplateVariable::Color, color.to_string()),
                    ]
                }),
            );
            true
        } else {
            false
        }
    }

    pub fn render(&self) -> RenderedBody {
        let html = self.template.eval(&self.variables);
        RenderedBody {
            text: html_to_text(&html),
            html,
        }
    }
}
