//! USBTMC / USB488 interface (class 0xFE, subclass 3, protocol 1).
//!
//! Bulk-OUT carries DEV_DEP_MSG_OUT (a command) and REQUEST_DEV_DEP_MSG_IN (the
//! host asking for a response); Bulk-IN carries DEV_DEP_MSG_IN. Every message
//! has a 12-byte header and is padded to a multiple of 4. The application side
//! is two calls: `take_command` for a complete command (EOM seen) and
//! `respond` to queue the response text.

use usb_device::class_prelude::*;
use usb_device::control::{Recipient, RequestType};
use usb_device::Result;

const CLASS: u8 = 0xFE;
const SUBCLASS: u8 = 0x03;
const PROTOCOL_USB488: u8 = 0x01;

const DEV_DEP_MSG_OUT: u8 = 1;
const REQUEST_DEV_DEP_MSG_IN: u8 = 2;
const DEV_DEP_MSG_IN: u8 = 2;

// Class requests.
const INITIATE_ABORT_BULK_OUT: u8 = 1;
const CHECK_ABORT_BULK_OUT_STATUS: u8 = 2;
const INITIATE_ABORT_BULK_IN: u8 = 3;
const CHECK_ABORT_BULK_IN_STATUS: u8 = 4;
const INITIATE_CLEAR: u8 = 5;
const CHECK_CLEAR_STATUS: u8 = 6;
const GET_CAPABILITIES: u8 = 7;
const INDICATOR_PULSE: u8 = 64;
const READ_STATUS_BYTE: u8 = 128;
const REN_CONTROL: u8 = 160;
const GO_TO_LOCAL: u8 = 161;
const LOCAL_LOCKOUT: u8 = 162;

const STATUS_SUCCESS: u8 = 0x01;
const STATUS_TRANSFER_NOT_IN_PROGRESS: u8 = 0x81;

const MPS: usize = 64;
pub const CMD_MAX: usize = 256;
pub const RSP_MAX: usize = 512;

const CAPABILITIES: [u8; 24] = [
    STATUS_SUCCESS, 0,
    0x00, 0x01,       // bcdUSBTMC 1.00
    0x04,             // interface: INDICATOR_PULSE
    0x01,             // device: TermChar supported
    0, 0, 0, 0, 0, 0,
    0x00, 0x01,       // bcdUSB488 1.00
    0x06,             // USB488.2 interface, REN/GTL/LLO accepted
    0x08,             // SCPI
    0, 0, 0, 0, 0, 0, 0, 0,
];

pub struct Usbtmc<'a, B: UsbBus> {
    iface: InterfaceNumber,
    ep_out: EndpointOut<'a, B>,
    ep_in: EndpointIn<'a, B>,

    // Bulk-OUT message being received.
    hdr: [u8; 12],
    hdr_len: usize,
    out_left: u32, // payload bytes still to come
    out_pad: u32,  // padding bytes still to skip
    out_eom: bool,
    cmd: [u8; CMD_MAX],
    cmd_len: usize,
    cmd_ready: bool,

    // Bulk-IN side.
    req_tag: Option<u8>, // REQUEST_DEV_DEP_MSG_IN waiting
    req_max: u32,
    rsp: [u8; RSP_MAX],
    rsp_len: usize,
    rsp_ready: bool,
    tx: [u8; RSP_MAX + 16],
    tx_len: usize,
    tx_off: usize,
    tx_zlp: bool,
    last_tag_in: u8,
    pub status_byte: u8,
    /// Debug: count of control requests seen and the last four
    /// (bmRequestType, bRequest, wIndex lo, wLength lo each).
    pub dbg_ctl: (u8, [u8; 16]),
}

