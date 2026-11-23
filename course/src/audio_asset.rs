use super::CourseError;
use std::path::{Path, PathBuf};

/// Merged narration can be long, but reading an asset must never be unbounded.
const MAX_AUDIO_ASSET_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn read_audio_asset(path: &Path) -> Result<Vec<u8>, CourseError> {
    let length = std::fs::metadata(path)?.len();
    if length > MAX_AUDIO_ASSET_BYTES {
        return Err(CourseError::Package(format!(
            "narration audio `{}` is too large ({length} bytes)",
            path.display()
        )));
    }
    Ok(std::fs::read(path)?)
}

pub(super) fn contained_asset(
    package_root: &Path,
    package_canonical: &Path,
    relative: &str,
) -> Result<PathBuf, CourseError> {
    let relative_path = Path::new(relative);
    if relative.contains('\\')
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CourseError::Package(format!(
            "narration asset `{relative}` is outside the Course Package"
        )));
    }
    let path = package_root.join(relative_path);
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(package_canonical) || !canonical.is_file() {
        return Err(CourseError::Package(format!(
            "narration asset `{relative}` is outside the Course Package"
        )));
    }
    Ok(canonical)
}

pub(super) fn ogg_vorbis_duration_ms(bytes: &[u8]) -> Result<u64, CourseError> {
    let mut offset = 0;
    let mut sample_rate = None;
    let mut last_granule = 0_u64;
    let mut saw_identification = false;

    while offset < bytes.len() {
        if bytes.get(offset..offset + 4) != Some(b"OggS") {
            return Err(CourseError::Package(
                "narration audio is not a valid Ogg stream".to_string(),
            ));
        }
        let header = bytes.get(offset..offset + 27).ok_or_else(|| {
            CourseError::Package("narration audio has a truncated Ogg page".to_string())
        })?;
        let granule = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if granule != u64::MAX {
            last_granule = granule;
        }
        let segment_count = header[26] as usize;
        let table_end = offset + 27 + segment_count;
        let payload_len = bytes
            .get(offset + 27..table_end)
            .ok_or_else(|| {
                CourseError::Package("narration audio has a truncated Ogg table".to_string())
            })?
            .iter()
            .map(|value| *value as usize)
            .sum::<usize>();
        let payload_end = table_end + payload_len;
        let payload = bytes.get(table_end..payload_end).ok_or_else(|| {
            CourseError::Package("narration audio has a truncated Ogg payload".to_string())
        })?;
        if !saw_identification && payload.starts_with(b"\x01vorbis") {
            if payload.len() < 16 {
                return Err(CourseError::Package(
                    "narration audio has an invalid Vorbis header".to_string(),
                ));
            }
            sample_rate = Some(u32::from_le_bytes(payload[12..16].try_into().unwrap()));
            saw_identification = true;
        }
        offset = payload_end;
    }

    let sample_rate = sample_rate
        .ok_or_else(|| CourseError::Package("narration audio must be Ogg Vorbis".to_string()))?;
    if sample_rate == 0 || last_granule == 0 {
        return Err(CourseError::Package(
            "narration audio has no valid duration".to_string(),
        ));
    }
    Ok(last_granule.saturating_mul(1_000) / u64::from(sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_non_ogg_stream() {
        let error = ogg_vorbis_duration_ms(b"RIFF....").unwrap_err();
        assert!(error.to_string().contains("not a valid Ogg stream"));
    }

    #[test]
    fn rejects_a_truncated_page_header() {
        let error = ogg_vorbis_duration_ms(b"OggS").unwrap_err();
        assert!(error.to_string().contains("truncated Ogg page"));
    }

    #[test]
    fn rejects_a_truncated_segment_table() {
        let mut page = vec![0_u8; 27];
        page[0..4].copy_from_slice(b"OggS");
        page[26] = 2;
        let error = ogg_vorbis_duration_ms(&page).unwrap_err();
        assert!(error.to_string().contains("truncated Ogg table"));
    }

    #[test]
    fn rejects_a_short_vorbis_identification_header() {
        let mut page = vec![0_u8; 27];
        page[0..4].copy_from_slice(b"OggS");
        page[26] = 1;
        page.push(7);
        page.extend_from_slice(b"\x01vorbis");
        let error = ogg_vorbis_duration_ms(&page).unwrap_err();
        assert!(error.to_string().contains("invalid Vorbis header"));
    }
}
