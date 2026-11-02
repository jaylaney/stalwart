/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

// Zero-access fork (upgrade Task 2): 0.16's test-only milter codec, which
// 1.0 moved into a private test-support slot. The mock milter in the tests
// crate (`tests/src/utils/milter.rs`) decodes the server's commands with
// `Command::deserialize` and encodes its replies with `Response::serialize`.
// 1.0 left the protocol types unchanged, so the code is 0.16's.

use super::*;
use std::net::IpAddr;

impl Command<'_> {
    pub fn deserialize(bytes: &[u8]) -> Command<'_> {
        let mut reader = PacketReader::new(bytes);
        match reader.byte() {
            SMFIC_ABORT => Command::Abort,
            SMFIC_BODY => Command::Body { value: &bytes[1..] },
            SMFIC_BODYEOB => Command::EndOfBody,
            SMFIC_CONNECT => {
                let hostname = reader.read_nul_terminated().unwrap();
                let family = reader.byte();
                let port = reader.read_u16();
                let address = std::str::from_utf8(reader.read_nul_terminated().unwrap()).unwrap();
                Command::Connect {
                    hostname,
                    port,
                    address: match family {
                        b'4' => IpAddr::V4(address.parse().unwrap()),
                        b'6' => IpAddr::V6(address.parse().unwrap()),
                        _ => unreachable!(),
                    },
                }
            }
            SMFIC_MACRO => {
                let cmdcode = reader.byte();
                let mut macros = Vec::new();
                while let Some(name) = reader.read_nul_terminated() {
                    let value = reader.read_nul_terminated().unwrap();
                    macros.push(crate::inbound::milter::Macro {
                        name,
                        value: value.into(),
                    });
                }
                Command::Macro {
                    macros: crate::inbound::milter::Macros { cmdcode, macros },
                }
            }
            SMFIC_HEADER => {
                let name = reader.read_nul_terminated().unwrap();
                let value = reader.read_nul_terminated().unwrap();
                Command::Header { name, value }
            }
            SMFIC_EOH => Command::EndOfHeader,
            SMFIC_HELO => {
                let hostname = reader.read_nul_terminated().unwrap();
                Command::Helo { hostname }
            }
            SMFIC_MAIL => {
                let sender = reader.read_nul_terminated().unwrap();
                let mut args = Vec::new();
                while let Some(arg) = reader.read_nul_terminated() {
                    args.push(arg);
                }
                Command::MailFrom {
                    sender,
                    args: Some(args),
                }
            }
            SMFIC_RCPT => {
                let recipient = reader.read_nul_terminated().unwrap();
                let mut args = Vec::new();
                while let Some(arg) = reader.read_nul_terminated() {
                    args.push(arg);
                }
                Command::Rcpt {
                    recipient,
                    args: Some(args),
                }
            }
            SMFIC_OPTNEG => Command::OptionNegotiation(crate::inbound::milter::Options {
                version: reader.read_u32(),
                actions: reader.read_u32(),
                protocol: reader.read_u32(),
            }),
            SMFIC_QUIT => Command::Quit,
            SMFIC_DATA => Command::Data,
            SMFIC_QUIT_NC => Command::QuitNewConnection,
            c => panic!("Unknown command: {}", char::from(c)),
        }
    }
}

