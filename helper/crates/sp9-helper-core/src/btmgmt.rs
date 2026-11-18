// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Packets of the kernel's Bluetooth management channel and of the raw HCI channel that the
//! cover coupling needs (encoding and decoding only; the sockets are in sp9-helperd).
//!
//!   mgmt  Read Controller Information, Add Remote OOB Data (the Temporary Key of the LE
//!         legacy OOB pairing as the 192-bit hash, kernel patch 0023), Pair Device, Unpair
//!         Device, and the events that tell a bond no longer works (Device Disconnected with
//!         reason "authentication failure", Authentication Failed)
//!   HCI   LE Extended Create Connection (the pen advertises directed to the host for about
//!         1 s; the kernel only connects to devices its scan reports, and this advert is not
//!         reported), LE Create Connection Cancel, the connection complete events
//!
//! The same packets as tools/linux/le-legacy-oob.py and the `hcitool cmd` of the coupling
//! tests. Addresses are `cover::Addr` (most significant byte first) and go out LSB first.

use crate::cover::Addr;

pub const AF_BLUETOOTH: i32 = 31;
pub const BTPROTO_HCI: i32 = 1;
pub const HCI_DEV_NONE: u16 = 0xFFFF;
pub const HCI_CHANNEL_RAW: u16 = 0;
pub const HCI_CHANNEL_CONTROL: u16 = 3;

pub const OP_READ_INDEX_LIST: u16 = 0x0003;
pub const OP_READ_INFO: u16 = 0x0004;
pub const OP_PAIR_DEVICE: u16 = 0x0019;
pub const OP_CANCEL_PAIR_DEVICE: u16 = 0x001A;
pub const OP_USER_CONFIRM_NEG_REPLY: u16 = 0x001D;
pub const OP_UNPAIR_DEVICE: u16 = 0x001B;
pub const OP_ADD_REMOTE_OOB_DATA: u16 = 0x0021;
pub const EV_CMD_COMPLETE: u16 = 0x0001;
pub const EV_CMD_STATUS: u16 = 0x0002;
pub const EV_INDEX_REMOVED: u16 = 0x0005;
pub const EV_NEW_SETTINGS: u16 = 0x0006;
pub const EV_DEVICE_DISCONNECTED: u16 = 0x000C;
pub const EV_NEW_LONG_TERM_KEY: u16 = 0x000A;
pub const EV_USER_CONFIRM_REQUEST: u16 = 0x000F;
pub const EV_AUTH_FAILED: u16 = 0x0011;
/// Reason of Device Disconnected: encryption with the stored key failed (the controller's
/// "PIN or key missing").
pub const DISCONN_AUTH_FAILURE: u8 = 0x04;

pub const STATUS_SUCCESS: u8 = 0x00;
pub const STATUS_NOT_CONNECTED: u8 = 0x02;
pub const STATUS_AUTH_FAILED: u8 = 0x05;
pub const STATUS_NOT_PAIRED: u8 = 0x06;
pub const STATUS_BUSY: u8 = 0x0A;
pub const STATUS_INVALID_PARAMS: u8 = 0x0D;
pub const STATUS_DISCONNECTED: u8 = 0x0E;
pub const STATUS_ALREADY_PAIRED: u8 = 0x13;

pub const ADDR_LE_PUBLIC: u8 = 1;
pub const ADDR_LE_RANDOM: u8 = 2;
pub const IO_NO_INPUT_NO_OUTPUT: u8 = 3;

/// Current settings of Read Controller Information.
pub const SETTING_POWERED: u32 = 1 << 0;
pub const SETTING_BONDABLE: u32 = 1 << 4;
pub const SETTING_LE: u32 = 1 << 9;
/// The adapter uses resolvable private addresses of its own: the pen is told the public
/// address through the cover, so the pairing's confirm value would not match.
pub const SETTING_PRIVACY: u32 = 1 << 13;

fn lsb(a: &Addr) -> [u8; 6] {
    let mut b = *a;
    b.reverse();
    b
}

fn msb(b: &[u8]) -> Addr {
    let mut a: Addr = b[..6].try_into().unwrap();
    a.reverse();
    a
}

/// A management command: opcode, controller index, parameter length, parameters.
pub fn command(opcode: u16, index: u16, params: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(6 + params.len());
    p.extend(opcode.to_le_bytes());
    p.extend(index.to_le_bytes());
    p.extend((params.len() as u16).to_le_bytes());
    p.extend(params);
    p
}

/// Read Controller Index List (no controller index).
pub fn read_index_list() -> Vec<u8> {
    command(OP_READ_INDEX_LIST, HCI_DEV_NONE, &[])
}

