//! Zip and tar extraction ported from `src/archive.ts` and
//! `src/download-source.ts`: strict central-directory parsing, CRC checks,
//! path-traversal rejection, and entry/size limits.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use flate2::read::DeflateDecoder;
use skills_store::error::{diag, SkillsError};

/// Extraction limits.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    /// Maximum total uncompressed bytes.
    pub max_extracted_bytes: usize,
    /// Maximum entry count.
    pub max_entries: usize,
}

fn ensure_range(
    buffer: &[u8],
    offset: usize,
    length: usize,
    label: &str,
) -> Result<(), SkillsError> {
    if offset.saturating_add(length) > buffer.len() {
        return Err(diag::archive_invalid(&format!(
            "Invalid zip archive: {label} is out of bounds"
        )));
    }
    Ok(())
}

fn read_u16(buffer: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buffer[offset], buffer[offset + 1]])
}

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ])
}

const ZIP_LOCAL_FILE_HEADER: u32 = 0x04034b50;
const ZIP_CENTRAL_DIRECTORY_HEADER: u32 = 0x02014b50;
const ZIP_END_OF_CENTRAL_DIRECTORY: u32 = 0x06054b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x06064b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR: u32 = 0x07064b50;
const ZIP_END_MIN_SIZE: usize = 22;
const ZIP_MAX_COMMENT_SIZE: usize = 0xffff;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(data);
    crc.sum()
}

fn find_end_of_central_directory(buffer: &[u8]) -> Option<usize> {
    let min_offset = buffer
        .len()
        .saturating_sub(ZIP_MAX_COMMENT_SIZE + ZIP_END_MIN_SIZE);
    let mut offset = buffer.len().checked_sub(ZIP_END_MIN_SIZE)?;
    loop {
        if read_u32(buffer, offset) == ZIP_END_OF_CENTRAL_DIRECTORY {
            let comment_length = read_u16(buffer, offset + 20) as usize;
            if offset + ZIP_END_MIN_SIZE + comment_length == buffer.len() {
                return Some(offset);
            }
        }
        if offset == min_offset {
            return None;
        }
        offset -= 1;
    }
}

fn read_u64(buffer: &[u8], offset: usize, label: &str) -> Result<u64, SkillsError> {
    ensure_range(buffer, offset, 8, label)?;
    Ok(u64::from_le_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
        buffer[offset + 4],
        buffer[offset + 5],
        buffer[offset + 6],
        buffer[offset + 7],
    ]))
}

fn normalize_archive_path(raw_path: &str) -> Option<String> {
    if raw_path.is_empty() || raw_path.contains('\0') {
        return None;
    }
    let path = raw_path.replace('\\', "/");
    if path.starts_with('/')
        || path
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() && path[1..].starts_with(':'))
    {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.contains(&"..") {
        return None;
    }
    let normalized: Vec<&str> = parts
        .into_iter()
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let joined = normalized.join("/");
    if joined.is_empty() && !path.ends_with('/') {
        return None;
    }
    if path.ends_with('/') && !joined.is_empty() {
        Some(format!("{joined}/"))
    } else {
        Some(joined)
    }
}

fn find_extra_field(
    buffer: &[u8],
    extra_offset: usize,
    extra_length: usize,
    target_id: u16,
) -> Result<Option<&[u8]>, SkillsError> {
    let extra_end = extra_offset + extra_length;
    let mut offset = extra_offset;
    while offset < extra_end {
        if offset + 4 > extra_end {
            return Err(diag::archive_invalid("Invalid zip extra field"));
        }
        let id = read_u16(buffer, offset);
        let size = read_u16(buffer, offset + 2) as usize;
        let data_offset = offset + 4;
        ensure_range(buffer, data_offset, size, "zip extra field")?;
        if data_offset + size > extra_end {
            return Err(diag::archive_invalid("Invalid zip extra field"));
        }
        if id == target_id {
            return Ok(Some(&buffer[data_offset..data_offset + size]));
        }
        offset = data_offset + size;
    }
    Ok(None)
}

