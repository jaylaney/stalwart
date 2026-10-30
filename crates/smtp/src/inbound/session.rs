/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{auth::SaslToken, data::MessageOrigin};
use crate::core::{DataFailure, Session, State};
use common::{
    config::server::ServerProtocol,
    expr::{Bump, Variable, functions::ResolveVariable},
    network::{
        SessionStream,
        memory::{MEMORY_STEP, MemoryClass},
    },
};
use compact_str::{CompactString, ToCompactString, format_compact};
use registry::schema::enums::ExpressionVariable;
use smtp_proto::{
    request::receiver::{
        BdatReceiver, DataReceiver, DummyDataReceiver, DummyLineReceiver, LineReceiver,
        MAX_LINE_LENGTH,
    },
    *,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use trc::{LimitEvent, NetworkEvent, SecurityEvent, SmtpEvent};

const MAX_BATCHED_RESPONSE: usize = 4096;
const MIN_DATA_BUFFER: usize = 1024;
pub(crate) const INSUFFICIENT_STORAGE: &[u8] =
    b"452 4.3.1 Insufficient system storage, try again later.\r\n";

impl<T: SessionStream> Session<T> {
    pub async fn ingest(&mut self, bytes: &[u8]) -> Result<bool, ()> {
        let mut iter = bytes.iter();
        let mut state = std::mem::replace(&mut self.state, State::None);

        'outer: loop {
            match &mut state {
                State::Request(receiver) => loop {
                    match receiver.ingest(&mut iter) {
                        Ok(request) => match request {
                            Request::Rcpt { to } => {
                                self.handle_rcpt_to(to).await?;
                            }
                            Request::Mail { from } => {
                                if self.server.is_memory_available(MemoryClass::SmtpInbound) {
                                    self.handle_mail_from(from).await?;
                                } else {
                                    trc::event!(
                                        Limit(LimitEvent::MemoryBudget),
                                        SpanId = self.data.session_id,
                                        Size = from.size,
                                        Limit = self.server.core.network.max_buffered_memory,
                                    );
                                    self.write(INSUFFICIENT_STORAGE).await?;
                                }
                            }
                            Request::Ehlo { host } => {
                                if self.instance.protocol == ServerProtocol::Smtp {
                                    self.handle_ehlo(host, true).await?;
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::LhloExpected),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(b"500 5.5.1 Invalid command.\r\n").await?;
                                }
                            }
                            Request::Data => {
                                if let Some(response) = self.can_send_data().await {
                                    self.write(response).await?;
                                } else {
                                    self.write(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n")
                                        .await?;
                                    self.data.message = Vec::new();
                                    self.data.message_memory = Some(
                                        self.server.memory_reservation(MemoryClass::SmtpInbound),
                                    );
                                    state = State::Data(DataReceiver::new());
                                    continue 'outer;
                                }
                            }
                            Request::Bdat {
                                chunk_size,
                                is_last,
                            } => {
                                state = if let Some(response) = self.can_send_data().await {
                                    State::SkipData(
                                        DummyDataReceiver::new_bdat(chunk_size),
                                        response,
                                        DataFailure::Chunk,
                                    )
                                } else if chunk_size.saturating_add(self.data.message.len())
                                    < self.params.max_message_size
                                {
                                    if self.data.message_memory.is_none() {
                                        self.data.message_memory = Some(
                                            self.server
                                                .memory_reservation(MemoryClass::SmtpInbound),
                                        );
                                    }
                                    State::Bdat {
                                        receiver: BdatReceiver::new(chunk_size, is_last),
                                        bytes_left: chunk_size,
                                    }
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::MessageTooLarge),
                                        SpanId = self.data.session_id,
                                        Size = chunk_size.saturating_add(self.data.message.len()),
                                        Limit = self.params.max_message_size,
                                    );
                                    self.reset();

                                    State::SkipData(
                                        DummyDataReceiver::new_bdat(chunk_size),
                                        b"552 5.3.4 Message too big for system.\r\n",
                                        DataFailure::Chunk,
                                    )
                                };
                                continue 'outer;
                            }
                            Request::Auth {
                                mechanism,
                                initial_response,
                            } => {
                                let auth = self.sasl_mechanisms().await;
                                if auth == 0 {
                                    trc::event!(
                                        Smtp(SmtpEvent::AuthNotAllowed),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(b"503 5.5.1 AUTH not allowed.\r\n").await?;
                                } else if let Some(authenticated_as) = self.authenticated_as() {
                                    trc::event!(
                                        Smtp(SmtpEvent::AlreadyAuthenticated),
                                        SpanId = self.data.session_id,
                                        AccountName = CompactString::from(authenticated_as),
                                    );

                                    self.write(b"503 5.5.1 Already authenticated.\r\n").await?;
                                } else if let Some(mut token) =
                                    SaslToken::from_mechanism(mechanism & auth)
                                {
                                    if self
                                        .handle_sasl_response(
                                            &mut token,
                                            initial_response.as_bytes(),
                                        )
                                        .await?
                                    {
                                        state = State::Sasl(LineReceiver::new(token));
                                        continue 'outer;
                                    }
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::AuthMechanismNotSupported),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(
                                        b"554 5.7.8 Authentication mechanism not supported.\r\n",
                                    )
                                    .await?;
                                }
                            }
                            Request::Noop { .. } => {
                                trc::event!(Smtp(SmtpEvent::Noop), SpanId = self.data.session_id,);

                                self.write(b"250 2.0.0 OK\r\n").await?;
                            }
                            Request::Vrfy { value } => {
                                self.handle_vrfy(value).await?;
                            }
                            Request::Expn { value } => {
                                self.handle_expn(value).await?;
                            }
                            Request::StartTls => {
                                if !self.stream.is_tls() {
                                    if self.instance.acceptor.is_tls() {
                                        trc::event!(
                                            Smtp(SmtpEvent::StartTls),
                                            SpanId = self.data.session_id,
                                        );

                                        self.write(b"220 2.0.0 Ready to start TLS.\r\n").await?;
                                        #[cfg(any(test, stalwart_tests))]
                                        if self.data.helo_domain.contains("badtls") {
                                            return Err(());
                                        }
                                        self.state = State::default();
                                        self.reset_tls();
                                        return Ok(false);
                                    } else {
                                        trc::event!(
                                            Smtp(SmtpEvent::StartTlsUnavailable),
                                            SpanId = self.data.session_id,
                                        );

                                        self.write(b"502 5.7.0 TLS not available.\r\n").await?;
                                    }
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::StartTlsAlready),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(b"504 5.7.4 Already in TLS mode.\r\n").await?;
                                }
                            }
                            Request::Rset => {
                                trc::event!(Smtp(SmtpEvent::Rset), SpanId = self.data.session_id,);

                                self.reset();
                                self.write(b"250 2.0.0 OK\r\n").await?;
                            }
                            Request::Quit => {
                                trc::event!(Smtp(SmtpEvent::Quit), SpanId = self.data.session_id,);

                                self.write(b"221 2.0.0 Bye.\r\n").await?;
                                return Err(());
                            }
                            Request::Help { .. } => {
                                trc::event!(Smtp(SmtpEvent::Help), SpanId = self.data.session_id,);

                                self.write(b"250 2.0.0 Help can be found at https://stalw.art\r\n")
                                    .await?;
                            }
                            Request::Helo { host } => {
                                if self.instance.protocol == ServerProtocol::Smtp {
                                    self.handle_ehlo(host, false).await?;
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::LhloExpected),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(b"500 5.5.1 Invalid command: LHLO expected.\r\n")
                                        .await?;
                                }
                            }
                            Request::Lhlo { host } => {
                                if self.instance.protocol == ServerProtocol::Lmtp {
                                    self.handle_ehlo(host, true).await?;
                                } else {
                                    trc::event!(
                                        Smtp(SmtpEvent::EhloExpected),
                                        SpanId = self.data.session_id,
                                    );

                                    self.write(b"502 5.5.1 Invalid command: EHLO expected.\r\n")
                                        .await?;
                                }
                            }
                            cmd @ (Request::Etrn { .. }
                            | Request::Atrn { .. }
                            | Request::Burl { .. }) => {
                                trc::event!(
                                    Smtp(SmtpEvent::CommandNotImplemented),
                                    SpanId = self.data.session_id,
                                    Details = format_compact!("{cmd:?}"),
                                );

                                self.write(b"502 5.5.1 Command not implemented.\r\n")
                                    .await?;
                            }
                        },
                        Err(err) => match err {
                            Error::NeedsMoreData { .. } => break 'outer,
                            Error::UnknownCommand | Error::InvalidResponse { .. } => {
                                // Check for port scanners
                                if !self.is_authenticated() {
                                    match self
                                        .server
                                        .is_scanner_fail2banned(self.data.remote_ip)
                                        .await
                                    {
                                        Ok(true) => {
                                            trc::event!(
                                                Security(SecurityEvent::ScanBan),
                                                SpanId = self.data.session_id,
                                                RemoteIp = self.data.remote_ip,
                                                Reason = "Invalid SMTP command",
                                            );

                                            return Err(());
                                        }
                                        Ok(false) => {}
                                        Err(err) => {
                                            trc::error!(
                                                err.span_id(self.data.session_id)
                                                    .details("Failed to check for fail2ban")
                                            );
                                        }
                                    }
                                }

                                trc::event!(
                                    Smtp(SmtpEvent::InvalidCommand),
                                    SpanId = self.data.session_id,
                                );

                                self.write(b"500 5.5.1 Invalid command.\r\n").await?;
                            }
                            Error::InvalidSenderAddress => {
                                trc::event!(
                                    Smtp(SmtpEvent::InvalidSenderAddress),
                                    SpanId = self.data.session_id,
                                );

                                self.write(b"501 5.1.8 Bad sender's system address.\r\n")
                                    .await?;
                            }
                            Error::InvalidRecipientAddress => {
                                trc::event!(
                                    Smtp(SmtpEvent::InvalidRecipientAddress),
                                    SpanId = self.data.session_id,
                                );

                                self.write(
                                    b"501 5.1.3 Bad destination mailbox address syntax.\r\n",
                                )
                                .await?;
                            }
                            Error::SyntaxError { syntax } => {
                                trc::event!(
                                    Smtp(SmtpEvent::SyntaxError),
                                    SpanId = self.data.session_id,
                                    Details = syntax
                                );

                                if !self.params.ehlo_reject_non_fqdn && syntax.starts_with("EHLO ")
                                {
                                    self.handle_ehlo("null".into(), true).await?
                                } else {
                                    self.write(
                                        format!("501 5.5.2 Syntax error, expected: {syntax}\r\n")
                                            .as_bytes(),
                                    )
                                    .await?;
                                }
                            }
                            Error::InvalidParameter { param } => {
                                trc::event!(
                                    Smtp(SmtpEvent::InvalidParameter),
                                    SpanId = self.data.session_id,
                                    Name = param
                                );

                                self.write(
                                    format!("501 5.5.4 Invalid parameter {param:?}.\r\n")
                                        .as_bytes(),
                                )
                                .await?;
                            }
                            Error::UnsupportedParameter { param } => {
                                trc::event!(
                                    Smtp(SmtpEvent::UnsupportedParameter),
                                    SpanId = self.data.session_id,
                                    Value = param.clone()
                                );

                                self.write(
                                    format!("504 5.5.4 Unsupported parameter {param:?}.\r\n")
                                        .as_bytes(),
                                )
                                .await?;
                            }
                            Error::ResponseTooLong => {
                                state = State::RequestTooLarge(DummyLineReceiver::default());
                                continue 'outer;
                            }
                        },
                    }
                },
                State::Data(receiver) => {
                    if self.data.message.len() + bytes.len() < self.params.max_message_size {
                        if !self.grow_message_buffer(iter.len()) {
                            state = State::SkipData(
                                DummyDataReceiver::new_data(receiver),
                                INSUFFICIENT_STORAGE,
                                DataFailure::Transaction,
                            );
                        } else if receiver.ingest(&mut iter, &mut self.data.message) {
                            let message = self.queue_message(MessageOrigin::Client).await;
                            if !message.is_empty() {
                                self.write_data_responses(message.as_ref()).await?;
                                self.reset();
                                state = State::default();
                            } else {
                                // Disconnect requested
                                return Err(());
                            }
                        } else {
                            break 'outer;
                        }
                    } else {
                        trc::event!(
                            Smtp(SmtpEvent::MessageTooLarge),
                            SpanId = self.data.session_id,
                            Size = self.data.message.len() + bytes.len(),
                            Limit = self.params.max_message_size,
                        );

                        state = State::SkipData(
                            DummyDataReceiver::new_data(receiver),
                            b"552 5.3.4 Message too big for system.\r\n",
                            DataFailure::Transaction,
                        );
                    }
                }
                State::Bdat {
                    receiver,
                    bytes_left,
                } => {
                    if !self.grow_message_buffer(iter.len().min(*bytes_left)) {
                        let bytes_left = *bytes_left;
                        self.reset();
                        state = State::SkipData(
                            DummyDataReceiver::new_bdat(bytes_left),
                            INSUFFICIENT_STORAGE,
                            DataFailure::Chunk,
                        );
                        continue 'outer;
                    }
                    let available = iter.len();
                    let completed = receiver.ingest(&mut iter, &mut self.data.message);
                    *bytes_left = bytes_left.saturating_sub(available - iter.len());
                    if completed {
                        if receiver.is_last {
                            let message = self.queue_message(MessageOrigin::Client).await;
                            if !message.is_empty() {
                                self.write_data_responses(message.as_ref()).await?;
                                self.reset();
                            } else {
                                // Disconnect requested
                                return Err(());
                            }
                        } else {
                            self.write(b"250 2.6.0 Chunk accepted.\r\n").await?;
                        }
                        state = State::default();
                    } else {
                        break 'outer;
                    }
                }
                State::Sasl(receiver) => {
                    if receiver.ingest(&mut iter) {
                        if receiver.buf.len() < MAX_LINE_LENGTH {
                            if self
                                .handle_sasl_response(&mut receiver.state, &receiver.buf)
                                .await?
                            {
                                receiver.buf.clear();
                                continue 'outer;
                            }
                        } else {
                            trc::event!(
                                Smtp(SmtpEvent::AuthExchangeTooLong),
                                SpanId = self.data.session_id,
                                Limit = MAX_LINE_LENGTH,
                            );

                            self.auth_error(
                                b"500 5.5.6 Authentication Exchange line is too long.\r\n",
                            )
                            .await?;
                        }
                        state = State::default();
                    } else {
                        break 'outer;
                    }
                }
                State::SkipData(receiver, response, failure) => {
                    if receiver.ingest(&mut iter) {
                        match failure {
                            DataFailure::Chunk => {
                                self.data.message = Vec::with_capacity(0);
                                self.data.message_memory = None;
                                self.write(response).await?;
                            }
                            DataFailure::Transaction => {
                                self.write_data_responses(response).await?;
                                self.reset();
                            }
                        }
                        state = State::default();
                    } else {
                        break 'outer;
                    }
                }
                State::RequestTooLarge(receiver) => {
                    if receiver.ingest(&mut iter) {
                        trc::event!(
                            Smtp(SmtpEvent::RequestTooLarge),
                            SpanId = self.data.session_id,
                        );

                        self.write(b"554 5.3.4 Line is too long.\r\n").await?;
                        state = State::default();
                    } else {
                        break 'outer;
                    }
                }
                State::None | State::Accepted(_) => unreachable!(),
            }
        }
        self.state = state;

        Ok(true)
    }

    fn grow_message_buffer(&mut self, incoming: usize) -> bool {
        let required = self.data.message.len().saturating_add(incoming);
        let capacity = self.data.message.capacity();
        if required <= capacity {
            return true;
        }

        let target = if capacity == 0 {
            self.data
                .message_size
                .clamp(MIN_DATA_BUFFER, MEMORY_STEP as usize)
        } else {
            capacity.saturating_mul(2)
        }
        .min(self.params.max_message_size)
        .max(required);
        if let Some(memory) = &mut self.data.message_memory
            && !memory.reserve_to(target as u64)
        {
            trc::event!(
                Limit(LimitEvent::MemoryBudget),
                SpanId = self.data.session_id,
                Size = target,
                Limit = memory.limit(),
            );
            self.data.message = Vec::new();
            self.data.message_memory = None;
            return false;
        }

        self.data
            .message
            .reserve_exact(target - self.data.message.len());
        true
    }

    async fn write_data_responses(&mut self, response: &[u8]) -> Result<(), ()> {
        let count = if self.instance.protocol == ServerProtocol::Smtp {
            1
        } else {
            self.data.rcpt_oks
        };
        match count {
            0 => return Ok(()),
            1 => return self.write(response).await,
            _ => {}
        }

        let per_write = (MAX_BATCHED_RESPONSE / response.len().max(1)).clamp(1, count);
        let batch = response.repeat(per_write);
        let mut remaining = count;
        while remaining > 0 {
            let chunk = remaining.min(per_write);
            self.write(batch.get(..chunk * response.len()).unwrap_or_default())
                .await?;
            remaining -= chunk;
        }
        Ok(())
    }
}

