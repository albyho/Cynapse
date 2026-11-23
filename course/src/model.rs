use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RequirementOverride<T> {
    #[default]
    Inherit,
    Disabled,
    Values(Vec<T>),
}

impl<'de, T> Deserialize<'de> for RequirementOverride<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match Option::<Vec<T>>::deserialize(deserializer)? {
            None => Ok(Self::Disabled),
            Some(values) if values.is_empty() => Ok(Self::Disabled),
            Some(values) => Ok(Self::Values(values)),
        }
    }
}

impl<T: Clone> RequirementOverride<T> {
    pub(super) fn resolve(&self, inherited: &[T]) -> Vec<T> {
        match self {
            Self::Inherit => inherited.to_vec(),
            Self::Disabled => Vec::new(),
            Self::Values(values) => values.clone(),
        }
    }
}

impl<T> RequirementOverride<T> {
    /// True when the field is absent, i.e. the requirement is inherited.
    pub fn is_inherit(&self) -> bool {
        matches!(self, Self::Inherit)
    }
}

impl<T: Serialize> Serialize for RequirementOverride<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            // `null` is the canonical spelling of an explicitly disabled
            // requirement; an inherited one is omitted by the caller.
            Self::Inherit | Self::Disabled => serializer.serialize_none(),
            Self::Values(values) => values.serialize(serializer),
        }
    }
}

fn deserialize_optional_requirement_override<'de, D, T>(
    deserializer: D,
) -> Result<Option<RequirementOverride<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    RequirementOverride::<T>::deserialize(deserializer).map(Some)
}

pub(super) fn resolve_inherited<T: Clone>(
    value: Option<&RequirementOverride<T>>,
    inherited: &[T],
) -> Vec<T> {
    value
        .map(|value| value.resolve(inherited))
        .unwrap_or_else(|| inherited.to_vec())
}

pub(super) fn merge_assets(
    inherited: &[UnitAsset],
    local: Option<&Vec<UnitAsset>>,
) -> Result<Vec<UnitAsset>, CourseError> {
    let mut merged = inherited.to_vec();
    let Some(local) = local else {
        return Ok(merged);
    };
    let mut local_ids = HashSet::new();
    for asset in local {
        if !local_ids.insert(asset.id.as_str()) {
            return Err(CourseError::Package(format!(
                "duplicate asset id `{}` in one manifest node",
                asset.id
            )));
        }
        if let Some(existing) = merged.iter_mut().find(|entry| entry.id == asset.id) {
            *existing = asset.clone();
        } else {
            merged.push(asset.clone());
        }
    }
    Ok(merged)
}

#[derive(Debug, Clone, Default)]
pub(super) struct InheritedNodeDefaults {
    pub(super) adapters: Vec<AdapterId>,
    pub(super) environment: Vec<EnvironmentRequirement>,
    pub(super) assets: Vec<UnitAsset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestInput {
    id: String,
    title: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    adapters: RequirementOverride<AdapterId>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    assets: Vec<UnitAsset>,
    #[serde(default)]
    environment: RequirementOverride<EnvironmentRequirement>,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    children: Vec<CourseNodeDocument>,
}

fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone)]
pub struct NodeFields {
    pub id: String,
    pub title: String,
    pub adapters: RequirementOverride<AdapterId>,
    pub assets: Vec<UnitAsset>,
    pub environment: RequirementOverride<EnvironmentRequirement>,
    pub children: Vec<CourseNodeDocument>,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub(crate) node: NodeFields,
    pub version: String,
    pub workspace: Option<String>,
}

impl Deref for Manifest {
    type Target = NodeFields;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = ManifestInput::deserialize(deserializer)?;
        Ok(Self {
            node: NodeFields {
                id: input.id,
                title: input.title,
                adapters: input.adapters,
                assets: input.assets,
                environment: input.environment,
                children: input.children,
            },
            version: input.version,
            workspace: input.workspace,
        })
    }
}

impl Serialize for Manifest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Manifest", 8)?;
        state.serialize_field("id", &self.node.id)?;
        state.serialize_field("title", &self.node.title)?;
        if !self.version.is_empty() {
            state.serialize_field("version", &self.version)?;
        }
        if let Some(workspace) = &self.workspace {
            state.serialize_field("workspace", workspace)?;
        }
        if !self.node.adapters.is_inherit() {
            state.serialize_field("adapters", &self.node.adapters)?;
        }
        if !self.node.environment.is_inherit() {
            state.serialize_field("environment", &self.node.environment)?;
        }
        if !self.node.assets.is_empty() {
            state.serialize_field("assets", &self.node.assets)?;
        }
        state.serialize_field("children", &self.node.children)?;
        state.end()
    }
}