fn decode_zip64_extra(
    buffer: &[u8],
    extra_offset: usize,
    extra_length: usize,
    values: (u32, u32, u32, u16),
) -> Result<(u64, u64, u64), SkillsError> {
    let (mut compressed_size, mut uncompressed_size, mut local_header_offset, mut disk_start) =
        values;
    let needs_zip64 = uncompressed_size == 0xffff_ffff
        || compressed_size == 0xffff_ffff
        || local_header_offset == 0xffff_ffff
        || disk_start == 0xffff;
    if !needs_zip64 {
        if disk_start != 0 {
            return Err(diag::archive_multi_disk_zip());
        }
        return Ok((
            u64::from(compressed_size),
            u64::from(uncompressed_size),
            u64::from(local_header_offset),
        ));
    }
    let Some(zip64_extra) = find_extra_field(buffer, extra_offset, extra_length, 0x0001)? else {
        return Err(diag::archive_invalid("Invalid zip64 extra field"));
    };
    let mut value_offset = 0usize;
    let mut read_u64_in_extra = |label: &str| -> Result<u64, SkillsError> {
        if value_offset + 8 > zip64_extra.len() {
            return Err(diag::archive_invalid(&format!(
                "Invalid zip64 extra field: missing {label}"
            )));
        }
        let value = u64::from_le_bytes(
            zip64_extra[value_offset..value_offset + 8]
                .try_into()
                .unwrap(),
        );
        value_offset += 8;
        Ok(value)
    };
    if uncompressed_size == 0xffff_ffff {
        uncompressed_size =
            u32::try_from(read_u64_in_extra("uncompressed size")?).unwrap_or(u32::MAX);
    }
    if compressed_size == 0xffff_ffff {
        compressed_size = u32::try_from(read_u64_in_extra("compressed size")?).unwrap_or(u32::MAX);
    }
    if local_header_offset == 0xffff_ffff {
        local_header_offset =
            u32::try_from(read_u64_in_extra("local header offset")?).unwrap_or(u32::MAX);
    }
    if disk_start == 0xffff {
        if value_offset + 4 > zip64_extra.len() {
            return Err(diag::archive_invalid(
                "Invalid zip64 extra field: missing disk start",
            ));
        }
        disk_start = u16::from_le_bytes([zip64_extra[value_offset], zip64_extra[value_offset + 1]]);
    }
    if disk_start != 0 {
        return Err(diag::archive_multi_disk_zip());
    }
    Ok((
        u64::from(compressed_size),
        u64::from(uncompressed_size),
        u64::from(local_header_offset),
    ))
}

