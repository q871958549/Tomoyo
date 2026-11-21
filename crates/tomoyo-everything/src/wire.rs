//! Pure-Rust binary wire encoding & decoding for Voidtools Everything `QUERY2W` / `LIST2`.

use crate::{EverythingEntry, EverythingError, EverythingQueryOptions, EverythingResponse};

pub const EVERYTHING_IPC_COPYDATA_QUERY2W: usize = 18;

pub const EVERYTHING_IPC_MATCHCASE: u32 = 0x0000_0001;
pub const EVERYTHING_IPC_MATCHWHOLEWORD: u32 = 0x0000_0002;
pub const EVERYTHING_IPC_MATCHPATH: u32 = 0x0000_0004;
pub const EVERYTHING_IPC_REGEX: u32 = 0x0000_0008;

pub const EVERYTHING_IPC_QUERY2_REQUEST_NAME: u32 = 0x0000_0001;
pub const EVERYTHING_IPC_QUERY2_REQUEST_PATH: u32 = 0x0000_0002;
pub const EVERYTHING_IPC_QUERY2_REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
pub const EVERYTHING_IPC_QUERY2_REQUEST_EXTENSION: u32 = 0x0000_0008;
pub const EVERYTHING_IPC_QUERY2_REQUEST_SIZE: u32 = 0x0000_0010;
pub const EVERYTHING_IPC_QUERY2_REQUEST_DATE_CREATED: u32 = 0x0000_0020;
pub const EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
pub const EVERYTHING_IPC_QUERY2_REQUEST_DATE_ACCESSED: u32 = 0x0000_0080;
pub const EVERYTHING_IPC_QUERY2_REQUEST_ATTRIBUTES: u32 = 0x0000_0100;

pub const EVERYTHING_IPC_FOLDER: u32 = 0x0000_0001;
pub const EVERYTHING_IPC_DRIVE: u32 = 0x0000_0002;

/// Packs an `EVERYTHING_IPC_QUERY2` header (28 bytes) + NUL-terminated UTF-16LE query string.
pub fn pack_query2w_payload(
    reply_hwnd_u32: u32,
    reply_id: u32,
    search: &str,
    opts: &EverythingQueryOptions,
) -> Vec<u8> {
    let mut search_flags = 0u32;
    if opts.match_case {
        search_flags |= EVERYTHING_IPC_MATCHCASE;
    }
    if opts.match_whole_word {
        search_flags |= EVERYTHING_IPC_MATCHWHOLEWORD;
    }
    if opts.match_path {
        search_flags |= EVERYTHING_IPC_MATCHPATH;
    }
    if opts.regex {
        search_flags |= EVERYTHING_IPC_REGEX;
    }

    let request_flags = EVERYTHING_IPC_QUERY2_REQUEST_NAME
        | EVERYTHING_IPC_QUERY2_REQUEST_PATH
        | EVERYTHING_IPC_QUERY2_REQUEST_SIZE
        | EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED;

    let wide: Vec<u16> = search.encode_utf16().chain(std::iter::once(0)).collect();
    let mut payload = Vec::with_capacity(28 + wide.len() * 2);
    payload.extend_from_slice(&reply_hwnd_u32.to_le_bytes());
    payload.extend_from_slice(&reply_id.to_le_bytes());
    payload.extend_from_slice(&search_flags.to_le_bytes());
    payload.extend_from_slice(&opts.offset.to_le_bytes());
    payload.extend_from_slice(&opts.max_results.to_le_bytes());
    payload.extend_from_slice(&request_flags.to_le_bytes());
    payload.extend_from_slice(&(opts.sort as u32).to_le_bytes());
    for u in wide {
        payload.extend_from_slice(&u.to_le_bytes());
    }
    payload
}

