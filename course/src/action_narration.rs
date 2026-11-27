use super::{ActionNarrationClip, ActionNarrationPhaseClips, CourseError, UnitAsset};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionNarrationInput {
    #[serde(default)]
    pub(super) before: Option<NarrationPhaseInput>,
    #[serde(default)]
    pub(super) during: Option<NarrationPhaseInput>,
    #[serde(default)]
    pub(super) after: Option<NarrationPhaseInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NarrationPhaseInput {
    #[serde(default, deserialize_with = "deserialize_asset_id")]
    asset_id: Option<String>,
    clips: Vec<NarrationClipInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NarrationClipInput {
    #[serde(default)]
    offset_ms: Option<u64>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    caption: Option<String>,
}

#[derive(Clone)]
pub(super) struct ResolvedAudioAsset {
    path: PathBuf,
    relative_path: PathBuf,
    duration_ms: u64,
}

fn deserialize_asset_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(asset_id) => Ok(Some(asset_id)),
        _ => Err(serde::de::Error::custom(
            "Action narration asset_id must be a string; omit it for caption-only narration",
        )),
    }
}

pub(super) fn take_action_narrations(
    step: &mut serde_json::Value,
) -> Result<Vec<ActionNarrationInput>, CourseError> {
    let actions = step
        .get_mut("actions")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| CourseError::Package("Step actions must be an array".to_string()))?;
    actions
        .iter_mut()
        .map(|action| {
            let Some(object) = action.as_object_mut() else {
                return Err(CourseError::Package(
                    "Step Action must be a JSON object".to_string(),
                ));
            };
            object
                .remove("narration")
                .map(serde_json::from_value)
                .transpose()
                .map(Option::unwrap_or_default)
                .map_err(CourseError::from)
        })
        .collect()
}

pub(super) fn load_action_narration_phase(
    unit_audio_assets: &HashMap<String, ResolvedAudioAsset>,
    phase: &str,
    narration: NarrationPhaseInput,
) -> Result<ActionNarrationPhaseClips, CourseError> {
    if narration.clips.is_empty() {
        return Err(CourseError::Package(format!(
            "Action {phase} narration clips must not be empty"
        )));
    }
    let Some(asset_id) = narration.asset_id else {
        let clips = narration
            .clips
            .into_iter()
            .enumerate()
            .map(|(index, clip)| {
                if clip.offset_ms.is_some() {
                    return Err(CourseError::Package(format!(
                        "Action {phase} narration clip {index} offset_ms requires audio"
                    )));
                }
                let caption = clip.caption.filter(|caption| !caption.trim().is_empty());
                if caption.is_none() {
                    return Err(CourseError::Package(format!(
                        "Action {phase} narration clip {index} must declare a caption"
                    )));
                }
                // A caption-only clip normally declares its own positive
                // `duration_ms`. A `Wait` `during` caption-only clip omits it and
                // resolves to `0` here; `resolve_wait_caption_durations` backfills
                // the concrete Wait `duration_ms` after contract validation.
                let duration_ms = clip
                    .duration_ms
                    .filter(|duration| *duration > 0)
                    .unwrap_or(0);
                Ok(ActionNarrationClip {
                    audio_path: None,
                    audio_relative_path: None,
                    offset_ms: None,
                    duration_ms,
                    caption,
                })
            })
            .collect::<Result<Vec<_>, CourseError>>()?;
        return Ok(ActionNarrationPhaseClips { clips });
    };
    if asset_id.trim().is_empty() {
        return Err(CourseError::Package(format!(
            "Action {phase} narration asset_id must not be empty"
        )));
    }
    let asset = unit_audio_assets.get(&asset_id).ok_or_else(|| {
        CourseError::Package(format!(
            "Action {phase} narration references unknown asset_id `{asset_id}`"
        ))
    })?;
    let mut previous_end_ms = None;
    let clips = narration
        .clips
        .into_iter()
        .enumerate()
        .map(|(index, clip)| {
            let caption = clip
                .caption
                .filter(|caption| !caption.trim().is_empty())
                .ok_or_else(|| {
                    CourseError::Package(format!(
                        "Action {phase} narration clip {index} must declare a caption"
                    ))
                })?;
            if clip.offset_ms.is_none() || clip.duration_ms.is_none() {
                return Err(CourseError::Package(format!(
                    "Action {phase} narration clip {index} audio requires offset_ms and duration_ms"
                )));
            }
            let duration_ms = clip
                .duration_ms
                .filter(|duration| *duration > 0)
                .ok_or_else(|| {
                    CourseError::Package(format!(
                        "Action {phase} narration clip {index} requires a positive duration_ms"
                    ))
                })?;
            let offset_ms = clip.offset_ms.expect("audio clip offset checked");
            let end_ms = offset_ms.checked_add(duration_ms).ok_or_else(|| {
                CourseError::Package(format!(
                    "Action {phase} narration clip {index} range overflows"
                ))
            })?;
            if end_ms > asset.duration_ms {
                return Err(CourseError::Package(format!(
                    "Action {phase} narration clip {index} range {}..{} exceeds audio duration {} ms",
                    offset_ms,
                    end_ms,
                    asset.duration_ms
                )));
            }
            if previous_end_ms.is_some_and(|previous_end| offset_ms < previous_end) {
                return Err(CourseError::Package(format!(
                    "Action {phase} narration clip {index} range {}..{} overlaps the previous clip",
                    offset_ms, end_ms
                )));
            }
            previous_end_ms = Some(end_ms);
            Ok(ActionNarrationClip {
                audio_path: Some(asset.path.clone()),
                audio_relative_path: Some(asset.relative_path.clone()),
                offset_ms: Some(offset_ms),
                duration_ms,
                caption: Some(caption),
            })
        })
        .collect::<Result<Vec<_>, CourseError>>()?;
    Ok(ActionNarrationPhaseClips { clips })
}