impl<'a, B: UsbBus> Usbtmc<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Usbtmc {
            iface: alloc.interface(),
            ep_out: alloc.bulk(MPS as u16),
            ep_in: alloc.bulk(MPS as u16),
            hdr: [0; 12],
            hdr_len: 0,
            out_left: 0,
            out_pad: 0,
            out_eom: false,
            cmd: [0; CMD_MAX],
            cmd_len: 0,
            cmd_ready: false,
            req_tag: None,
            req_max: 0,
            rsp: [0; RSP_MAX],
            rsp_len: 0,
            rsp_ready: false,
            tx: [0; RSP_MAX + 16],
            tx_len: 0,
            tx_off: 0,
            tx_zlp: false,
            last_tag_in: 0,
            status_byte: 0,
            dbg_ctl: (0, [0; 16]),
        }
    }

    /// A complete command (up to the EOM message), if one arrived.
    pub fn take_command(&mut self) -> Option<&[u8]> {
        if self.cmd_ready {
            self.cmd_ready = false;
            let n = self.cmd_len;
            self.cmd_len = 0;
            Some(&self.cmd[..n])
        } else {
            None
        }
    }

    /// True while a command is being processed and the response is not queued.
    pub fn response_pending(&self) -> bool {
        self.rsp_ready
    }

    /// Queue response text; sent when the host requests it.
    pub fn respond(&mut self, data: &[u8]) {
        let n = data.len().min(RSP_MAX);
        self.rsp[..n].copy_from_slice(&data[..n]);
        self.rsp_len = n;
        self.rsp_ready = true;
        self.try_start_in();
    }

    fn clear(&mut self) {
        self.hdr_len = 0;
        self.out_left = 0;
        self.out_pad = 0;
        self.cmd_len = 0;
        self.cmd_ready = false;
        self.req_tag = None;
        self.rsp_ready = false;
        self.tx_len = 0;
        self.tx_off = 0;
        self.tx_zlp = false;
    }

    fn on_out_bytes(&mut self, mut d: &[u8]) {
        while !d.is_empty() {
            if self.out_left == 0 && self.out_pad == 0 {
                // Header.
                let k = (12 - self.hdr_len).min(d.len());
                self.hdr[self.hdr_len..self.hdr_len + k].copy_from_slice(&d[..k]);
                self.hdr_len += k;
                d = &d[k..];
                if self.hdr_len < 12 {
                    return;
                }
                self.hdr_len = 0;
                let h = self.hdr;
                if h[1] != !h[2] {
                    continue; // bad tag pair: drop
                }
                let size = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
                match h[0] {
                    DEV_DEP_MSG_OUT => {
                        self.out_left = size;
                        self.out_pad = (4 - size % 4) % 4;
                        self.out_eom = h[8] & 1 != 0;
                        if size == 0 && self.out_eom {
                            self.cmd_ready = true;
                        }
                    }
                    REQUEST_DEV_DEP_MSG_IN => {
                        self.req_tag = Some(h[1]);
                        self.req_max = size;
                        self.try_start_in();
                    }
                    _ => {}
                }
            } else if self.out_left > 0 {
                let k = (self.out_left as usize).min(d.len());
                let room = CMD_MAX - self.cmd_len;
                let c = k.min(room);
                self.cmd[self.cmd_len..self.cmd_len + c].copy_from_slice(&d[..c]);
                self.cmd_len += c;
                self.out_left -= k as u32;
                d = &d[k..];
                if self.out_left == 0 && self.out_pad == 0 && self.out_eom {
                    self.cmd_ready = true;
                }
            } else {
                let k = (self.out_pad as usize).min(d.len());
                self.out_pad -= k as u32;
                d = &d[k..];
                if self.out_pad == 0 && self.out_eom {
                    self.cmd_ready = true;
                }
            }
        }
    }

    fn try_start_in(&mut self) {
        if self.tx_len != 0 || !self.rsp_ready {
            return;
        }
        let Some(tag) = self.req_tag.take() else { return };
        let n = self.rsp_len.min(self.req_max as usize);
        let t = &mut self.tx;
        t[0] = DEV_DEP_MSG_IN;
        t[1] = tag;
        t[2] = !tag;
        t[3] = 0;
        t[4..8].copy_from_slice(&(n as u32).to_le_bytes());
        t[8] = 1; // EOM
        t[9..12].fill(0);
        t[12..12 + n].copy_from_slice(&self.rsp[..n]);
        let mut len = 12 + n;
        while len % 4 != 0 {
            t[len] = 0;
            len += 1;
        }
        self.tx_len = len;
        self.tx_off = 0;
        self.tx_zlp = len % MPS == 0;
        self.last_tag_in = tag;
        self.rsp_ready = false;
        self.pump_in();
    }

    fn pump_in(&mut self) {
        if self.tx_off < self.tx_len {
            let end = (self.tx_off + MPS).min(self.tx_len);
            if let Ok(n) = self.ep_in.write(&self.tx[self.tx_off..end]) {
                self.tx_off += n;
            }
        } else if self.tx_len != 0 {
            if self.tx_zlp {
                if self.ep_in.write(&[]).is_ok() {
                    self.tx_zlp = false;
                }
            } else {
                self.tx_len = 0;
                self.tx_off = 0;
            }
        }
    }
}