impl<T: AsyncWrite + AsyncRead + Unpin> Session<T> {
    pub fn reset(&mut self) {
        self.data.mail_from = None;
        self.data.spf_mail_from = None;
        self.data.rcpt_to.clear();
        self.data.message = Vec::with_capacity(0);
        self.data.message_memory = None;
        self.data.message_size = 0;
        self.data.priority = 0;
        self.data.delivery_by = 0;
        self.data.future_release = 0;
        self.data.rcpt_oks = 0;
        self.data.za_key_rcpt = false;
    }

    pub fn reset_tls(&mut self) {
        self.reset();
        self.data.helo_domain.clear();
        self.data.spf_ehlo = None;
        self.data.authenticated_as = None;
    }

    #[inline(always)]
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        // Issue #20: the reply to a 334 can carry a SASL response, also when
        // the session left the exchange (the MFA reply).
        self.data.za_answers_334 |= bytes.starts_with(b"334");
        match self.stream.write_all(bytes).await {
            Ok(_) => match self.stream.flush().await {
                Ok(_) => {
                    trc::event!(
                        Smtp(SmtpEvent::RawOutput),
                        SpanId = self.data.session_id,
                        Size = bytes.len(),
                        Contents = trc::Value::from_maybe_string(bytes),
                    );

                    Ok(())
                }
                Err(err) => {
                    trc::event!(
                        Network(NetworkEvent::FlushError),
                        SpanId = self.data.session_id,
                        Reason = err.to_compact_string(),
                    );
                    Err(())
                }
            },
            Err(err) => {
                trc::event!(
                    Network(NetworkEvent::WriteError),
                    SpanId = self.data.session_id,
                    Reason = err.to_compact_string(),
                );

                Err(())
            }
        }
    }

    #[inline(always)]
    pub async fn read(&mut self, bytes: &mut Vec<u8>) -> Result<usize, ()> {
        bytes.clear();
        match self.stream.read_buf(bytes).await {
            Ok(len) => {
                // Masked until the line answering a 334 ends.
                let answers_334 = self.data.za_answers_334;
                self.data.za_answers_334 =
                    answers_334 && !bytes.get(0..len).unwrap_or_default().contains(&b'\n');
                trc::event!(
                    Smtp(SmtpEvent::RawInput),
                    SpanId = self.data.session_id,
                    Size = len,
<<<<<<< HEAD
                    Contents = if answers_334
                        || is_auth_input(&mut self.state, bytes.get(0..len).unwrap_or_default())
                        || self.data.za_key_sender()
                        || is_data_input(
                            &mut self.state,
                            bytes.get(0..len).unwrap_or_default(),
                            self.data.za_key_rcpt,
                        ) {
                        trc::Value::redacted()
                    } else {
                        String::from_utf8_lossy(bytes.get(0..len).unwrap_or_default())
                            .into_owned()
                            .into()
                    },
=======
                    Contents = CompactString::from(String::from_utf8_lossy(
                        bytes.get(0..len).unwrap_or_default()
                    )),
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                );

                Ok(len)
            }
            Err(err) => {
                trc::event!(
                    Network(NetworkEvent::ReadError),
                    SpanId = self.data.session_id,
                    Reason = err.to_compact_string(),
                );

                Err(())
            }
        }
    }
}

