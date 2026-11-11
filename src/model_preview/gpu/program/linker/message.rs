//! Length-delimited compiler messages. The limit is an IPC budget, not a native asset limit.
use eframe::glow;
use std::io::{self, Read, Write};

const MAX_BYTES: usize = 256 * 1024 * 1024;

fn check_length(length: usize) -> io::Result<()> {
    if length > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The preview compiler message exceeds 256 MiB",
        ));
    }
    Ok(())
}

fn read_bytes(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    check_length(length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(io::Error::other)?;
    bytes.resize(length, 0);
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_bytes(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    check_length(bytes.len())?;
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)
}

pub(super) fn read_text(reader: &mut impl Read) -> io::Result<String> {
    String::from_utf8(read_bytes(reader)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(super) fn write_text(writer: &mut impl Write, text: &str) -> io::Result<()> {
    write_bytes(writer, text.as_bytes())
}

pub(super) fn read_result(
    reader: &mut impl Read,
) -> io::Result<Result<glow::ProgramBinary, String>> {
    let mut tag = [0];
    reader.read_exact(&mut tag)?;
    match tag[0] {
        0 => Ok(Err(read_text(reader)?)),
        1 => {
            let mut format = [0; 4];
            reader.read_exact(&mut format)?;
            let buffer = read_bytes(reader)?;
            if buffer.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The preview compiler returned an empty program",
                ));
            }
            Ok(Ok(glow::ProgramBinary {
                format: u32::from_le_bytes(format),
                buffer,
            }))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The preview compiler returned an unknown response",
        )),
    }
}

pub(super) fn write_result(
    writer: &mut impl Write,
    result: &Result<glow::ProgramBinary, String>,
) -> io::Result<()> {
    match result {
        Ok(binary) => {
            writer.write_all(&[1])?;
            writer.write_all(&binary.format.to_le_bytes())?;
            write_bytes(writer, &binary.buffer)
        }
        Err(error) => {
            writer.write_all(&[0])?;
            write_text(writer, error)
        }
    }
}