pub(super) fn load_unit_audio_assets(
    root: &Path,
    canonical_root: &Path,
    unit_path: &str,
    assets: &[UnitAsset],
    cache: &mut HashMap<PathBuf, ResolvedAudioAsset>,
) -> Result<HashMap<String, ResolvedAudioAsset>, CourseError> {
    let mut resolved = HashMap::new();
    for asset in assets {
        if asset.id.trim().is_empty() {
            return Err(CourseError::Package(format!(
                "Unit `{unit_path}` contains an asset with an empty id"
            )));
        }
        if resolved.contains_key(&asset.id) {
            return Err(CourseError::Package(format!(
                "Unit `{unit_path}` contains duplicate asset id `{}`",
                asset.id
            )));
        }
        if asset.path.contains('\\') {
            return Err(CourseError::Package(format!(
                "Unit `{unit_path}` asset `{}` path `{}` must use forward slashes",
                asset.id, asset.path
            )));
        }
        if !asset.path.ends_with(".ogg") {
            return Err(CourseError::Package(format!(
                "Unit `{unit_path}` asset `{}` path `{}` must use the .ogg extension",
                asset.id, asset.path
            )));
        }
        let path = super::audio_asset::contained_asset(root, canonical_root, &asset.path).map_err(
            |error| {
                CourseError::Package(format!(
                    "Unit `{unit_path}` asset `{}` path `{}` is invalid: {error}",
                    asset.id, asset.path
                ))
            },
        )?;
        let resolved_asset = if let Some(cached) = cache.get(&path) {
            cached.clone()
        } else {
            let duration_ms = super::audio_asset::ogg_vorbis_duration_ms(
                &super::audio_asset::read_audio_asset(&path)?,
            )
            .map_err(|error| {
                CourseError::Package(format!(
                    "Unit `{unit_path}` asset `{}` path `{}` is not valid Ogg Vorbis: {error}",
                    asset.id, asset.path
                ))
            })?;
            let resolved_asset = ResolvedAudioAsset {
                path: path.clone(),
                relative_path: PathBuf::from(&asset.path),
                duration_ms,
            };
            cache.insert(path, resolved_asset.clone());
            resolved_asset
        };
        resolved.insert(asset.id.clone(), resolved_asset);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_audio_asset_is_read_and_parsed_once_per_load() {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/narration-demo/assets/intro.ogg");
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::copy(source, root.join("assets/intro.ogg")).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let asset = UnitAsset {
            id: "intro".to_string(),
            path: "assets/intro.ogg".to_string(),
        };
        let mut cache = HashMap::new();

        let first = load_unit_audio_assets(
            root,
            &canonical_root,
            "unit-a",
            std::slice::from_ref(&asset),
            &mut cache,
        )
        .unwrap();

        std::fs::write(root.join("assets/intro.ogg"), b"not ogg").unwrap();
        let second = load_unit_audio_assets(
            root,
            &canonical_root,
            "unit-b",
            std::slice::from_ref(&asset),
            &mut cache,
        )
        .unwrap();

        assert_eq!(first["intro"].duration_ms, second["intro"].duration_ms);
    }
}