/// Read a zip archive into `{ path: bytes }`, enforcing limits.
#[allow(clippy::too_many_lines)]
pub fn read_zip_archive(
    bytes: &[u8],
    limits: ArchiveLimits,
) -> Result<HashMap<String, Vec<u8>>, SkillsError> {
    let buffer = bytes;
    let Some(end_offset) = find_end_of_central_directory(buffer) else {
        return Err(diag::archive_invalid("Invalid zip archive"));
    };

    let disk_number = read_u16(buffer, end_offset + 4);
    let central_directory_disk = read_u16(buffer, end_offset + 6);
    let entries_on_disk = read_u16(buffer, end_offset + 8);
    let total_entries_raw = read_u16(buffer, end_offset + 10);
    let cd_size_raw = read_u32(buffer, end_offset + 12);
    let cd_offset_raw = read_u32(buffer, end_offset + 16);
    let uses_zip64 = entries_on_disk == 0xffff
        || total_entries_raw == 0xffff
        || cd_size_raw == 0xffff_ffff
        || cd_offset_raw == 0xffff_ffff;

    let (total_entries, cd_offset, cd_size, trailer_offset) = if !uses_zip64 {
        if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries_raw {
            return Err(diag::archive_multi_disk_zip());
        }
        (
            u64::from(total_entries_raw),
            u64::from(cd_offset_raw),
            u64::from(cd_size_raw),
            u64::try_from(end_offset).unwrap_or(u64::MAX),
        )
    } else {
        if disk_number != 0 || central_directory_disk != 0 {
            return Err(diag::archive_multi_disk_zip());
        }
        let locator_offset = end_offset - 20;
        ensure_range(buffer, locator_offset, 20, "zip64 locator")?;
        if read_u32(buffer, locator_offset) != ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR {
            return Err(diag::archive_invalid("Invalid zip64 locator"));
        }
        if read_u32(buffer, locator_offset + 4) != 0 || read_u32(buffer, locator_offset + 16) != 1 {
            return Err(diag::archive_multi_disk_zip());
        }
        let zip64_end_offset = read_u64(buffer, locator_offset + 8, "zip64 end offset")? as usize;
        ensure_range(
            buffer,
            zip64_end_offset,
            56,
            "zip64 end of central directory",
        )?;
        if read_u32(buffer, zip64_end_offset) != ZIP64_END_OF_CENTRAL_DIRECTORY {
            return Err(diag::archive_invalid(
                "Invalid zip64 end of central directory",
            ));
        }
        let record_size = read_u64(buffer, zip64_end_offset + 4, "zip64 end size")?;
        if record_size < 44 {
            return Err(diag::archive_invalid(
                "Invalid zip64 end of central directory",
            ));
        }
        if zip64_end_offset + record_size as usize + 12 != locator_offset {
            return Err(diag::archive_invalid(
                "Invalid zip64 end of central directory",
            ));
        }
        if read_u32(buffer, zip64_end_offset + 16) != 0
            || read_u32(buffer, zip64_end_offset + 20) != 0
        {
            return Err(diag::archive_multi_disk_zip());
        }
        let zip64_entries_on_disk =
            read_u64(buffer, zip64_end_offset + 24, "zip64 entries on disk")?;
        let zip64_total_entries = read_u64(buffer, zip64_end_offset + 32, "zip64 total entries")?;
        if zip64_entries_on_disk != zip64_total_entries {
            return Err(diag::archive_multi_disk_zip());
        }
        (
            zip64_total_entries,
            read_u64(
                buffer,
                zip64_end_offset + 48,
                "zip64 central directory offset",
            )?,
            read_u64(
                buffer,
                zip64_end_offset + 40,
                "zip64 central directory size",
            )?,
            u64::try_from(zip64_end_offset).unwrap_or(u64::MAX),
        )
    };

    if total_entries > limits.max_entries as u64 {
        return Err(diag::archive_too_many_files(
            total_entries as usize,
            limits.max_entries,
            None,
        ));
    }
    ensure_range(
        buffer,
        cd_offset as usize,
        cd_size as usize,
        "central directory",
    )?;
    if cd_offset + cd_size > trailer_offset {
        return Err(diag::archive_invalid(
            "Invalid zip archive: central directory overlaps archive trailer",
        ));
    }

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut extracted_bytes: usize = 0;
    let mut offset = cd_offset as usize;

    for _ in 0..total_entries {
        ensure_range(buffer, offset, 46, "central directory entry")?;
        if read_u32(buffer, offset) != ZIP_CENTRAL_DIRECTORY_HEADER {
            return Err(diag::archive_invalid("Invalid zip central directory entry"));
        }
        let flags = read_u16(buffer, offset + 8);
        let method = read_u16(buffer, offset + 10);
        let expected_checksum = read_u32(buffer, offset + 16);
        let compressed_size_raw = read_u32(buffer, offset + 20);
        let uncompressed_size_raw = read_u32(buffer, offset + 24);
        let file_name_length = read_u16(buffer, offset + 28) as usize;
        let extra_length = read_u16(buffer, offset + 30) as usize;
        let comment_length = read_u16(buffer, offset + 32) as usize;
        let disk_start_raw = read_u16(buffer, offset + 34);
        let external_attributes = read_u32(buffer, offset + 38);
        let local_header_offset_raw = read_u32(buffer, offset + 42);
        let variable_size = file_name_length + extra_length + comment_length;
        ensure_range(
            buffer,
            offset + 46,
            variable_size,
            "central directory entry data",
        )?;

        let name_start = offset + 46;
        let extra_offset = name_start + file_name_length;
        let (compressed_size, uncompressed_size, local_header_offset) = decode_zip64_extra(
            buffer,
            extra_offset,
            extra_length,
            (
                compressed_size_raw,
                uncompressed_size_raw,
                local_header_offset_raw,
                disk_start_raw,
            ),
        )?;

        let central_file_name = &buffer[name_start..name_start + file_name_length];
        let is_utf8 = flags & 0x800 != 0;
        let unicode_extra = find_extra_field(buffer, extra_offset, extra_length, 0x7075)?;
        let raw_file_name = decode_file_name(central_file_name, is_utf8, unicode_extra);
        let Some(file_name) = normalize_archive_path(&raw_file_name) else {
            return Err(diag::archive_unsafe_path(&raw_file_name));
        };
        if flags & 0x1 != 0 {
            return Err(diag::archive_encrypted_entry());
        }
        let file_type = (external_attributes >> 16) & 0o170000;
        if file_type != 0 && file_type != 0o100000 && file_type != 0o040000 {
            return Err(diag::archive_link_entry());
        }
        let is_directory = raw_file_name.replace('\\', "/").ends_with('/') || file_type == 0o040000;
        offset += 46 + variable_size;

        extracted_bytes += uncompressed_size as usize;
        if extracted_bytes > limits.max_extracted_bytes {
            return Err(diag::archive_too_large(limits.max_extracted_bytes, None));
        }
        if is_directory {
            continue;
        }

        let local_header_offset = local_header_offset as usize;
        ensure_range(buffer, local_header_offset, 30, "local file header")?;
        if read_u32(buffer, local_header_offset) != ZIP_LOCAL_FILE_HEADER {
            return Err(diag::archive_invalid("Invalid zip local file header"));
        }
        let local_flags = read_u16(buffer, local_header_offset + 6);
        let local_method = read_u16(buffer, local_header_offset + 8);
        let local_file_name_length = read_u16(buffer, local_header_offset + 26) as usize;
        let local_extra_length = read_u16(buffer, local_header_offset + 28) as usize;
        let local_name_offset = local_header_offset + 30;
        ensure_range(
            buffer,
            local_name_offset,
            local_file_name_length + local_extra_length,
            "local file header data",
        )?;
        let local_file_name =
            &buffer[local_name_offset..local_name_offset + local_file_name_length];
        if local_flags != flags || local_method != method || local_file_name != central_file_name {
            return Err(diag::archive_invalid(
                "Zip local header does not match central directory",
            ));
        }
        let data_offset = local_header_offset + 30 + local_file_name_length + local_extra_length;
        ensure_range(buffer, data_offset, compressed_size as usize, "file data")?;
        if data_offset + compressed_size as usize > cd_offset as usize {
            return Err(diag::archive_invalid(
                "Invalid zip archive: file data overlaps central directory",
            ));
        }
        let compressed = &buffer[data_offset..data_offset + compressed_size as usize];
        let contents: Vec<u8> = match method {
            0 => compressed.to_vec(),
            8 => inflate_capped(compressed, uncompressed_size as usize)?,
            other => return Err(diag::archive_unsupported_compression(other)),
        };
        if contents.len() != uncompressed_size as usize {
            return Err(diag::archive_invalid("Zip entry size mismatch"));
        }
        if crc32(&contents) != expected_checksum {
            return Err(diag::archive_invalid("Zip entry checksum mismatch"));
        }
        files.insert(file_name, contents);
    }

    if offset != cd_offset as usize + cd_size as usize {
        return Err(diag::archive_invalid("Invalid zip central directory size"));
    }
    Ok(files)
}

