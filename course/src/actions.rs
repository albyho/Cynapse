use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    Present,
    Wait(WaitAction),
    OpenFile(OpenFileAction),
    OpenPreview(OpenPreviewAction),
    OpenFileWithPreview(OpenFileWithPreviewAction),
    CloseFile(CloseFileAction),
    ClosePreview(ClosePreviewAction),
    RevealText(RevealTextAction),
    ClearReveal,
    SetCursor(SetCursorAction),
    InsertText(InsertTextAction),
    DeleteText(DeleteTextAction),
    ReplaceText(ReplaceTextAction),
    CreateDirectory(CreateDirectoryAction),
    CreateFile(CreateFileAction),
    DeleteFile(DeleteFileAction),
    DeleteDirectory(DeleteDirectoryAction),
    TerminalEnsure(TerminalEnsureAction),
    TerminalCommand(TerminalCommandAction),
    JupyterLabCloseDocument(JupyterLabCloseDocumentAction),
    JupyterLabInterruptKernel(JupyterLabInterruptKernelAction),
    JupyterLabOpenNotebook(JupyterLabOpenNotebookAction),
    JupyterLabOpenPreview(JupyterLabOpenPreviewAction),
    JupyterLabRestartKernel(JupyterLabRestartKernelAction),
    JupyterLabCellDelete(JupyterLabCellDeleteAction),
    JupyterLabCellDeleteText(JupyterLabCellDeleteTextAction),
    JupyterLabCellDuplicate(JupyterLabCellDuplicateAction),
    JupyterLabCellInsert(JupyterLabCellInsertAction),
    JupyterLabCellInsertText(JupyterLabCellInsertTextAction),
    JupyterLabCellMove(JupyterLabCellMoveAction),
    JupyterLabCellReplaceText(JupyterLabCellReplaceTextAction),
    JupyterLabCellRun(JupyterLabCellRunAction),
    JupyterLabCellSelect(JupyterLabCellSelectAction),
}

