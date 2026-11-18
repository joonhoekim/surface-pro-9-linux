// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/ipts/parser.hpp, src/ipts/protocol/*.hpp, src/common/reader.hpp),
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Parser for the raw reports of the digitizer.
//!
//! A report is a three byte header (report ID, 16 bit timestamp) followed by one HID frame.
//! HID frames nest; the leaves are a heatmap frame (the capacitive image) or a list of report
//! frames (heatmap dimensions, DFT metadata, DFT windows of the pen, and types that are skipped).
//!
//! The parser hands out [`Event`]s while it walks the report. A structural error ends the walk
//! with an [`Error`]; events that were handed out before it stay valid, as in the reference
//! implementation, where the error surfaces as an exception after the callbacks ran.

use std::fmt;

/// Antennas per row of a DFT window.
pub const NUM_COMPONENTS: usize = 9;
/// Index of the antenna a row is centered on.
pub const CENTER: usize = NUM_COMPONENTS / 2;
/// Rows of a pressure window that carry the pressure tone.
pub const PRESSURE_ROWS: usize = 6;

const REPORT_HEADER: usize = 3;
const HID_FRAME_HEADER: usize = 7;
const HEATMAP_FRAME_HEADER: usize = 9;
const REPORT_FRAME_HEADER: usize = 4;
const DIMENSIONS_SIZE: usize = 8;
const DFT_METADATA_SIZE: usize = 16;
const DFT_WINDOW_HEADER: usize = 12;
const ROW_SIZE: usize = 48;
const METADATA_SIZE: usize = 105;

const HID_FRAME_HID: u8 = 0x00;
const HID_FRAME_HEATMAP: u8 = 0x01;
const HID_FRAME_METADATA: u8 = 0x02;
const HID_FRAME_REPORTS: u8 = 0xFF;

const REPORT_HEATMAP_DIMENSIONS: u8 = 0x03;
const REPORT_HEATMAP_DATA: u8 = 0x25;
const REPORT_DFT_WINDOW: u8 = 0x5C;
const REPORT_DFT_METADATA: u8 = 0x5F;

/// The kind of a DFT window (`data_type` of the window header).
pub mod window {
    pub const POSITION: u8 = 0x06;
    pub const POSITION_MPP2: u8 = 0x07;
    pub const POSITION_MPP2_SECOND: u8 = 0x08;
    pub const BUTTON: u8 = 0x09;
    pub const BINARY_MPP2: u8 = 0x0A;
    pub const PRESSURE: u8 = 0x0B;
}

/// Why the walk through a report ended early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A read at the very end of a frame.
    EndOfData { wanted: usize },
    /// A read past the end of a frame.
    ShortRead { wanted: usize, available: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EndOfData { wanted } => write!(f, "tried to read {wanted} bytes but no data is left"),
            Error::ShortRead { wanted, available } => write!(f, "tried to read {wanted} bytes with only {available} bytes available"),
        }
    }
}

impl std::error::Error for Error {}

/// A cursor over a frame with the bounds rules of the reference reader: a read of any size
/// (also zero) at the end of the data is an error, as is a read past the end.
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data }
    }

    fn remaining(&self) -> usize {
        self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.is_empty() {
            return Err(Error::EndOfData { wanted: n });
        }
        if n > self.data.len() {
            return Err(Error::ShortRead { wanted: n, available: self.data.len() });
        }
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        Ok(head)
    }

    fn sub(&mut self, n: usize) -> Result<Reader<'a>, Error> {
        self.take(n).map(Reader::new)
    }
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// One tone of a DFT window on one axis: the complex amplitudes of nine neighbouring antennas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Row {
    /// The frequency of the tone, the bits of an `f32`.
    pub frequency: u32,
    pub magnitude: u32,
    pub real: [i16; NUM_COMPONENTS],
    pub imag: [i16; NUM_COMPONENTS],
    /// The antenna number of the first component.
    pub first: i8,
    pub last: i8,
    pub mid: i8,
    pub zero: i8,
}