impl<T: SessionStream> ResolveVariable for Session<T> {
    fn resolve_variable<'a>(
        &'a self,
        variable: ExpressionVariable,
        arena: &'a Bump,
    ) -> Variable<'a> {
        match variable {
            ExpressionVariable::Rcpt => self
                .data
                .rcpt_to
                .last()
                .map(|r| r.address_lcase.as_str())
                .unwrap_or_default()
                .into(),
            ExpressionVariable::RcptDomain => self
                .data
                .rcpt_to
                .last()
                .map(|r| r.domain.as_str())
                .unwrap_or_default()
                .into(),
            ExpressionVariable::Recipients => Variable::Array(
                arena.alloc_slice_fill_iter(
                    self.data
                        .rcpt_to
                        .iter()
                        .map(|r| Variable::String(r.address_lcase.as_str())),
                ),
            ),
            ExpressionVariable::Sender => self
                .data
                .mail_from
                .as_ref()
                .map(|m| m.address_lcase.as_str())
                .unwrap_or_default()
                .into(),
            ExpressionVariable::SenderDomain => self
                .data
                .mail_from
                .as_ref()
                .map(|m| m.domain.as_str())
                .unwrap_or_default()
                .into(),
            ExpressionVariable::HeloDomain => self.data.helo_domain.as_str().into(),
            ExpressionVariable::AuthenticatedAs => {
                self.authenticated_as().unwrap_or_default().into()
            }
            ExpressionVariable::Listener => self.instance.id.as_str().into(),
            ExpressionVariable::RemoteIp => self.data.remote_ip_str.as_str().into(),
            ExpressionVariable::RemotePort => self.data.remote_port.into(),
            ExpressionVariable::LocalIp => self.data.local_ip_str.as_str().into(),
            ExpressionVariable::LocalPort => self.data.local_port.into(),
            ExpressionVariable::IsTls => self.stream.is_tls().into(),
            ExpressionVariable::Priority => self.data.priority.into(),
            ExpressionVariable::Protocol => self.instance.protocol.as_str().into(),
            ExpressionVariable::Asn => self
                .data
                .asn_geo_data
                .asn
                .as_ref()
                .map(|a| a.id)
                .unwrap_or_default()
                .into(),
            ExpressionVariable::Country => self
                .data
                .asn_geo_data
                .country
                .as_deref()
                .unwrap_or_default()
                .into(),
            _ => Variable::default(),
        }
    }
}