impl<B: UsbBus> Usbtmc<'_, B> {
    fn log(&mut self, req: &usb_device::control::Request, dir: u8) {
        self.dbg_ctl.0 = self.dbg_ctl.0.wrapping_add(1);
        self.dbg_ctl.1.copy_within(4..16, 0);
        self.dbg_ctl.1[12..16].copy_from_slice(&[
            dir | (req.request_type as u8) << 5 | req.recipient as u8,
            req.request,
            req.index as u8,
            req.length as u8,
        ]);
    }
}

impl<B: UsbBus> UsbClass<B> for Usbtmc<'_, B> {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> Result<()> {
        w.interface(self.iface, CLASS, SUBCLASS, PROTOCOL_USB488)?;
        w.endpoint(&self.ep_out)?;
        w.endpoint(&self.ep_in)?;
        Ok(())
    }

    fn reset(&mut self) {
        self.clear();
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = *xfer.request();
        self.log(&req, 0);
    }

    fn endpoint_out(&mut self, addr: EndpointAddress) {
        if addr != self.ep_out.address() {
            return;
        }
        let mut buf = [0u8; MPS];
        if let Ok(n) = self.ep_out.read(&mut buf) {
            self.on_out_bytes(&buf[..n]);
        }
    }

    fn endpoint_in_complete(&mut self, addr: EndpointAddress) {
        if addr == self.ep_in.address() {
            self.pump_in();
        }
    }

    fn poll(&mut self) {
        self.try_start_in();
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = *xfer.request();
        self.log(&req, 0x80);
        if req.request_type != RequestType::Class {
            return;
        }
        let ours = match req.recipient {
            Recipient::Interface => req.index == u8::from(self.iface) as u16,
            Recipient::Endpoint => {
                req.index as u8 == u8::from(self.ep_in.address())
                    || req.index as u8 == u8::from(self.ep_out.address())
            }
            _ => false,
        };
        if !ours {
            return;
        }
        let tag = req.value as u8;
        let _ = match req.request {
            GET_CAPABILITIES => xfer.accept_with(&CAPABILITIES),
            INITIATE_CLEAR => {
                self.clear();
                xfer.accept_with(&[STATUS_SUCCESS])
            }
            CHECK_CLEAR_STATUS => xfer.accept_with(&[STATUS_SUCCESS, 0]),
            INITIATE_ABORT_BULK_OUT => {
                self.out_left = 0;
                self.out_pad = 0;
                self.hdr_len = 0;
                xfer.accept_with(&[STATUS_TRANSFER_NOT_IN_PROGRESS, tag])
            }
            CHECK_ABORT_BULK_OUT_STATUS => xfer.accept_with(&[STATUS_SUCCESS, 0, 0, 0, 0, 0, 0, 0]),
            INITIATE_ABORT_BULK_IN => {
                let last = self.last_tag_in;
                self.tx_len = 0;
                self.tx_off = 0;
                self.tx_zlp = false;
                xfer.accept_with(&[STATUS_TRANSFER_NOT_IN_PROGRESS, last])
            }
            CHECK_ABORT_BULK_IN_STATUS => xfer.accept_with(&[STATUS_SUCCESS, 0, 0, 0, 0, 0, 0, 0]),
            INDICATOR_PULSE => xfer.accept_with(&[STATUS_SUCCESS]),
            READ_STATUS_BYTE => {
                let sb = self.status_byte | if self.rsp_ready { 0x10 } else { 0 }; // MAV
                xfer.accept_with(&[STATUS_SUCCESS, tag, sb])
            }
            REN_CONTROL | GO_TO_LOCAL | LOCAL_LOCKOUT => xfer.accept_with(&[STATUS_SUCCESS]),
            _ => xfer.reject(),
        };
    }
}