impl Response {
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Response::Action(action) => match action {
                Action::Accept => Command::build(SMFIR_ACCEPT, 0),
                Action::Continue => Command::build(SMFIR_CONTINUE, 0),
                Action::Discard => Command::build(SMFIR_DISCARD, 0),
                Action::Reject => Command::build(SMFIR_REJECT, 0),
                Action::TempFail => Command::build(SMFIR_TEMPFAIL, 0),
                Action::ReplyCode { code, text } => {
                    let mut buf = Command::build(SMFIR_REPLYCODE, text.len() as u32 + 4 + 1);
                    buf.extend(code);
                    buf.push(b' ');
                    buf.extend(text.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Action::Shutdown => Command::build(SMFIR_SHUTDOWN, 0),
                Action::ConnectionFailure => Command::build(SMFIR_CONN_FAIL, 0),
            },
            Response::Modification(modif) => match modif {
                Modification::ChangeFrom { sender, args } => {
                    let mut buf =
                        Command::build(SMFIR_CHGFROM, sender.len() as u32 + args.len() as u32 + 2);
                    buf.extend(sender.as_bytes());
                    buf.push(0x00);
                    buf.extend(args.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::AddRcpt { recipient, args } => {
                    let mut buf = Command::build(
                        SMFIR_ADDRCPT_PAR,
                        recipient.len() as u32 + args.len() as u32 + 2,
                    );
                    buf.extend(recipient.as_bytes());
                    buf.push(0x00);
                    buf.extend(args.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::DeleteRcpt { recipient } => {
                    let mut buf = Command::build(SMFIR_DELRCPT, recipient.len() as u32 + 1);
                    buf.extend(recipient.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::ReplaceBody { value } => {
                    let mut buf = Command::build(SMFIR_REPLBODY, value.len() as u32);
                    buf.extend(value);
                    buf
                }
                Modification::AddHeader { name, value } => {
                    let mut buf =
                        Command::build(SMFIR_ADDHEADER, name.len() as u32 + value.len() as u32 + 2);
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                    buf.extend(value.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::InsertHeader { index, name, value } => {
                    let mut buf = Command::build(
                        SMFIR_INSHEADER,
                        name.len() as u32
                            + value.len() as u32
                            + std::mem::size_of::<u32>() as u32
                            + 2,
                    );
                    buf.extend(index.to_be_bytes().as_slice());
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                    buf.extend(value.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::ChangeHeader { index, name, value } => {
                    let mut buf = Command::build(
                        SMFIR_CHGHEADER,
                        name.len() as u32
                            + value.len() as u32
                            + std::mem::size_of::<u32>() as u32
                            + 2,
                    );
                    buf.extend(index.to_be_bytes().as_slice());
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                    buf.extend(value.as_bytes());
                    buf.push(0x00);
                    buf
                }
                Modification::Quarantine { reason } => {
                    let mut buf = Command::build(SMFIR_QUARANTINE, reason.len() as u32 + 1);
                    buf.extend(reason.as_bytes());
                    buf.push(0x00);
                    buf
                }
            },
            Response::Progress => Command::build(SMFIR_PROGRESS, 0),
            Response::Skip => Command::build(SMFIR_SKIP, 0),
            Response::SetSymbols => Command::build(SMFIR_SETSYMLIST, 0),
            Response::OptionNegotiation(opt) => {
                let mut buf = Command::build(SMFIC_OPTNEG, 3 * std::mem::size_of::<u32>() as u32);
                buf.extend(opt.version.to_be_bytes().as_slice());
                buf.extend(opt.actions.to_be_bytes().as_slice());
                buf.extend(opt.protocol.to_be_bytes().as_slice());
                buf
            }
        }
    }
}

pub(super) struct PacketReader<'x> {
    bytes: &'x [u8],
    iter: std::iter::Enumerate<std::slice::Iter<'x, u8>>,
}

impl<'x> PacketReader<'x> {
    pub fn new(bytes: &'x [u8]) -> PacketReader<'x> {
        Self {
            bytes,
            iter: bytes.iter().enumerate(),
        }
    }

    pub fn byte(&mut self) -> u8 {
        *self.iter.next().unwrap().1
    }

    pub fn read_nul_terminated(&mut self) -> Option<&'x [u8]> {
        let (start_pos, ch) = self.iter.next()?;
        let mut end_pos = start_pos;

        if *ch != 0x00 {
            loop {
                match self.iter.next().unwrap().1 {
                    0x00 => break,
                    _ => end_pos += 1,
                }
            }
        }

        Some(&self.bytes[start_pos..end_pos + 1])
    }

    pub fn read_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        for byte in buf.iter_mut() {
            *byte = self.byte();
        }
        u32::from_be_bytes(buf)
    }

    pub fn read_u16(&mut self) -> u16 {
        let mut buf = [0u8; 2];
        for byte in buf.iter_mut() {
            *byte = self.byte();
        }
        u16::from_be_bytes(buf)
    }
}