impl Row {
    fn parse(b: &[u8]) -> Row {
        let mut row = Row { frequency: le32(&b[0..4]), magnitude: le32(&b[4..8]), ..Row::default() };
        for i in 0..NUM_COMPONENTS {
            row.real[i] = le16(&b[8 + 2 * i..]) as i16;
            row.imag[i] = le16(&b[26 + 2 * i..]) as i16;
        }
        row.first = b[44] as i8;
        row.last = b[45] as i8;
        row.mid = b[46] as i8;
        row.zero = b[47] as i8;
        row
    }

    /// The amplitude of one antenna.
    #[inline]
    pub fn amplitude(&self, i: usize) -> f64 {
        f64::from(self.real[i]).hypot(f64::from(self.imag[i]))
    }
}

/// A DFT window: the tones the pen sent in one sampling slot, on both axes.
#[derive(Clone, Copy, Debug)]
pub struct DftWindow<'a> {
    /// The counter of the frame the window belongs to, if the metadata report before it matches.
    pub group: Option<u32>,
    /// See [`window`].
    pub kind: u8,
    /// Sampling time in microseconds.
    pub timestamp: u32,
    /// Antennas on the X axis according to the last heatmap dimensions report (0 before one).
    pub columns: u8,
    /// Antennas on the Y axis according to the last heatmap dimensions report (0 before one).
    pub rows: u8,
    pub x: &'a [Row],
    pub y: &'a [Row],
}

/// A capacitive image, row-major, low values where something touches.
#[derive(Clone, Copy, Debug)]
pub struct Heatmap<'a> {
    pub rows: u8,
    pub columns: u8,
    pub min: u8,
    pub max: u8,
    pub data: &'a [u8],
}