impl Action {
    pub fn is_client_local(&self) -> bool {
        matches!(self, Self::Present | Self::Wait(_))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WaitAction {
    pub duration_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitActionInput {
    pub duration_ms: u64,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for WaitAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = WaitActionInput::deserialize(deserializer)?;
        if input.duration_ms == 0 {
            return Err(serde::de::Error::custom(
                "Wait duration_ms must be greater than zero",
            ));
        }
        Ok(Self {
            duration_ms: input.duration_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewView {
    Side,
    Current,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenPreviewAction {
    pub path: String,
    pub view: PreviewView,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenPreviewActionInput {
    pub path: String,
    #[serde(default)]
    view: Option<PreviewView>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for OpenPreviewAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = OpenPreviewActionInput::deserialize(deserializer)?;
        if input.path.is_empty() {
            return Err(serde::de::Error::custom(
                "OpenPreview path must not be empty",
            ));
        }
        Ok(Self {
            path: input.path,
            view: input.view.unwrap_or(PreviewView::Side),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenFileAction {
    pub path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenFileActionInput {
    pub path: String,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for OpenFileAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = OpenFileActionInput::deserialize(deserializer)?;
        Ok(Self { path: input.path })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenFileWithPreviewAction {
    pub path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenFileWithPreviewActionInput {
    pub path: String,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for OpenFileWithPreviewAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = OpenFileWithPreviewActionInput::deserialize(deserializer)?;
        if input.path.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "OpenFileWithPreview path must not be empty",
            ));
        }
        Ok(Self { path: input.path })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CloseFileAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClosePreviewAction {
    pub path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseActionInput {
    pub path: String,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

macro_rules! close_action {
    ($action:ty, $name:literal) => {
        impl<'de> Deserialize<'de> for $action {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let input = CloseActionInput::deserialize(deserializer)?;
                if input.path.trim().is_empty() {
                    return Err(serde::de::Error::custom(concat!(
                        $name,
                        " path must not be empty"
                    )));
                }
                Ok(Self { path: input.path })
            }
        }
    };
}

close_action!(CloseFileAction, "CloseFile");
close_action!(ClosePreviewAction, "ClosePreview");

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RevealTextAction {
    pub path: String,
    pub start: RevealPosition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<RevealPosition>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RevealPosition {
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealTextActionInput {
    pub path: String,
    pub start: RevealPosition,
    #[serde(default)]
    pub end: Option<RevealPosition>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for RevealTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = RevealTextActionInput::deserialize(deserializer)?;
        if input.path.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "RevealText path must not be empty",
            ));
        }
        if input.start.line == 0 {
            return Err(serde::de::Error::custom(
                "RevealText start.line must be at least 1",
            ));
        }
        if let Some(end) = input.end {
            if end.line == 0 {
                return Err(serde::de::Error::custom(
                    "RevealText end.line must be at least 1",
                ));
            }
            if end.line < input.start.line {
                return Err(serde::de::Error::custom(
                    "RevealText end must not precede start",
                ));
            }
            if end.line == input.start.line
                && let (Some(start_column), Some(end_column)) = (input.start.column, end.column)
                && end_column < start_column
            {
                return Err(serde::de::Error::custom(
                    "RevealText end.column must not precede start.column on the same line",
                ));
            }
        }
        Ok(Self {
            path: input.path,
            start: input.start,
            end: input.end,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InsertTextAction {
    pub path: String,
    pub target: InsertTextTarget,
    pub text: String,
    #[serde(skip_serializing)]
    pub text_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InsertTextActionInput {
    pub path: String,
    pub target: InsertTextTarget,
    pub text: Option<String>,
    pub text_path: Option<String>,
    pub speed_ms: Option<u32>,
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for InsertTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = InsertTextActionInput::deserialize(deserializer)?;
        let (text, text_path) = match (input.text, input.text_path) {
            (Some(text), None) if !text.is_empty() => (text, None),
            (Some(_), None) => {
                return Err(serde::de::Error::custom(
                    "InsertText text must not be empty",
                ));
            }
            (None, Some(text_path)) if !text_path.trim().is_empty() => {
                (String::new(), Some(text_path))
            }
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "InsertText must define exactly one of `text` or `text_path`",
                ));
            }
            (None, _) => {
                return Err(serde::de::Error::custom(
                    "InsertText must define exactly one of `text` or `text_path`",
                ));
            }
        };
        if input.target_hold_ms == Some(0) {
            return Err(serde::de::Error::custom(
                "InsertText target_hold_ms must be greater than zero",
            ));
        }
        Ok(Self {
            path: input.path,
            target: input.target,
            text,
            text_path,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum AnchorAt {
    Column(u32),
    Text(TextAnchor),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextAnchor {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

impl<'de> Deserialize<'de> for AnchorAt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(number) => {
                let column = number
                    .as_u64()
                    .and_then(|column| u32::try_from(column).ok())
                    .ok_or_else(|| {
                        serde::de::Error::custom(
                            "Anchor `at` column must be a non-negative integer",
                        )
                    })?;
                Ok(Self::Column(column))
            }
            serde_json::Value::Object(object) => {
                if let Some(field) = object
                    .keys()
                    .find(|key| !["text", "index"].contains(&key.as_str()))
                {
                    return Err(serde::de::Error::custom(format!(
                        "unknown text Anchor field `{field}`"
                    )));
                }
                let text = object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        serde::de::Error::custom("text Anchor requires a string `text`")
                    })?
                    .to_string();
                let index = match object.get("index") {
                    None => None,
                    Some(serde_json::Value::Null) => {
                        return Err(serde::de::Error::custom(
                            "text Anchor `index` must not be null",
                        ));
                    }
                    Some(value) => Some(serde_json::from_value::<u32>(value.clone()).map_err(
                        |error| {
                            serde::de::Error::custom(format!(
                                "text Anchor `index` is invalid: {error}"
                            ))
                        },
                    )?),
                };
                Ok(Self::Text(TextAnchor { text, index }))
            }
            _ => Err(serde::de::Error::custom(
                "Anchor `at` must be a column number or a text object",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub line: Option<u32>,
    pub at: Option<AnchorAt>,
    pub placement: Option<Placement>,
    pub eof: bool,
}

impl Anchor {
    pub fn validate(&self) -> Result<(), String> {
        if self.eof {
            if self.line.is_some() || self.at.is_some() || self.placement.is_some() {
                return Err("eof Anchor cannot include other fields".into());
            }
            return Ok(());
        }
        if self.line == Some(0) {
            return Err("Anchor line must be at least 1".into());
        }
        match &self.at {
            Some(AnchorAt::Column(_)) if self.line.is_none() => {
                Err("column Anchor requires `line`".into())
            }
            Some(AnchorAt::Text(text)) if text.text.is_empty() => {
                Err("Anchor text must not be empty".into())
            }
            Some(_) => Ok(()),
            None if self.line.is_none() => Err("Anchor must include `line`, `at`, or `eof`".into()),
            None => Ok(()),
        }
    }

    pub(crate) fn placement_or_default(&self, end: bool) -> Placement {
        self.placement.unwrap_or(match &self.at {
            Some(AnchorAt::Column(_)) => Placement::Before,
            _ if end => Placement::After,
            _ => Placement::Before,
        })
    }
}

impl Serialize for Anchor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        if let Some(line) = self.line {
            map.serialize_entry("line", &line)?;
        }
        if let Some(at) = &self.at {
            map.serialize_entry("at", at)?;
        }
        if let Some(placement) = self.placement {
            map.serialize_entry("placement", &placement)?;
        }
        if self.eof {
            map.serialize_entry("eof", &true)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Anchor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("Anchor must be an object"))?;
        if let Some(field) = object
            .keys()
            .find(|key| !["line", "at", "placement", "eof"].contains(&key.as_str()))
        {
            return Err(serde::de::Error::custom(format!(
                "unknown Anchor field `{field}`"
            )));
        }
        let anchor = Self {
            line: take_anchor_field::<u32>(object, "line").map_err(serde::de::Error::custom)?,
            at: take_anchor_field::<AnchorAt>(object, "at").map_err(serde::de::Error::custom)?,
            placement: take_anchor_field::<Placement>(object, "placement")
                .map_err(serde::de::Error::custom)?,
            eof: take_anchor_field::<bool>(object, "eof")
                .map_err(serde::de::Error::custom)?
                .unwrap_or(false),
        };
        anchor.validate().map_err(serde::de::Error::custom)?;
        Ok(anchor)
    }
}

pub type InsertTextTarget = Anchor;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SetCursorActionInput")]
pub struct SetCursorAction {
    pub path: String,
    pub target: Anchor,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCursorActionInput {
    path: String,
    target: Anchor,
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl TryFrom<SetCursorActionInput> for SetCursorAction {
    type Error = &'static str;

    fn try_from(input: SetCursorActionInput) -> Result<Self, Self::Error> {
        if input.path.trim().is_empty() {
            return Err("SetCursor path must not be empty");
        }
        Ok(Self {
            path: input.path,
            target: input.target,
        })
    }
}

fn take_anchor_field<T>(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<Option<T>, String>
where
    T: serde::de::DeserializeOwned,
{
    match object.get(name) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Err(format!("Anchor `{name}` must not be null")),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("Anchor `{name}` is invalid: {error}")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EditRange {
    pub start: Anchor,
    pub end: Anchor,
    pub expected_text: String,
}

fn parse_anchor(value: serde_json::Value, role: &str) -> Result<Anchor, String> {
    serde_json::from_value(value).map_err(|error| format!("{role} anchor: {error}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteTextAction {
    pub path: String,
    pub target: DeleteTextTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<DeleteDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteTextActionInput {
    pub path: String,
    pub target: DeleteTextTarget,
    pub direction: Option<DeleteDirection>,
    pub speed_ms: Option<u32>,
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for DeleteTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = DeleteTextActionInput::deserialize(deserializer)?;
        if matches!(input.target, DeleteTextTarget::Cursor(_)) && input.direction.is_none() {
            return Err(serde::de::Error::custom(
                "DeleteText cursor target requires `direction`",
            ));
        }
        if input.target_hold_ms == Some(0) {
            return Err(serde::de::Error::custom(
                "DeleteText target_hold_ms must be greater than zero",
            ));
        }
        Ok(Self {
            path: input.path,
            target: input.target,
            direction: input.direction,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DeleteTextTarget {
    Range(EditRange),
    Cursor(CursorDeletionTarget),
}

impl<'de> Deserialize<'de> for DeleteTextTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("DeleteText target must be an object"))?;
        if let Some(field) = object
            .keys()
            .find(|key| !["start", "end", "expected_text", "cursor"].contains(&key.as_str()))
        {
            return Err(serde::de::Error::custom(format!(
                "unknown DeleteText target field `{field}`"
            )));
        }
        let start = take_delete_json_field(object, "start")?;
        let end = take_delete_json_field(object, "end")?;
        let cursor =
            take_delete_field::<CursorDeletion, D::Error>(object.get("cursor").cloned(), "cursor")?;
        let expected_text = take_delete_field::<String, D::Error>(
            object.get("expected_text").cloned(),
            "expected_text",
        )?;
        if let Some(cursor) = cursor {
            if start.is_some() || end.is_some() {
                return Err(serde::de::Error::custom(
                    "DeleteText target cannot mix cursor and Edit Range fields",
                ));
            }
            let Some(expected_text) = expected_text else {
                return Err(serde::de::Error::custom(
                    "DeleteText cursor target requires `expected_text`",
                ));
            };
            if expected_text.is_empty() {
                return Err(serde::de::Error::custom(
                    "DeleteText cursor `expected_text` must not be empty",
                ));
            }
            return Ok(Self::Cursor(CursorDeletionTarget {
                cursor,
                expected_text,
            }));
        }
        let (Some(start), Some(end), Some(expected_text)) = (start, end, expected_text) else {
            return Err(serde::de::Error::custom(
                "DeleteText target must contain either `cursor` or complete Edit Range fields",
            ));
        };
        let start = parse_anchor(start, "start").map_err(serde::de::Error::custom)?;
        let end = parse_anchor(end, "end").map_err(serde::de::Error::custom)?;
        Ok(Self::Range(EditRange {
            start,
            end,
            expected_text,
        }))
    }
}

fn take_delete_json_field<E>(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<Option<serde_json::Value>, E>
where
    E: serde::de::Error,
{
    match object.get(name) {
        None => Ok(None),
        Some(serde_json::Value::Null) => {
            Err(E::custom(format!("DeleteText `{name}` must not be null")))
        }
        Some(value) => Ok(Some(value.clone())),
    }
}

fn take_delete_field<T, E>(field: Option<serde_json::Value>, name: &str) -> Result<Option<T>, E>
where
    T: serde::de::DeserializeOwned,
    E: serde::de::Error,
{
    match field {
        None => Ok(None),
        Some(serde_json::Value::Null) => {
            Err(E::custom(format!("DeleteText `{name}` must not be null")))
        }
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|error| E::custom(format!("DeleteText `{name}` is invalid: {error}"))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CursorDeletionTarget {
    pub cursor: CursorDeletion,
    pub expected_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CursorDeletion {
    pub line: u32,
    pub column: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeleteDirection {
    Backspace,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplaceTextAction {
    pub path: String,
    pub target: EditRange,
    pub text: String,
    #[serde(skip_serializing)]
    pub text_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_mode: Option<ReplaceTextEditMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceTextActionInput {
    pub path: String,
    pub target: EditRange,
    pub text: Option<String>,
    pub text_path: Option<String>,
    pub edit_mode: Option<ReplaceTextEditMode>,
    pub speed_ms: Option<u32>,
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for ReplaceTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = ReplaceTextActionInput::deserialize(deserializer)?;
        let (text, text_path) = match (input.text, input.text_path) {
            (Some(text), None) if !text.is_empty() => (text, None),
            (Some(_), None) => {
                return Err(serde::de::Error::custom(
                    "ReplaceText text must not be empty",
                ));
            }
            (None, Some(text_path)) if !text_path.trim().is_empty() => {
                (String::new(), Some(text_path))
            }
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "ReplaceText must define exactly one of `text` or `text_path`",
                ));
            }
            (None, _) => {
                return Err(serde::de::Error::custom(
                    "ReplaceText must define exactly one of `text` or `text_path`",
                ));
            }
        };
        if input.edit_mode.is_some() && input.speed_ms.unwrap_or(0) == 0 {
            return Err(serde::de::Error::custom(
                "ReplaceText edit_mode is only valid with a positive speed_ms",
            ));
        }
        if input.target_hold_ms == Some(0) {
            return Err(serde::de::Error::custom(
                "ReplaceText target_hold_ms must be greater than zero",
            ));
        }
        Ok(Self {
            path: input.path,
            target: input.target,
            text,
            text_path,
            edit_mode: input.edit_mode,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplaceTextEditMode {
    OverwriteSelection,
    DeleteThenType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDirectoryAction {
    pub path: String,
    #[serde(default = "default_true")]
    pub create_parents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFileAction {
    pub path: String,
    #[serde(default = "default_true")]
    pub create_parents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFileAction {
    pub path: String,
    #[serde(default)]
    pub if_missing: MissingTargetPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteDirectoryAction {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub if_missing: MissingTargetPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalEnsureAction {
    pub session_key: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCommandAction {
    pub session_key: String,
    pub cwd: String,
    pub command: String,
    #[serde(default = "default_accepted_exit_codes")]
    pub accepted_exit_codes: Vec<i32>,
    #[serde(default)]
    pub expected_output: Vec<String>,
    #[serde(default = "default_terminal_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_marker: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub new_terminal: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabOpenNotebookAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellSelectAction {
    pub path: String,
    pub cell_id: String,
    pub scroll: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<JupyterLabCellScrollAlign>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum JupyterLabCellScrollAlign {
    Auto,
    Start,
    Center,
    TopCenter,
    End,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellRunAction {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_cell_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_cell_id: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabRestartKernelAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabInterruptKernelAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabOpenPreviewAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCloseDocumentAction {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellInsertTextAction {
    pub path: String,
    pub cell_id: String,
    pub target: InsertTextTarget,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellDeleteTextAction {
    pub path: String,
    pub cell_id: String,
    pub target: DeleteTextTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<DeleteDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellReplaceTextAction {
    pub path: String,
    pub cell_id: String,
    pub target: EditRange,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_mode: Option<ReplaceTextEditMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hold_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JupyterLabCellType {
    Code,
    Markdown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellInsertAction {
    pub path: String,
    pub cell_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_cell_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_cell_id: Option<String>,
    pub cell_type: JupyterLabCellType,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellMoveAction {
    pub path: String,
    pub cell_id: String,
    pub direction: JupyterLabMoveDirection,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JupyterLabMoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellDuplicateAction {
    pub path: String,
    pub cell_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_cell_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JupyterLabCellDeleteAction {
    pub path: String,
    pub cell_id: String,
}

/// nbformat cell ids and notebook paths share the fields of the path-only
/// Notebook actions, so one input type keeps the metadata tolerance in one place.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotebookPathActionInput {
    pub path: String,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

macro_rules! path_only_notebook_action {
    ($action:ty) => {
        impl<'de> Deserialize<'de> for $action {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let input = NotebookPathActionInput::deserialize(deserializer)?;
                Ok(Self { path: input.path })
            }
        }
    };
}

path_only_notebook_action!(JupyterLabOpenNotebookAction);
path_only_notebook_action!(JupyterLabRestartKernelAction);
path_only_notebook_action!(JupyterLabInterruptKernelAction);
path_only_notebook_action!(JupyterLabOpenPreviewAction);
path_only_notebook_action!(JupyterLabCloseDocumentAction);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellInsertTextActionInput {
    pub path: String,
    pub cell_id: String,
    pub target: InsertTextTarget,
    pub text: String,
    #[serde(default)]
    pub speed_ms: Option<u32>,
    #[serde(default)]
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellInsertTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellInsertTextActionInput::deserialize(deserializer)?;
        if input.text.is_empty() {
            return Err(serde::de::Error::custom(
                "JupyterLabCellInsertText text must not be empty",
            ));
        }
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            target: input.target,
            text: input.text,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellDeleteTextActionInput {
    pub path: String,
    pub cell_id: String,
    pub target: DeleteTextTarget,
    #[serde(default)]
    pub direction: Option<DeleteDirection>,
    #[serde(default)]
    pub speed_ms: Option<u32>,
    #[serde(default)]
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellDeleteTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellDeleteTextActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            target: input.target,
            direction: input.direction,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellReplaceTextActionInput {
    pub path: String,
    pub cell_id: String,
    pub target: EditRange,
    pub text: String,
    #[serde(default)]
    pub edit_mode: Option<ReplaceTextEditMode>,
    #[serde(default)]
    pub speed_ms: Option<u32>,
    #[serde(default)]
    pub target_hold_ms: Option<u32>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellReplaceTextAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellReplaceTextActionInput::deserialize(deserializer)?;
        if input.text.is_empty() {
            return Err(serde::de::Error::custom(
                "JupyterLabCellReplaceText text must not be empty",
            ));
        }
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            target: input.target,
            text: input.text,
            edit_mode: input.edit_mode,
            speed_ms: input.speed_ms,
            target_hold_ms: input.target_hold_ms,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellInsertActionInput {
    pub path: String,
    pub cell_id: String,
    pub after_cell_id: Option<String>,
    pub before_cell_id: Option<String>,
    pub cell_type: JupyterLabCellType,
    pub source: String,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellInsertAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellInsertActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            after_cell_id: input.after_cell_id,
            before_cell_id: input.before_cell_id,
            cell_type: input.cell_type,
            source: input.source,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellMoveActionInput {
    pub path: String,
    pub cell_id: String,
    pub direction: JupyterLabMoveDirection,
    #[serde(default = "default_move_count")]
    pub count: u32,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellMoveAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellMoveActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            direction: input.direction,
            count: input.count,
        })
    }
}

fn default_move_count() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellDuplicateActionInput {
    pub path: String,
    pub cell_id: String,
    #[serde(default)]
    pub new_cell_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellDuplicateAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellDuplicateActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            new_cell_id: input.new_cell_id,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellDeleteActionInput {
    pub path: String,
    pub cell_id: String,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellDeleteAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellDeleteActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellSelectActionInput {
    pub path: String,
    pub cell_id: String,
    #[serde(default = "default_true")]
    pub scroll: bool,
    #[serde(default)]
    pub align: Option<JupyterLabCellScrollAlign>,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellSelectAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellSelectActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            cell_id: input.cell_id,
            scroll: input.scroll,
            align: input.align,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JupyterLabCellRunActionInput {
    pub path: String,
    #[serde(default)]
    pub from_cell_id: Option<String>,
    #[serde(default)]
    pub to_cell_id: Option<String>,
    #[serde(default = "default_cell_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    #[serde(rename = "source")]
    _source: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "confidence")]
    _confidence: Option<f64>,
    #[serde(default)]
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for JupyterLabCellRunAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = JupyterLabCellRunActionInput::deserialize(deserializer)?;
        Ok(Self {
            path: input.path,
            from_cell_id: input.from_cell_id,
            to_cell_id: input.to_cell_id,
            timeout_ms: input.timeout_ms,
        })
    }
}

fn default_cell_timeout_ms() -> u64 {
    300_000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingTargetPolicy {
    #[default]
    Error,
    Ignore,
}

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !value
}

fn default_accepted_exit_codes() -> Vec<i32> {
    vec![0]
}

fn default_terminal_timeout_ms() -> u64 {
    300_000
}