fn inflate_capped(compressed: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, SkillsError> {
    let decoder = DeflateDecoder::new(compressed);
    let mut contents = Vec::with_capacity(uncompressed_size);
    decoder
        .take(uncompressed_size as u64 + 1)
        .read_to_end(&mut contents)
        .map_err(|_| diag::archive_invalid("Invalid zip deflate stream"))?;
    Ok(contents)
}

fn decode_file_name(bytes: &[u8], is_utf8: bool, unicode_extra: Option<&[u8]>) -> String {
    if is_utf8 {
        return String::from_utf8_lossy(bytes).to_string();
    }
    if let Some(extra) = unicode_extra {
        if extra.len() >= 5
            && extra[0] == 1
            && u32::from_le_bytes(extra[1..5].try_into().unwrap()) == crc32(bytes)
        {
            return String::from_utf8_lossy(&extra[5..]).to_string();
        }
    }
    // CP437 high bytes: mirror the upstream mapping for the ASCII subset and
    // preserve the raw byte as Latin-1 otherwise (display-only path).
    bytes
        .iter()
        .map(|b| {
            if *b < 0x80 {
                *b as char
            } else {
                char::from_u32(u32::from(*b)).unwrap_or('?')
            }
        })
        .collect()
}

// ─── tar ────────────────────────────────────────────────────────────────────

fn validate_archive_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").trim_start_matches("./").to_string();
    if normalized.is_empty() || normalized.ends_with('/') {
        return Some(normalized);
    }
    if normalized.starts_with('/')
        || normalized.contains(':') && normalized.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    if normalized.split('/').any(|segment| segment == "..") {
        return None;
    }
    Some(normalized)
}

