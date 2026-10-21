/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The mock milter of the SMTP milter tests, shared with the zero-access
//! trace tests (`tests/src/za/mail/traces.rs`).

use serde::Deserialize;
use smtp::inbound::milter::{
    Action, Command, Modification, Options, Response,
    receiver::{FrameResult, Receiver},
};
use std::{fs, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};

#[derive(Debug, Deserialize)]
pub struct HeaderTest {
    pub modifications: Vec<Modification>,
    pub result: String,
}

pub fn spawn_mock_milter_server() -> watch::Sender<bool> {
    let (tx, rx) = watch::channel(true);
    let tests = Arc::new(
        serde_json::from_str::<Vec<HeaderTest>>(
            &fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("resources")
                    .join("smtp")
                    .join("milter")
                    .join("message.json"),
            )
            .unwrap(),
        )
        .unwrap(),
    );

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9332")
            .await
            .unwrap_or_else(|e| {
                panic!("Failed to bind mock Milter server to 127.0.0.1:9332: {e}");
            });
        let mut rx_ = rx.clone();
        //println!("Mock Milter server listening on port 9332");
        loop {
            tokio::select! {
                stream = listener.accept() => {
                    match stream {
                        Ok((stream, _)) => {
                            tokio::spawn(accept_milter(stream, rx.clone(), tests.clone()));
                        }
                        Err(err) => {
                            panic!("Something went wrong: {err}" );
                        }
                    }
                },
                _ = rx_.changed() => {
                    //println!("Mock Milter server stopping");
                    break;
                }
            };
        }
    });

    tx
}

async fn accept_milter(
    mut stream: TcpStream,
    mut rx: watch::Receiver<bool>,
    tests: Arc<Vec<HeaderTest>>,
) {
    let mut buf = vec![0u8; 1024];
    let mut receiver = Receiver::with_max_frame_len(5000000);
    let mut action = None;
    let mut modifications = None;

    'outer: loop {
        let br = tokio::select! {
            br = stream.read(&mut buf) => {
                match br {
                    Ok(br) => {
                        br
                    }
                    Err(_) => {
                        break;
                    }
                }
            },
            _ = rx.changed() => {
                break;
            }
        };

        if br == 0 {
            break;
        }

        loop {
            match receiver.read_frame(&buf[..br]) {
                FrameResult::Frame(bytes) => {
                    let cmd = Command::deserialize(bytes.as_ref());

                    let response = match cmd {
                        Command::Abort | Command::Macro { .. } => continue,
                        Command::Body { .. }
                        | Command::Data
                        | Command::Connect { .. }
                        | Command::Header { .. }
                        | Command::Helo { .. }
                        | Command::Rcpt { .. }
                        | Command::QuitNewConnection
                        | Command::EndOfHeader => Response::Action(Action::Accept),
                        Command::OptionNegotiation(_) => Response::OptionNegotiation(Options {
                            version: 6,
                            actions: 0,
                            protocol: 0,
                        }),
                        Command::MailFrom { sender, .. } => {
                            let sender = std::str::from_utf8(sender).unwrap();
                            let (local, domain) =
                                sender.strip_prefix('<').unwrap().split_once('@').unwrap();
                            action = match local {
                                // `add_rcpt.<name>@<domain>` adds the recipient
                                // `<name>@<domain>` (the zero-access trace tests).
                                local if local.starts_with("add_rcpt.") => {
                                    modifications = vec![Modification::AddRcpt {
                                        recipient: format!(
                                            "<{}@{}>",
                                            &local["add_rcpt.".len()..],
                                            domain.trim_end_matches('>')
                                        ),
                                        args: String::new(),
                                    }]
                                    .into();
                                    Action::Accept
                                }
                                "accept" => Action::Accept,
                                "reject" => Action::Reject,
                                "discard" => Action::Discard,
                                "temp_fail" => Action::TempFail,
                                "shutdown" => Action::Shutdown,
                                "conn_fail" => Action::ConnectionFailure,
                                "reply_code" => Action::ReplyCode {
                                    code: *b"321",
                                    text: "test".into(),
                                },
                                test_num => {
                                    // A sender that names no test (the
                                    // zero-access trace tests') is accepted
                                    // unmodified.
                                    if let Ok(test_num) = test_num.parse::<usize>() {
                                        modifications =
                                            tests[test_num].modifications.clone().into();
                                    }
                                    Action::Accept
                                }
                            }
                            .into();
                            Response::Action(Action::Accept)
                        }
                        Command::Quit => break 'outer,
                        Command::EndOfBody => {
                            if let Some(modifications) = modifications.take() {
                                for modification in modifications {
                                    // Write modifications
                                    stream
                                        .write_all(
                                            &Response::Modification(modification).serialize(),
                                        )
                                        .await
                                        .unwrap();
                                }
                            }

                            Response::Action(action.take().unwrap())
                        }
                    };

                    // Write response
                    stream.write_all(&response.serialize()).await.unwrap();
                }
                FrameResult::Incomplete => continue 'outer,
                FrameResult::TooLarge(size) => {
                    panic!("Frame too large: {size}")
                }
            }
        }
    }
}