/// Its reply parameters: count, then that many indices.
pub fn parse_index_list(params: &[u8]) -> Vec<u16> {
    let n = params.get(..2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).unwrap_or(0);
    params.get(2..).unwrap_or(&[]).chunks_exact(2).take(n).map(|b| u16::from_le_bytes([b[0], b[1]])).collect()
}

/// Give up a Pair Device that got no reply.
pub fn cancel_pair_device(index: u16, addr: &Addr, addr_type: u8) -> Vec<u8> {
    let mut p = lsb(addr).to_vec();
    p.push(addr_type);
    command(OP_CANCEL_PAIR_DEVICE, index, &p)
}

pub fn read_info(index: u16) -> Vec<u8> {
    command(OP_READ_INFO, index, &[])
}

/// Add Remote OOB Data, extended form: address, hash192 = the Temporary Key in the order it
/// is written to the cover, rand192 / hash256 / rand256 zero. A zero key drops the data.
pub fn add_remote_oob(index: u16, addr: &Addr, addr_type: u8, tk: &[u8; 16]) -> Vec<u8> {
    let mut p = lsb(addr).to_vec();
    p.push(addr_type);
    p.extend(tk);
    p.extend([0u8; 48]);
    command(OP_ADD_REMOTE_OOB_DATA, index, &p)
}

pub fn pair_device(index: u16, addr: &Addr, addr_type: u8, io_capability: u8) -> Vec<u8> {
    let mut p = lsb(addr).to_vec();
    p.extend([addr_type, io_capability]);
    command(OP_PAIR_DEVICE, index, &p)
}

/// Refuse a pairing the kernel asks the user to confirm (Just Works: the peer answered
/// without the OOB flag, so the Temporary Key is not in use).
pub fn user_confirm_neg_reply(index: u16, addr: &Addr, addr_type: u8) -> Vec<u8> {
    let mut p = lsb(addr).to_vec();
    p.push(addr_type);
    command(OP_USER_CONFIRM_NEG_REPLY, index, &p)
}

pub fn unpair_device(index: u16, addr: &Addr, addr_type: u8, disconnect: bool) -> Vec<u8> {
    let mut p = lsb(addr).to_vec();
    p.extend([addr_type, u8::from(disconnect)]);
    command(OP_UNPAIR_DEVICE, index, &p)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Command Complete or Command Status of `opcode`; `params` is empty for a status.
    Reply {
        opcode: u16,
        status: u8,
        params: Vec<u8>,
    },
    /// Pairing with this device failed.
    AuthFailed {
        addr: Addr,
        addr_type: u8,
        status: u8,
    },
    /// The link went down; `reason` `DISCONN_AUTH_FAILURE` is a key the peer no longer has.
    Disconnected {
        addr: Addr,
        addr_type: u8,
        reason: u8,
    },
    /// The peer gave its long term key: the pairing went through with bonding.
    NewLongTermKey {
        addr: Addr,
    },
    /// The kernel wants a pairing confirmed by the user: the pairing is not using OOB.
    UserConfirmRequest {
        addr: Addr,
        addr_type: u8,
    },
    /// The adapter's settings changed (powered, bondable, ...): the current settings.
    NewSettings(u32),
    /// The adapter went away.
    IndexRemoved,
    Other(u16),
}

/// One packet read from the management socket: (controller index, event).
pub fn parse_event(pkt: &[u8]) -> Option<(u16, Event)> {
    if pkt.len() < 6 {
        return None;
    }
    let code = u16::from_le_bytes([pkt[0], pkt[1]]);
    let index = u16::from_le_bytes([pkt[2], pkt[3]]);
    let len = u16::from_le_bytes([pkt[4], pkt[5]]) as usize;
    let body = pkt.get(6..6 + len)?;
    let ev = match code {
        EV_CMD_COMPLETE | EV_CMD_STATUS if body.len() >= 3 => {
            Event::Reply { opcode: u16::from_le_bytes([body[0], body[1]]), status: body[2], params: body[3..].to_vec() }
        }
        // store hint (1), then the key: address (6), address type, ...
        EV_NEW_LONG_TERM_KEY if body.len() >= 8 => Event::NewLongTermKey { addr: msb(&body[1..]) },
        EV_AUTH_FAILED if body.len() >= 8 => Event::AuthFailed { addr: msb(body), addr_type: body[6], status: body[7] },
        EV_DEVICE_DISCONNECTED if body.len() >= 8 => Event::Disconnected { addr: msb(body), addr_type: body[6], reason: body[7] },
        EV_USER_CONFIRM_REQUEST if body.len() >= 7 => Event::UserConfirmRequest { addr: msb(body), addr_type: body[6] },
        EV_NEW_SETTINGS if body.len() >= 4 => Event::NewSettings(u32::from_le_bytes(body[..4].try_into().unwrap())),
        EV_INDEX_REMOVED => Event::IndexRemoved,
        _ => Event::Other(code),
    };
    Some((index, ev))
}