/// Extract a (possibly gzipped) tar archive into `extract_dir`, enforcing the
/// upstream validation rules.
pub fn extract_tar(
    archive_path: &Path,
    extract_dir: &Path,
    gzipped: bool,
    limits: ArchiveLimits,
) -> Result<(), SkillsError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
    let reader: Box<dyn Read> = if gzipped {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(false);
    archive.set_unpack_xattrs(false);

    let mut state_bytes: usize = 0;
    let mut state_entries: usize = 0;
    for entry in archive
        .entries()
        .map_err(|err| SkillsError::new("SKILLS_ARCHIVE_INVALID", err.to_string()))?
    {
        let mut entry =
            entry.map_err(|err| SkillsError::new("SKILLS_ARCHIVE_INVALID", err.to_string()))?;
        let raw_path = entry
            .path()
            .map_err(|err| SkillsError::new("SKILLS_ARCHIVE_INVALID", err.to_string()))?
            .to_string_lossy()
            .to_string();
        let Some(safe_path) = validate_archive_path(&raw_path) else {
            return Err(diag::archive_unsafe_path(&raw_path));
        };
        let target = extract_dir.join(&safe_path);
        let lexical_target = lexical_join(extract_dir, &safe_path);
        if !skills_store::installer::sanitize_name(&safe_path).is_empty()
            && !target.starts_with(extract_dir)
            && !lexical_target.starts_with(extract_dir)
        {
            return Err(diag::archive_unsafe_path(&raw_path));
        }

        state_entries += 1;
        if state_entries > limits.max_entries {
            return Err(diag::archive_too_many_files(
                state_entries,
                limits.max_entries,
                Some("SKILLS_EXTRACT_MAX_FILES"),
            ));
        }
        let size = entry.size() as usize;
        state_bytes += size;
        if state_bytes > limits.max_extracted_bytes {
            return Err(diag::archive_too_large(
                limits.max_extracted_bytes,
                Some("SKILLS_EXTRACT_MAX_BYTES"),
            ));
        }

        let is_regular = entry.header().entry_type().is_file();
        let is_directory = entry.header().entry_type().is_dir();
        if entry.header().entry_type().is_symlink() {
            return Err(diag::archive_link_entry());
        }
        if is_regular {
            if let Some(parent) = lexical_target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
            }
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
            std::fs::write(&lexical_target, contents)
                .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
        } else if is_directory {
            std::fs::create_dir_all(&lexical_target)
                .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
        }
    }
    Ok(())
}