/// Parses a raw `EVERYTHING_IPC_LIST2` reply buffer (20-byte header + 8-byte item headers).
pub fn parse_list2_buffer(buf: &[u8]) -> Result<EverythingResponse, EverythingError> {
    if buf.len() < 20 {
        return Err(EverythingError::Ipc(format!(
            "reply buffer too short: {} bytes",
            buf.len()
        )));
    }

    let read_u32 = |offset: usize| -> Result<u32, EverythingError> {
        let end = offset
            .checked_add(4)
            .ok_or_else(|| EverythingError::Ipc("offset overflow".into()))?;
        let slice = buf
            .get(offset..end)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading u32".into()))?;
        Ok(u32::from_le_bytes(slice.try_into().unwrap()))
    };

    let read_u64 = |offset: usize| -> Result<u64, EverythingError> {
        let end = offset
            .checked_add(8)
            .ok_or_else(|| EverythingError::Ipc("offset overflow".into()))?;
        let slice = buf
            .get(offset..end)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading u64".into()))?;
        Ok(u64::from_le_bytes(slice.try_into().unwrap()))
    };

    let read_len_prefixed_utf16 = |cursor: &mut usize| -> Result<String, EverythingError> {
        let len_chars = read_u32(*cursor)? as usize;
        *cursor += 4;
        let byte_len = len_chars
            .checked_mul(2)
            .ok_or_else(|| EverythingError::Ipc("utf16 len overflow".into()))?;
        let bytes = buf
            .get(*cursor..*cursor + byte_len)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading utf16 string".into()))?;
        let mut units = Vec::with_capacity(len_chars);
        for chunk in bytes.chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        *cursor = cursor
            .checked_add(byte_len)
            .and_then(|v| v.checked_add(2))
            .ok_or_else(|| EverythingError::Ipc("cursor overflow".into()))?;
        Ok(String::from_utf16_lossy(&units))
    };

    // Real Voidtools EVERYTHING_IPC_LIST2 header is 5 DWORDs (20 bytes):
    // totitems, numitems, offset, request_flags, sort_type; items[] start at offset 20.
    let total_items = read_u32(0)?;
    let num_items = read_u32(4)?;
    let _offset = read_u32(8)?;
    let request_flags = read_u32(12)?;
    let _sort_type = read_u32(16)?;

    let safe_count = (num_items as usize).min(1000);
    let mut entries = Vec::with_capacity(safe_count);

    for i in 0..safe_count {
        let item_header_offset = 20 + i * 8;
        if item_header_offset + 8 > buf.len() {
            break;
        }
        let flags = read_u32(item_header_offset)?;
        let data_offset = read_u32(item_header_offset + 4)? as usize;
        let mut cursor = data_offset;

        let mut name = String::new();
        let mut path = String::new();
        let mut full_path = String::new();
        let mut size = None;
        let mut date_modified_filetime = None;
        let mut attributes = None;

        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_NAME) != 0 {
            name = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_PATH) != 0 {
            path = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_FULL_PATH_AND_NAME) != 0 {
            full_path = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_EXTENSION) != 0 {
            let _ = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_SIZE) != 0 {
            let v = read_u64(cursor)?;
            cursor += 8;
            if v != u64::MAX {
                size = Some(v);
            }
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_CREATED) != 0 {
            cursor += 8;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED) != 0 {
            let v = read_u64(cursor)?;
            cursor += 8;
            if v != u64::MAX && v != 0 {
                date_modified_filetime = Some(v);
            }
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_ACCESSED) != 0 {
            cursor += 8;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_ATTRIBUTES) != 0 {
            attributes = Some(read_u32(cursor)?);
        }

        if full_path.is_empty() {
            full_path = if path.is_empty() {
                name.clone()
            } else if path.ends_with('\\') {
                format!("{path}{name}")
            } else {
                format!("{path}\\{name}")
            };
        }
        if name.is_empty() {
            name = full_path
                .rsplit('\\')
                .next()
                .unwrap_or(&full_path)
                .to_string();
        }
        if path.is_empty()
            && let Some((parent, _)) = full_path.rsplit_once('\\') {
                path = parent.to_string();
            }

        let is_drive = (flags & EVERYTHING_IPC_DRIVE) != 0;
        let is_folder = (flags & EVERYTHING_IPC_FOLDER) != 0 || is_drive;

        entries.push(EverythingEntry {
            name,
            path,
            full_path,
            is_folder,
            is_drive,
            size: if is_folder { None } else { size },
            date_modified_filetime,
            attributes,
        });
    }

    let total_folders = entries.iter().filter(|e| e.is_folder).count() as u32;
    let total_files = entries.len() as u32 - total_folders;

    Ok(EverythingResponse {
        total_folders,
        total_files,
        total_items,
        entries,
    })
}