/// What the coupling needs from Read Controller Information.
#[derive(Clone, Debug, PartialEq)]
pub struct ControllerInfo {
    pub address: Addr,
    pub settings: u32,
}

impl ControllerInfo {
    pub fn has(&self, setting: u32) -> bool {
        self.settings & setting != 0
    }
}

/// Reply parameters: address 6, version 1, manufacturer 2, supported settings 4, current
/// settings 4, ...
pub fn parse_info(params: &[u8]) -> Option<ControllerInfo> {
    if params.len() < 17 {
        return None;
    }
    Some(ControllerInfo { address: msb(params), settings: u32::from_le_bytes(params[13..17].try_into().unwrap()) })
}

pub fn status_name(status: u8) -> String {
    let n = match status {
        0x00 => "success",
        0x01 => "unknown command",
        0x02 => "not connected",
        0x03 => "failed",
        0x04 => "connect failed",
        0x05 => "authentication failed",
        0x06 => "not paired",
        0x08 => "timeout",
        0x09 => "already connected",
        0x0A => "busy",
        0x0B => "rejected",
        0x0C => "not supported",
        0x0D => "invalid parameters",
        0x0E => "disconnected",
        0x0F => "not powered",
        0x10 => "cancelled",
        0x11 => "invalid index",
        0x13 => "already paired",
        0x14 => "permission denied",
        _ => return format!("status 0x{status:02x}"),
    };
    n.to_string()
}

// ---------------------------------------------------------------- raw HCI --

pub const HCI_COMMAND_PKT: u8 = 0x01;
pub const HCI_EVENT_PKT: u8 = 0x04;
pub const OP_LE_CREATE_CONN_CANCEL: u16 = 0x200E;
pub const OP_LE_EXT_CREATE_CONN: u16 = 0x2043;
pub const EVT_CMD_COMPLETE: u8 = 0x0E;
pub const EVT_CMD_STATUS: u8 = 0x0F;
pub const EVT_LE_META: u8 = 0x3E;
pub const HCI_UNKNOWN_CONN_ID: u8 = 0x02;

/// An HCI command packet for a raw socket.
pub fn hci_command(opcode: u16, params: &[u8]) -> Vec<u8> {
    let mut p = vec![HCI_COMMAND_PKT];
    p.extend(opcode.to_le_bytes());
    p.push(params.len() as u8);
    p.extend(params);
    p
}

/// LE Extended Create Connection to a random address on the 1M PHY, own address public, no
/// filter accept list: scan interval 22.5 ms, window 11.25 ms, connection interval 15 ms,
/// latency 0, supervision timeout 1 s (the values the pen settles on with Windows).
pub fn le_ext_create_conn(peer: &Addr, peer_random: bool) -> Vec<u8> {
    let mut p = vec![0x00, 0x00, u8::from(peer_random)];
    p.extend(lsb(peer));
    p.push(0x01); // initiating PHYs: LE 1M
    for v in [0x0024u16, 0x0012, 0x000C, 0x000C, 0x0000, 0x0064, 0x0000, 0x0000] {
        p.extend(v.to_le_bytes());
    }
    hci_command(OP_LE_EXT_CREATE_CONN, &p)
}

pub fn le_create_conn_cancel() -> Vec<u8> {
    hci_command(OP_LE_CREATE_CONN_CANCEL, &[])
}

/// `struct hci_filter` for a raw socket that only wants Command Complete, Command Status
/// and LE Meta events.
pub fn hci_event_filter() -> [u8; 16] {
    let mut f = [0u8; 16];
    f[0..4].copy_from_slice(&(1u32 << HCI_EVENT_PKT).to_ne_bytes());
    let lo: u32 = (1 << EVT_CMD_COMPLETE) | (1 << EVT_CMD_STATUS);
    let hi: u32 = 1 << (EVT_LE_META - 32);
    f[4..8].copy_from_slice(&lo.to_ne_bytes());
    f[8..12].copy_from_slice(&hi.to_ne_bytes());
    f
}

#[derive(Clone, Debug, PartialEq)]
pub enum HciEvent {
    /// Command Status, or the status byte of a Command Complete
    CommandStatus { opcode: u16, status: u8 },
    /// LE (Enhanced) Connection Complete
    LeConnection { status: u8, handle: u16, peer_random: bool, peer: Addr },
}

