/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A local SMTP sink: accepts every transaction and keeps the messages in
//! memory for the za suite's iMIP modules, which route all outside mail to
//! it (`za::imip::route_outside_mail`). It advertises no extension (no
//! STARTTLS, PIPELINING or CHUNKING), so the queue delivers in plain text,
//! one command at a time.

use mail_parser::{Message, MessageParser, MessagePart};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

/// The sink's port (unused elsewhere in `tests/src`).
pub const ZA_SINK_PORT: u16 = 12525;

/// One accepted transaction: its envelope and the message, dot-unstuffed.
#[derive(Debug, Clone)]
pub struct SinkMessage {
    pub mail_from: String,
    pub rcpt_to: Vec<String>,
    pub data: Vec<u8>,
}

impl SinkMessage {
    pub fn parsed(&self) -> Message<'_> {
        MessageParser::new()
            .parse(&self.data)
            .expect("the sink holds a message that parses")
    }

    /// The last header of this name, decoded where the parser knows its
    /// form, else its raw value trimmed.
    pub fn header(&self, name: &str) -> Option<String> {
        let parsed = self.parsed();
        let header = parsed.headers().get(name)?;
        Some(
            header
                .value()
                .as_text()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    String::from_utf8_lossy(header.raw_value())
                        .trim()
                        .to_string()
                }),
        )
    }

    /// The first address of the From header.
    pub fn from_address(&self) -> Option<String> {
        self.parsed()
            .from()
            .and_then(|from| from.first())
            .and_then(|mailbox| mailbox.address().map(str::to_string))
    }

    /// The decoded text of the first `text/calendar` part.
    pub fn calendar_part(&self) -> Option<String> {
        self.part_text("text", "calendar")
    }

    /// The `method` parameter of the first `text/calendar` part.
    pub fn calendar_method(&self) -> Option<String> {
        self.parsed()
            .parts()
            .find(|part| part.is_content_type("text", "calendar"))
            .and_then(|part| part.content_type())
            .and_then(|content_type| content_type.attribute("method").map(str::to_string))
    }

    /// The decoded text of the first `text/plain` part.
    pub fn text_part(&self) -> Option<String> {
        self.part_text("text", "plain")
    }

    /// The decoded text of the first `text/html` part.
    pub fn html_part(&self) -> Option<String> {
        self.part_text("text", "html")
    }

    /// Every part that is not a multipart container, base64 and
    /// quoted-printable decoded.
    pub fn decoded_parts(&self) -> Vec<Vec<u8>> {
        self.parsed()
            .parts()
            .filter(|part| !part.is_multipart())
            .map(|part| part.decoded().into_owned())
            .collect()
    }

    fn part_text(&self, ctype: &str, subtype: &str) -> Option<String> {
        self.parsed()
            .parts()
            .find(|part: &MessagePart<'_>| part.is_content_type(ctype, subtype))
            .and_then(|part| part.text().map(|text| text.into_owned()))
    }
}

/// The messages the sink accepted, in order of acceptance.
#[derive(Default)]
pub struct SmtpSink {
    messages: Mutex<Vec<SinkMessage>>,
}

impl SmtpSink {
    pub fn messages(&self) -> Vec<SinkMessage> {
        self.lock().clone()
    }

    /// Removes and returns the messages addressed to `rcpt` (case-insensitive).
    pub fn take_to(&self, rcpt: &str) -> Vec<SinkMessage> {
        let mut messages = self.lock();
        let (taken, kept) = std::mem::take(&mut *messages)
            .into_iter()
            .partition(|message| is_to(message, rcpt));
        *messages = kept;
        taken
    }

    /// How many messages are addressed to `rcpt` (case-insensitive).
    pub fn count_to(&self, rcpt: &str) -> usize {
        self.lock()
            .iter()
            .filter(|message| is_to(message, rcpt))
            .count()
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<SinkMessage>> {
        self.messages.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push(&self, message: SinkMessage) {
        self.lock().push(message);
    }
}

fn is_to(message: &SinkMessage, rcpt: &str) -> bool {
    message
        .rcpt_to
        .iter()
        .any(|address| address.eq_ignore_ascii_case(rcpt))
}

static SINK: OnceLock<Arc<SmtpSink>> = OnceLock::new();

/// Starts the suite's sink on `127.0.0.1:port`; a second call returns the
/// running sink.
pub async fn start(port: u16) -> Arc<SmtpSink> {
    if let Some(sink) = SINK.get() {
        return sink.clone();
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap_or_else(|err| panic!("the SMTP sink cannot bind port {port}: {err}"));
    let sink = SINK.get_or_init(|| Arc::new(SmtpSink::default())).clone();
    let accepting = sink.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(session(stream, accepting.clone()));
        }
    });
    sink
}

/// The suite's sink; panics before `start`.
pub fn sink() -> Arc<SmtpSink> {
    SINK.get().expect("the SMTP sink is not started").clone()
}

/// One connection: a greeting, then one reply per command.
async fn session(stream: TcpStream, sink: Arc<SmtpSink>) {
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    let mut line = Vec::new();
    let mut mail_from = String::new();
    let mut rcpt_to = Vec::new();

    if write
        .write_all(b"220 za-sink.test ESMTP\r\n")
        .await
        .is_err()
    {
        return;
    }
    loop {
        line.clear();
        match read.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = String::from_utf8_lossy(&line);
        let verb = command
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reply: &[u8] = match verb.as_str() {
            "EHLO" | "HELO" => b"250 za-sink.test\r\n",
            "MAIL" => {
                mail_from = envelope_address(&command);
                rcpt_to.clear();
                b"250 2.1.0 OK\r\n"
            }
            "RCPT" => {
                rcpt_to.push(envelope_address(&command));
                b"250 2.1.5 OK\r\n"
            }
            "DATA" => {
                if write
                    .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                    .await
                    .is_err()
                {
                    return;
                }
                let Some(data) = read_data(&mut read).await else {
                    return;
                };
                // Kept before the reply, so a queue that saw the 250 finds
                // the message here.
                sink.push(SinkMessage {
                    mail_from: std::mem::take(&mut mail_from),
                    rcpt_to: std::mem::take(&mut rcpt_to),
                    data,
                });
                b"250 2.0.0 OK\r\n"
            }
            "RSET" => {
                mail_from.clear();
                rcpt_to.clear();
                b"250 2.0.0 OK\r\n"
            }
            "NOOP" => b"250 2.0.0 OK\r\n",
            "QUIT" => {
                let _ = write.write_all(b"221 2.0.0 Bye\r\n").await;
                return;
            }
            _ => b"502 5.5.1 Command not implemented\r\n",
        };
        if write.write_all(reply).await.is_err() {
            return;
        }
    }
}

/// The address between the angle brackets of a MAIL or RCPT command.
fn envelope_address(command: &str) -> String {
    command
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(address, _)| address.to_string())
        .unwrap_or_default()
}

/// The message lines up to the lone dot, dot-unstuffed; `None` when the
/// connection ends first.
async fn read_data(read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        match read.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        if line == b".\r\n" || line == b".\n" {
            return Some(data);
        }
        let unstuffed = line.strip_prefix(b".").unwrap_or(&line);
        data.extend_from_slice(unstuffed);
    }
}