/// Whether a read may carry the authentication exchange, so that its raw-input
/// trace records the redaction marker (issue #20): the session is in a SASL
/// exchange, or the read, joined to the unparsed start of the current request
/// line, holds an AUTH command anywhere (the parser can start a request
/// mid-line, after a BDAT chunk or a parameter error). Other traffic is traced
/// as upstream traces it.
fn is_auth_input(state: &mut State, bytes: &[u8]) -> bool {
    let partial: &[u8] = match state {
        State::Sasl(_) => return true,
        // `buf` clears a line the receiver already parsed, as its next
        // `ingest` would.
        State::Request(receiver) => receiver.buf(),
        _ => &[],
    };
    has_auth_command(partial.iter().chain(bytes))
}

/// Whether `AUTH` followed by ASCII whitespace (the parser's stop characters,
/// a form feed included) occurs anywhere, in any case, ignoring CRs as the
/// command parser does. A false positive only masks a read.
fn has_auth_command<'x>(bytes: impl Iterator<Item = &'x u8>) -> bool {
    has_command(bytes, b"AUTH")
}

/// Mail spec 10: whether a read may carry a key-account recipient's message
/// content. With such a recipient (`key_rcpt`), the session is in the DATA or
/// BDAT phase, or the read, joined to the unparsed start of the current
/// request line, holds a DATA or BDAT command (a BDAT chunk can follow its
/// command in one read). Ruling R41: the same read joined to the same start,
/// also holding an RCPT command not yet processed, is masked for every
/// account, as the recipient is unknown when the read is traced (an AUTH is
/// masked for the same reason, issue #20).
fn is_data_input(state: &mut State, bytes: &[u8], key_rcpt: bool) -> bool {
    let partial: &[u8] = match state {
        State::Data(_) | State::Bdat(_) | State::SkipData(..) if key_rcpt => return true,
        State::Request(receiver) => receiver.buf(),
        _ => &[],
    };
    let joined = partial.iter().chain(bytes);
    has_data_command(joined.clone()) && (key_rcpt || has_command(joined, b"RCPT"))
}