fn lexical_join(base: &Path, relative: &str) -> std::path::PathBuf {
    let mut out = base.to_path_buf();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Read a tar.gz byte slice into `{ path: bytes }` with the well-known
/// validation rules (upstream `extractTarGz` in providers/wellknown.ts).
pub fn read_tar_gz(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, SkillsError> {
    const MAX_ARCHIVE_UNPACKED_BYTES: usize = 50 * 1024 * 1024;
    const MAX_ARCHIVE_FILES: usize = 1000;

    let mut gz = flate2::read::GzDecoder::new(bytes);
    let mut tar_bytes = Vec::new();
    gz.read_to_end(&mut tar_bytes)
        .map_err(|_| diag::archive_invalid("Invalid gzip stream"))?;

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut running_bytes: usize = 0;
    let mut offset = 0usize;
    while offset + 512 <= tar_bytes.len() {
        let header = &tar_bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = read_tar_string(header, 0, 100);
        let size_text = read_tar_string(header, 124, 12).trim().to_string();
        let type_flag = header[156];
        let prefix = read_tar_string(header, 345, 155);
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let size = usize::from_str_radix(size_text.trim_matches(' '), 8)
            .map_err(|_| diag::archive_invalid("Invalid tar entry size"))?;
        offset += 512;
        if type_flag == 0x32 || type_flag == 0x31 {
            return Err(diag::archive_link_entry());
        }
        let is_file = type_flag == 0 || type_flag == 0x30;
        if is_file {
            let end = (offset + size).min(tar_bytes.len());
            let content = tar_bytes[offset..end].to_vec();
            let Some(normalized) = normalize_wellknown_path(&path) else {
                return Err(diag::wellknown_archive_unsafe_path(&path));
            };
            running_bytes += content.len();
            if running_bytes > MAX_ARCHIVE_UNPACKED_BYTES {
                return Err(diag::wellknown_archive_too_large());
            }
            if files.len() >= MAX_ARCHIVE_FILES {
                return Err(diag::wellknown_archive_too_many_files());
            }
            files.insert(normalized, content);
        }
        offset += size.div_ceil(512) * 512;
    }
    if !files.contains_key("SKILL.md") {
        return Err(diag::archive_missing_skill_md());
    }
    Ok(files)
}

fn normalize_wellknown_path(raw_path: &str) -> Option<String> {
    if raw_path.is_empty()
        || raw_path.contains('\0')
        || raw_path.starts_with('/')
        || raw_path.starts_with('\\')
        || raw_path.contains('\\')
    {
        return None;
    }
    if raw_path.len() >= 2 && raw_path.as_bytes()[1] == b':' {
        return None;
    }
    let parts: Vec<&str> = raw_path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| *p == "." || *p == "..") {
        return None;
    }
    Some(parts.join("/"))
}

fn read_tar_string(buffer: &[u8], offset: usize, length: usize) -> String {
    let end = (offset + length).min(buffer.len());
    let slice = &buffer[offset..end];
    let nul = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..nul]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_zip_paths() {
        assert!(normalize_archive_path("../evil").is_none());
        assert!(normalize_archive_path("/abs").is_none());
        assert!(normalize_archive_path("C:/x").is_none());
        assert_eq!(normalize_archive_path("./a/b").as_deref(), Some("a/b"));
    }

    #[test]
    fn rejects_unsafe_tar_paths() {
        assert!(normalize_wellknown_path("../evil").is_none());
        assert!(normalize_wellknown_path("a\\b").is_none());
        assert_eq!(
            normalize_wellknown_path("skills/x/SKILL.md").as_deref(),
            Some("skills/x/SKILL.md")
        );
    }
}