/// One packet read from the raw HCI socket.
pub fn parse_hci_event(pkt: &[u8]) -> Option<HciEvent> {
    if pkt.len() < 3 || pkt[0] != HCI_EVENT_PKT {
        return None;
    }
    let body = pkt.get(3..3 + pkt[2] as usize)?;
    match pkt[1] {
        EVT_CMD_STATUS if body.len() >= 4 => {
            Some(HciEvent::CommandStatus { opcode: u16::from_le_bytes([body[2], body[3]]), status: body[0] })
        }
        EVT_CMD_COMPLETE if body.len() >= 4 => {
            Some(HciEvent::CommandStatus { opcode: u16::from_le_bytes([body[1], body[2]]), status: body[3] })
        }
        // subevents 0x01 (Connection Complete), 0x0A and 0x29 (Enhanced, v1 and v2) start alike:
        // status, handle, role, peer address type, peer address
        EVT_LE_META if body.len() >= 12 && matches!(body[0], 0x01 | 0x0A | 0x29) => Some(HciEvent::LeConnection {
            status: body[1],
            handle: u16::from_le_bytes([body[2], body[3]]),
            peer_random: body[5] & 1 == 1,
            peer: msb(&body[6..12]),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cover::parse_addr;

    fn addr() -> Addr {
        parse_addr("ED:08:D9:17:E1:E7").unwrap()
    }

    /// tools/linux/le-legacy-oob.py: struct.pack("<HHH", opcode, index, len) + params.
    #[test]
    fn mgmt_commands() {
        assert_eq!(read_info(0), vec![0x04, 0, 0, 0, 0, 0]);
        let tk: [u8; 16] = std::array::from_fn(|i| i as u8 + 1);
        let p = add_remote_oob(0, &addr(), ADDR_LE_RANDOM, &tk);
        assert_eq!(p.len(), 6 + 71);
        assert_eq!(&p[..6], &[0x21, 0x00, 0x00, 0x00, 71, 0x00]);
        assert_eq!(&p[6..13], &[0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2]);
        assert_eq!(&p[13..29], &tk);
        assert!(p[29..].iter().all(|b| *b == 0));
        assert_eq!(
            pair_device(0, &addr(), ADDR_LE_RANDOM, IO_NO_INPUT_NO_OUTPUT),
            vec![0x19, 0, 0, 0, 8, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2, 3]
        );
        assert_eq!(unpair_device(1, &addr(), ADDR_LE_RANDOM, true), vec![0x1B, 0, 1, 0, 8, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2, 1]);
    }

    #[test]
    fn mgmt_events() {
        // Command Complete of Add Remote OOB Data: Invalid Parameters (a kernel without the TK)
        let pkt = [0x01, 0, 0, 0, 10, 0, 0x21, 0x00, 0x0D, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2];
        let (idx, ev) = parse_event(&pkt).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(ev, Event::Reply { opcode: OP_ADD_REMOTE_OOB_DATA, status: STATUS_INVALID_PARAMS, params: pkt[9..].to_vec() });
        // Command Status
        assert_eq!(
            parse_event(&[0x02, 0, 0, 0, 3, 0, 0x19, 0x00, 0x0A]).unwrap().1,
            Event::Reply { opcode: OP_PAIR_DEVICE, status: 0x0A, params: vec![] }
        );
        // Authentication Failed
        let pkt = [0x11, 0, 0, 0, 8, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2, 0x05];
        assert_eq!(parse_event(&pkt).unwrap().1, Event::AuthFailed { addr: addr(), addr_type: ADDR_LE_RANDOM, status: 0x05 });
        // Device Disconnected, reason authentication failure
        let pkt = [0x0C, 0, 0, 0, 8, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2, DISCONN_AUTH_FAILURE];
        assert_eq!(parse_event(&pkt).unwrap().1, Event::Disconnected { addr: addr(), addr_type: ADDR_LE_RANDOM, reason: 4 });
        // User Confirmation Request: address, type, confirm hint, value
        let pkt = [0x0F, 0, 0, 0, 12, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2, 1, 0, 0, 0, 0];
        assert_eq!(parse_event(&pkt).unwrap().1, Event::UserConfirmRequest { addr: addr(), addr_type: ADDR_LE_RANDOM });
        assert_eq!(user_confirm_neg_reply(0, &addr(), ADDR_LE_RANDOM), vec![0x1D, 0, 0, 0, 7, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2]);
        // New Long Term Key: store hint, address, address type, ...
        let mut pkt = vec![0x0A, 0, 0, 0, 37, 0, 1, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2];
        pkt.extend([0u8; 29]);
        assert_eq!(parse_event(&pkt).unwrap().1, Event::NewLongTermKey { addr: addr() });
        assert_eq!(parse_event(&[0x06, 0, 0, 0, 4, 0, 0x10, 0x02, 0, 0]).unwrap().1, Event::NewSettings(0x0210));
        assert_eq!(parse_event(&[0x05, 0, 1, 0, 0, 0]).unwrap(), (1, Event::IndexRemoved));
        assert_eq!(parse_event(&[0x0B, 0, 0, 0, 0, 0]).unwrap().1, Event::Other(0x0B));
        assert!(parse_event(&[0x01, 0, 0, 0, 10, 0, 0x21]).is_none(), "truncated");
        assert_eq!(status_name(0x0D), "invalid parameters");
        assert_eq!(status_name(0x77), "status 0x77");
    }

    #[test]
    fn index_list_and_cancel() {
        assert_eq!(read_index_list(), vec![0x03, 0, 0xFF, 0xFF, 0, 0]);
        assert_eq!(parse_index_list(&[2, 0, 1, 0, 0, 0]), vec![1, 0]);
        assert_eq!(parse_index_list(&[0, 0]), Vec::<u16>::new());
        assert_eq!(parse_index_list(&[3, 0, 1, 0]), vec![1], "a short reply");
        assert_eq!(cancel_pair_device(1, &addr(), ADDR_LE_RANDOM), vec![0x1A, 0, 1, 0, 7, 0, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 2]);
    }

    #[test]
    fn controller_info() {
        let mut p = vec![0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x0C, 0x02, 0x00];
        p.extend(0xFFFFu32.to_le_bytes());
        p.extend((SETTING_POWERED | SETTING_BONDABLE | SETTING_LE).to_le_bytes());
        p.extend([0u8; 3]);
        let i = parse_info(&p).unwrap();
        assert_eq!(crate::cover::format_addr(&i.address), "11:22:33:44:55:66");
        assert!(i.has(SETTING_POWERED) && i.has(SETTING_BONDABLE) && !i.has(1 << 1));
        assert!(parse_info(&p[..10]).is_none());
    }

    /// The command of the coupling test that connected:
    /// hcitool cmd 0x08 0x0043 00 00 01 <peer LSB first> 01 24 00 12 00 0c 00 0c 00 00 00 64 00 00 00 00 00
    #[test]
    fn hci_create_connection() {
        let p = le_ext_create_conn(&addr(), true);
        let mut want = vec![0x01, 0x43, 0x20, 26, 0x00, 0x00, 0x01, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED, 0x01];
        want.extend([0x24, 0x00, 0x12, 0x00, 0x0C, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(p, want);
        assert_eq!(le_create_conn_cancel(), vec![0x01, 0x0E, 0x20, 0x00]);
        let f = hci_event_filter();
        assert_eq!(u32::from_ne_bytes(f[0..4].try_into().unwrap()), 0x10);
        assert_eq!(u32::from_ne_bytes(f[4..8].try_into().unwrap()), 0xC000);
        assert_eq!(u32::from_ne_bytes(f[8..12].try_into().unwrap()), 0x4000_0000);
    }

    #[test]
    fn hci_events() {
        // Command Status: status, ncmd, opcode
        assert_eq!(
            parse_hci_event(&[0x04, 0x0F, 4, 0x00, 0x01, 0x43, 0x20]),
            Some(HciEvent::CommandStatus { opcode: OP_LE_EXT_CREATE_CONN, status: 0 })
        );
        // Command Complete: ncmd, opcode, status
        assert_eq!(
            parse_hci_event(&[0x04, 0x0E, 4, 0x01, 0x0E, 0x20, 0x0C]),
            Some(HciEvent::CommandStatus { opcode: OP_LE_CREATE_CONN_CANCEL, status: 0x0C })
        );
        // LE Enhanced Connection Complete
        let mut pkt = vec![0x04, 0x3E, 31, 0x0A, 0x00, 0x00, 0x08, 0x00, 0x01, 0xE7, 0xE1, 0x17, 0xD9, 0x08, 0xED];
        pkt.extend([0u8; 19]);
        assert_eq!(parse_hci_event(&pkt), Some(HciEvent::LeConnection { status: 0, handle: 0x0800, peer_random: true, peer: addr() }));
        // other subevents and packet types are not ours
        assert_eq!(parse_hci_event(&[0x04, 0x3E, 12, 0x0D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(parse_hci_event(&[0x02, 0x00, 0x00]), None);
    }
}