/// Whether a DATA or a BDAT command occurs, as `has_command` finds one.
fn has_data_command<'x>(bytes: impl Iterator<Item = &'x u8> + Clone) -> bool {
    has_command(bytes.clone(), b"DATA") || has_command(bytes, b"BDAT")
}

/// Whether `command` (no proper prefix of which is also its suffix) followed by
/// ASCII whitespace (the parser's stop characters, a form feed and the line
/// end included) occurs anywhere, in any case, ignoring CRs as the command
/// parser does: the parser can start a request mid-line, after a BDAT chunk.
/// A false positive only masks a read.
fn has_command<'x>(bytes: impl Iterator<Item = &'x u8>, command: &[u8; 4]) -> bool {
    let mut matched = 0;
    for &ch in bytes.filter(|&&ch| ch != b'\r') {
        matched = match ch {
            _ if matched == 4 && ch.is_ascii_whitespace() => return true,
            _ if matched < 4 && ch.eq_ignore_ascii_case(&command[matched]) => matched + 1,
            _ if ch.eq_ignore_ascii_case(&command[0]) => 1,
            _ => 0,
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{has_data_command, is_data_input};
    use crate::core::State;

    #[test]
    fn has_data_command_finds_data_and_bdat() {
        for bytes in [
            &b"DATA\r\n"[..],
            b"bdat 12\r\nxx",
            b"RCPT TO:<a@b>\r\nDATA\r\n",
            b" data\n",
            b"DATA \r\n",
            b"DATA\t\r\n",
            b"DATA\x0c\r\n",
            b"DA\rTA\r\n",
            b"BDAT\x0c12 LAST\r\n",
            b"BD\rAT 12 LAST\r\n",
            // A DATA starting mid-line, after a parameter error.
            b"MAIL FROM:<a@b> SIZE=xDATA\r\n",
            // Matched anywhere: after a BDAT chunk the parser starts a request
            // mid-word (`BDAT 2\r\nUPDATA\r\n` is the chunk `UP`, then DATA).
            b"UPDATA\r\n",
        ] {
            assert!(
                has_data_command(bytes.iter()),
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
        for bytes in [&b"RCPT TO:<data@b>\r\n"[..], b"DATAX\r\n", b"NOOP\r\n"] {
            assert!(
                !has_data_command(bytes.iter()),
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn has_data_command_masks_an_unclassified_recipient() {
        let pipelined = b"RCPT TO:<a@b>\r\nBDAT 3 LAST\r\nabc";
        // Ruling R41: masked whether or not a key recipient is known.
        assert!(is_data_input(&mut State::default(), pipelined, false));
        assert!(is_data_input(&mut State::default(), pipelined, true));
        // A chunk without a pipelined recipient: masked for a key recipient.
        let chunk = b"BDAT 3 LAST\r\nabc";
        assert!(!is_data_input(&mut State::default(), chunk, false));
        assert!(is_data_input(&mut State::default(), chunk, true));
        // An envelope command alone is traced.
        assert!(!is_data_input(
            &mut State::default(),
            b"RCPT TO:<a@b>\r\n",
            true
        ));
    }
}