/// Canonical manifest text: struct field order, two-space indent, one trailing
/// newline. The caller owns line-ending preservation.
pub fn canonical_manifest_json(manifest: &Manifest) -> Result<String, CourseError> {
    let mut json = serde_json::to_string_pretty(manifest)?;
    json.push('\n');
    Ok(json)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRequirement {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub install_hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterId {
    Vscode,
    Jupyterlab,
}

impl AdapterId {
    pub fn connection_name(self) -> &'static str {
        match self {
            Self::Vscode => "vscode",
            Self::Jupyterlab => "jupyterlab",
        }
    }

    /// The name shown to learners when the required target software is not
    /// connected. Keep this independent from [`Self::connection_name`], which
    /// is the stable protocol identity.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Vscode => "VS Code",
            Self::Jupyterlab => "JupyterLab",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub unit_id: String,
    pub title: String,
    #[serde(default)]
    pub assets: Vec<UnitAsset>,
    pub adapters: Vec<AdapterId>,
    pub environment: Vec<EnvironmentRequirement>,
    pub steps: Vec<StepReference>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnitAsset {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StepReference {
    pub step_file: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CourseNodeDocument {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Optional on-disk directory segment. A missing value resolves to `id`,
    /// which keeps every package authored before `dir_name` at its existing
    /// layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dir_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_requirement_override"
    )]
    pub(crate) adapters: Option<RequirementOverride<AdapterId>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_requirement_override"
    )]
    pub(crate) environment: Option<RequirementOverride<EnvironmentRequirement>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) steps: Option<Vec<StepReference>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) assets: Option<Vec<UnitAsset>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) children: Option<Vec<CourseNodeDocument>>,
}

impl CourseNodeDocument {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn dir_name(&self) -> Option<&str> {
        self.dir_name.as_deref()
    }

    pub fn children(&self) -> &[CourseNodeDocument] {
        self.children.as_deref().unwrap_or_default()
    }

    pub fn steps(&self) -> &[StepReference] {
        self.steps.as_deref().unwrap_or_default()
    }

    pub fn assets(&self) -> &[UnitAsset] {
        self.assets.as_deref().unwrap_or_default()
    }

    pub fn adapters(&self) -> Option<&RequirementOverride<AdapterId>> {
        self.adapters.as_ref()
    }

    pub fn environment(&self) -> Option<&RequirementOverride<EnvironmentRequirement>> {
        self.environment.as_ref()
    }
}

/// A node in the course tree.
///
/// There is only one node type. `children` is what makes a node expandable,
/// `steps` is what makes it a learning unit; a node carries at least one of
/// the two and may carry both (a chapter that teaches something itself and
/// then hands over to its subsections).
#[derive(Debug, Clone)]
pub struct CourseNode {
    pub id: String,
    /// On-disk directory segment for this node. A node authored before
    /// `dir_name` existed resolves it to its `id`.
    pub dir_name: String,
    /// Root-relative slash-joined sequence of ancestor `dir_name`s plus this
    /// node's own. Used only for filesystem layout, never for identity.
    pub dir_path: String,
    pub title: String,
    pub adapters: Vec<AdapterId>,
    pub environment: Vec<EnvironmentRequirement>,
    pub assets: Vec<UnitAsset>,
    pub steps: Vec<StepReference>,
    pub children: Vec<CourseNode>,
}

impl CourseNode {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn dir_name(&self) -> &str {
        &self.dir_name
    }