/// The device metadata of the metadata feature report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metadata {
    pub rows: u8,
    pub columns: u8,
    /// Width of the screen in centimeters.
    pub width: f64,
    /// Height of the screen in centimeters.
    pub height: f64,
    pub invert_x: bool,
    pub invert_y: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    Heatmap(Heatmap<'a>),
    Dft(DftWindow<'a>),
    Metadata(Metadata),
}

#[derive(Clone, Copy, Debug, Default)]
struct Dimensions {
    rows: u8,
    columns: u8,
    z_min: u8,
    z_max: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct DftMetadata {
    group_counter: u32,
    seq_num: u8,
    data_type: u8,
}

/// The parser. It keeps the last heatmap dimensions and the last DFT metadata report across
/// reports: both describe data that follows in later frames.
#[derive(Debug, Default)]
pub struct Parser {
    dim: Dimensions,
    dft_meta: DftMetadata,
    rows_x: Vec<Row>,
    rows_y: Vec<Row>,
}

impl Parser {
    pub fn new() -> Self {
        Parser::default()
    }

    /// Walks a report as read from the device (report ID and timestamp first).
    pub fn parse(&mut self, report: &[u8], sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        self.parse_with_header(report, REPORT_HEADER, sink)
    }

    /// Walks the answer to the metadata feature request (report ID first).
    pub fn parse_feature(&mut self, report: &[u8], sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        self.parse_with_header(report, 1, sink)
    }

    fn parse_with_header(&mut self, report: &[u8], header: usize, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        let mut reader = Reader::new(report);
        reader.take(header)?;
        self.hid_frame(&mut reader, sink)
    }

    fn hid_frame(&mut self, reader: &mut Reader<'_>, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        let header = reader.take(HID_FRAME_HEADER)?;
        let size = le32(header) as usize;
        let kind = header[5];

        // A size below that of the header wraps around and fails as a read past the end.
        let mut sub = reader.sub(size.wrapping_sub(HID_FRAME_HEADER))?;

        match kind {
            HID_FRAME_HID => {
                while sub.remaining() > 0 {
                    self.hid_frame(&mut sub, sink)?;
                }
            }
            HID_FRAME_HEATMAP => {
                let header = sub.take(HEATMAP_FRAME_HEADER)?;
                let mut data = sub.sub(le32(&header[5..]) as usize)?;
                self.heatmap_data(&mut data, sink)?;
            }
            HID_FRAME_METADATA => Self::metadata(&mut sub, sink)?,
            HID_FRAME_REPORTS => {
                // A frame that is followed by exactly four more bytes is a malformed one that
                // some devices send once per second: it is dropped together with those bytes.
                if reader.remaining() == 4 {
                    reader.take(4)?;
                    return Ok(());
                }
                self.report_frames(&mut sub, sink)?;
            }
            _ => {}
        }

        Ok(())
    }

    fn metadata(reader: &mut Reader<'_>, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        let b = reader.take(METADATA_SIZE)?;
        let float = |o: usize| f32::from_bits(le32(&b[o..]));

        // Dimensions: rows, columns, width and height in 0.01 mm; then one byte and the
        // transform (xx, yx, tx, xy, yy, ty) as floats.
        sink(Event::Metadata(Metadata {
            rows: le32(&b[0..]) as u8,
            columns: le32(&b[4..]) as u8,
            width: f64::from(le32(&b[8..])) / 1000.0,
            height: f64::from(le32(&b[12..])) / 1000.0,
            invert_x: float(17) < 0.0,
            invert_y: float(17 + 16) < 0.0,
        }));
        Ok(())
    }

    fn report_frames(&mut self, reader: &mut Reader<'_>, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        while reader.remaining() > 0 {
            let header = reader.take(REPORT_FRAME_HEADER)?;
            let kind = header[0];
            let mut sub = reader.sub(usize::from(le16(&header[2..])))?;

            match kind {
                REPORT_HEATMAP_DIMENSIONS => {
                    let b = sub.take(DIMENSIONS_SIZE)?;
                    // Some firmware leaves the upper end of the value range at 0.
                    self.dim = Dimensions { rows: b[0], columns: b[1], z_min: b[6], z_max: if b[7] == 0 { 255 } else { b[7] } };
                }
                REPORT_HEATMAP_DATA => self.heatmap_data(&mut sub, sink)?,
                REPORT_DFT_METADATA => {
                    let b = sub.take(DFT_METADATA_SIZE)?;
                    self.dft_meta = DftMetadata { group_counter: le32(b), seq_num: b[4], data_type: b[5] };
                }
                REPORT_DFT_WINDOW => self.dft_window(&mut sub, sink)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// The size of the image is that of the last dimensions report, which comes after the
    /// image in the same frame: the first image after start has no size and is empty.
    fn heatmap_data(&mut self, reader: &mut Reader<'_>, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        let size = usize::from(self.dim.rows) * usize::from(self.dim.columns);
        let data = reader.take(size)?;
        sink(Event::Heatmap(Heatmap { rows: self.dim.rows, columns: self.dim.columns, min: self.dim.z_min, max: self.dim.z_max, data }));
        Ok(())
    }

    fn dft_window(&mut self, reader: &mut Reader<'_>, sink: &mut dyn FnMut(Event<'_>)) -> Result<(), Error> {
        let b = reader.take(DFT_WINDOW_HEADER)?;
        let timestamp = le32(b);
        let num_rows = usize::from(b[4]);
        let seq_num = b[5];
        let kind = b[9];

        Self::rows(reader, num_rows, &mut self.rows_x)?;
        Self::rows(reader, num_rows, &mut self.rows_y)?;

        let group = (seq_num == self.dft_meta.seq_num && kind == self.dft_meta.data_type).then_some(self.dft_meta.group_counter);

        sink(Event::Dft(DftWindow {
            group,
            kind,
            timestamp,
            columns: self.dim.columns,
            rows: self.dim.rows,
            x: &self.rows_x,
            y: &self.rows_y,
        }));
        Ok(())
    }

    fn rows(reader: &mut Reader<'_>, count: usize, out: &mut Vec<Row>) -> Result<(), Error> {
        out.clear();
        // The count is checked like a byte count first (an empty list at the end of the
        // frame is an error), then the rows are taken.
        if reader.remaining() == 0 {
            return Err(Error::EndOfData { wanted: count });
        }
        if count > reader.remaining() {
            return Err(Error::ShortRead { wanted: count, available: reader.remaining() });
        }
        let bytes = reader.take(count * ROW_SIZE)?;
        out.extend(bytes.chunks_exact(ROW_SIZE).map(Row::parse));
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testdata {
    //! Builders for synthetic reports.
    use super::*;

    pub fn hid_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + HID_FRAME_HEADER) as u32).to_le_bytes().to_vec();
        v.extend([0, kind, 0]);
        v.extend(payload);
        v
    }

    pub fn report_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![kind, 0];
        v.extend((payload.len() as u16).to_le_bytes());
        v.extend(payload);
        v
    }

    pub fn report(id: u8, frame: &[u8]) -> Vec<u8> {
        let mut v = vec![id, 0, 0];
        v.extend(frame);
        v
    }

    pub fn dimensions(rows: u8, columns: u8, z_min: u8, z_max: u8) -> Vec<u8> {
        report_frame(REPORT_HEATMAP_DIMENSIONS, &[rows, columns, 0, rows.wrapping_sub(1), 0, columns.wrapping_sub(1), z_min, z_max])
    }

    pub fn row_bytes(row: &Row) -> Vec<u8> {
        let mut v = row.frequency.to_le_bytes().to_vec();
        v.extend(row.magnitude.to_le_bytes());
        for r in row.real {
            v.extend(r.to_le_bytes());
        }
        for i in row.imag {
            v.extend(i.to_le_bytes());
        }
        v.extend([row.first as u8, row.last as u8, row.mid as u8, row.zero as u8]);
        v
    }

    /// A DFT metadata report and the window it announces.
    pub fn dft(group: u32, seq: u8, kind: u8, timestamp: u32, x: &[Row], y: &[Row]) -> Vec<u8> {
        let mut meta = group.to_le_bytes().to_vec();
        meta.extend([seq, kind]);
        meta.extend([0; 10]);
        let mut win = timestamp.to_le_bytes().to_vec();
        win.extend([x.len() as u8, seq, 0, 0, 0, kind, 0, 0]);
        for r in x.iter().chain(y) {
            win.extend(row_bytes(r));
        }
        let mut v = report_frame(REPORT_DFT_METADATA, &meta);
        v.extend(report_frame(REPORT_DFT_WINDOW, &win));
        v
    }

    /// A heatmap report as the device sends it: the image, then the dimensions.
    pub fn heatmap_report(rows: u8, columns: u8, z_min: u8, z_max: u8, image: &[u8]) -> Vec<u8> {
        let mut heat = vec![0; 5];
        heat.extend((image.len() as u32).to_le_bytes());
        heat.extend(image);
        let mut inner = hid_frame(HID_FRAME_HEATMAP, &heat);
        inner.extend(hid_frame(HID_FRAME_REPORTS, &dimensions(rows, columns, z_min, z_max)));
        report(0x12, &hid_frame(HID_FRAME_HID, &inner))
    }

    pub fn dft_report(frames: &[u8]) -> Vec<u8> {
        report(0x0B, &hid_frame(HID_FRAME_REPORTS, frames))
    }
}

#[cfg(test)]
mod tests {
    use super::testdata::*;
    use super::*;

    fn collect(parser: &mut Parser, report: &[u8]) -> (Vec<String>, Result<(), Error>) {
        let mut seen = Vec::new();
        let result = parser.parse(report, &mut |e| {
            seen.push(match e {
                Event::Heatmap(h) => format!("heatmap {}x{} {}..{} {}", h.rows, h.columns, h.min, h.max, h.data.len()),
                Event::Dft(d) => {
                    format!("dft {:#x} group {:?} rows {} {}x{} t {}", d.kind, d.group, d.x.len(), d.columns, d.rows, d.timestamp)
                }
                Event::Metadata(m) => format!("{m:?}"),
            })
        });
        (seen, result)
    }

    #[test]
    fn heatmap_takes_the_dimensions_of_the_report_before() {
        let mut p = Parser::new();
        let image = vec![7u8; 6];
        let report = heatmap_report(2, 3, 10, 0, &image);
        // No dimensions yet: an empty image.
        assert_eq!(collect(&mut p, &report), (vec!["heatmap 0x0 0..0 0".to_string()], Ok(())));
        // z_max 0 reads as 255.
        assert_eq!(collect(&mut p, &report), (vec!["heatmap 2x3 10..255 6".to_string()], Ok(())));
    }

    #[test]
    fn dft_window_and_group() {
        let mut p = Parser::new();
        let row = Row {
            frequency: 1,
            magnitude: 2,
            real: [1, 2, 3, 4, 5, 6, 7, 8, 9],
            imag: [-1, -2, -3, -4, -5, -6, -7, -8, -9],
            first: 10,
            last: 18,
            mid: 14,
            zero: 0,
        };
        let mut frames = dft(77, 3, window::POSITION, 1234, &[row, row], &[row, row]);
        // A window whose metadata does not match has no group.
        let mut other = dft(78, 4, window::BUTTON, 1300, &[row], &[row]);
        other[4 + 4] = 9; // the sequence number of the metadata report
        frames.extend(other);
        let (seen, result) = collect(&mut p, &dft_report(&frames));
        assert_eq!(result, Ok(()));
        assert_eq!(seen, vec!["dft 0x6 group Some(77) rows 2 0x0 t 1234", "dft 0x9 group None rows 1 0x0 t 1300"]);

        let mut got = None;
        p.parse(&dft_report(&dft(1, 1, window::PRESSURE, 5, &[row], &[row])), &mut |e| {
            if let Event::Dft(d) = e {
                got = Some((d.x[0], d.y[0]));
            }
        })
        .unwrap();
        assert_eq!(got, Some((row, row)));
    }

    #[test]
    fn errors_keep_earlier_events() {
        let mut p = Parser::new();
        let row = Row::default();
        let mut frames = dft(1, 1, window::POSITION, 5, &[row], &[row]);
        // A report frame that claims more than is left.
        frames.extend([0x5C, 0, 200, 0, 1, 2, 3]);
        let (seen, result) = collect(&mut p, &dft_report(&frames));
        assert_eq!(seen.len(), 1);
        assert_eq!(result, Err(Error::ShortRead { wanted: 200, available: 3 }));

        // An empty report frame at the very end is an error, in the middle it is not.
        let mut frames = report_frame(0x62, &[]);
        frames.extend(dft(1, 1, window::POSITION, 5, &[row], &[row]));
        assert_eq!(collect(&mut p, &dft_report(&frames)).1, Ok(()));
        frames.extend(report_frame(0x62, &[]));
        assert_eq!(collect(&mut p, &dft_report(&frames)).1, Err(Error::EndOfData { wanted: 0 }));

        // Truncated reports.
        assert_eq!(collect(&mut p, &[0x0B, 0]).1, Err(Error::ShortRead { wanted: 3, available: 2 }));
        assert_eq!(collect(&mut p, &[0x0B, 0, 0]).1, Err(Error::EndOfData { wanted: 7 }));
        assert_eq!(
            collect(&mut p, &report(1, &[3, 0, 0, 0, 0, 0xFF, 0, 1])).1,
            Err(Error::ShortRead { wanted: usize::MAX - 3, available: 1 })
        );
    }

    #[test]
    fn unknown_frames_are_skipped_and_the_four_byte_tail_drops_the_frame() {
        let mut p = Parser::new();
        let row = Row::default();
        let mut inner = hid_frame(0x42, &[1, 2, 3]);
        inner.extend(hid_frame(HID_FRAME_REPORTS, &dft(1, 1, window::POSITION, 5, &[row], &[row])));
        assert_eq!(collect(&mut p, &report(1, &hid_frame(HID_FRAME_HID, &inner))).0.len(), 1);

        let mut tail = hid_frame(HID_FRAME_REPORTS, &dft(1, 1, window::POSITION, 5, &[row], &[row]));
        tail.extend([0x74, 0, 4, 0]);
        assert_eq!(collect(&mut p, &report(1, &hid_frame(HID_FRAME_HID, &tail))), (vec![], Ok(())));
    }

    #[test]
    fn metadata_feature() {
        let mut body = Vec::new();
        for v in [46u32, 68, 27389, 18259] {
            body.extend(v.to_le_bytes());
        }
        body.push(1);
        for v in [408.79f32, 0.0, 0.0, 0.0, -405.76, 0.0] {
            body.extend(v.to_le_bytes());
        }
        body.extend([0; 64]);
        let mut feature = vec![6];
        feature.extend(hid_frame(HID_FRAME_HID, &hid_frame(HID_FRAME_METADATA, &body)));
        let mut got = None;
        Parser::new()
            .parse_feature(&feature, &mut |e| {
                if let Event::Metadata(m) = e {
                    got = Some(m);
                }
            })
            .unwrap();
        assert_eq!(got, Some(Metadata { rows: 46, columns: 68, width: 27.389, height: 18.259, invert_x: false, invert_y: true }));
    }
}