    pub fn dir_path(&self) -> &str {
        &self.dir_path
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn has_steps(&self) -> bool {
        !self.steps.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct UnitEntry {
    pub path: String,
    /// Directory Path of the same node: the layout key for `seeds/` and for
    /// resource aliases. `path` stays the identity key.
    pub dir_path: String,
    pub parent_id: String,
    pub parent_title: String,
    pub unit: Unit,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub step_id: String,
    pub title: String,
    #[serde(default)]
    pub requires_human: bool,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub challenge: Option<Challenge>,
    #[serde(skip)]
    pub(super) action_narrations: Vec<ActionNarration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub instructions: String,
    #[serde(default)]
    pub checks: Vec<ChallengeCheck>,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default = "default_challenge_allow_skip")]
    pub allow_skip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ChallengeCheck {
    #[serde(rename = "COMMAND")]
    Command {
        session_key: String,
        #[serde(default = "default_challenge_cwd")]
        cwd: String,
        command: String,
        #[serde(default = "default_challenge_exit_codes")]
        accepted_exit_codes: Vec<i32>,
        #[serde(default)]
        expected_output: Vec<String>,
        #[serde(default = "default_challenge_timeout_ms")]
        timeout_ms: u64,
    },
}

fn default_challenge_allow_skip() -> bool {
    true
}

fn default_challenge_cwd() -> String {
    ".".to_string()
}

fn default_challenge_exit_codes() -> Vec<i32> {
    vec![0]
}

fn default_challenge_timeout_ms() -> u64 {
    300_000
}

impl ChallengeCheck {
    pub fn session_key(&self) -> &str {
        match self {
            Self::Command { session_key, .. } => session_key,
        }
    }

    pub fn cwd(&self) -> &str {
        match self {
            Self::Command { cwd, .. } => cwd,
        }
    }

    pub fn command(&self) -> &str {
        match self {
            Self::Command { command, .. } => command,
        }
    }

    pub fn accepted_exit_codes(&self) -> &[i32] {
        match self {
            Self::Command {
                accepted_exit_codes,
                ..
            } => accepted_exit_codes,
        }
    }

    pub fn expected_output(&self) -> &[String] {
        match self {
            Self::Command {
                expected_output, ..
            } => expected_output,
        }
    }

    pub fn timeout_ms(&self) -> u64 {
        match self {
            Self::Command { timeout_ms, .. } => *timeout_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedStep {
    pub parent_id: String,
    pub parent_title: String,
    pub unit_title: String,
    pub unit_path: String,
    pub step_file: PathBuf,
    pub step: Step,
}

#[derive(Debug, Clone)]
pub struct PlaybackStep {
    pub parent_id: String,
    pub parent_title: String,
    pub unit_title: String,
    pub unit_path: String,
    pub step_id: String,
    pub step_title: String,
    pub requires_human: bool,
    pub challenge: bool,
    pub actions: Vec<PlaybackAction>,
}

#[derive(Debug, Clone)]
pub struct PlaybackAction {
    pub action: Action,
    pub step_id: String,
    pub narration: ActionNarration,
}

impl PlaybackAction {
    /// Effective duration of a `Wait`: the longer of the authored `duration_ms`
    /// and its single `during` clip. `offset_ms` never extends it.
    pub fn effective_wait_duration_ms(&self) -> Option<u64> {
        let Action::Wait(wait) = &self.action else {
            return None;
        };
        let clip_duration_ms = self
            .narration
            .clips(ActionNarrationPhase::During)
            .and_then(|clips| clips.first())
            .map_or(0, |clip| clip.duration_ms);
        Some(wait.duration_ms.max(clip_duration_ms))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActionNarration {
    pub before: Option<ActionNarrationPhaseClips>,
    pub during: Option<ActionNarrationPhaseClips>,
    pub after: Option<ActionNarrationPhaseClips>,
}

impl ActionNarration {
    pub fn has_any(&self) -> bool {
        self.before.is_some() || self.during.is_some() || self.after.is_some()
    }

    pub fn clips(&self, phase: ActionNarrationPhase) -> Option<&[ActionNarrationClip]> {
        match phase {
            ActionNarrationPhase::Before => {
                self.before.as_ref().map(|phase| phase.clips.as_slice())
            }
            ActionNarrationPhase::During => {
                self.during.as_ref().map(|phase| phase.clips.as_slice())
            }
            ActionNarrationPhase::After => self.after.as_ref().map(|phase| phase.clips.as_slice()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActionNarrationPhaseClips {
    pub clips: Vec<ActionNarrationClip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionNarrationPhase {
    Before,
    During,
    After,
}

#[derive(Debug, Clone)]
pub struct ActionNarrationClip {
    pub audio_path: Option<PathBuf>,
    pub audio_relative_path: Option<PathBuf>,
    pub offset_ms: Option<u64>,
    pub duration_ms: u64,
    pub caption: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Course {
    pub manifest: Manifest,
    pub tree: CourseNode,
    pub units: Vec<UnitEntry>,
    pub steps: Vec<LoadedStep>,
    pub package_root: PathBuf,
}

#[derive(Debug)]
pub enum CourseError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Package(String),
}

impl std::fmt::Display for CourseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "course I/O error: {error}"),
            Self::Json(error) => write!(formatter, "course JSON error: {error}"),
            Self::Package(message) => write!(formatter, "course package error: {message}"),
        }
    }
}

impl std::error::Error for CourseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Package(_) => None,
        }
    }
}

impl From<std::io::Error> for CourseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for CourseError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
