pub mod client_settings;
pub mod client_state;
pub mod client_transition;
pub mod course;
pub mod course_import;
pub mod course_session;
pub mod discovery;
pub mod environment;
pub mod jupyterlab;
pub mod learning;
pub mod protocol;
pub mod runtime;
pub mod server;

pub use client_settings::{
    CLIENT_SETTINGS_VERSION, CaptionFontFamily, CaptionPosition, CaptionSettings, ClientSettings,
    DEFAULT_CAPTION_ACCENT_COLOR, DEFAULT_CAPTION_BACKGROUND_COLOR, DEFAULT_CAPTION_TEXT_COLOR,
    LanguagePreference, ThemePreference,
};
pub use client_state::{
    ADAPTER_DISCONNECTED_KEY, ClientEvent, ClientStateReducer, ClientTransitionKind,
    ClientTransitionPhase, ClientTransitionStage, ClientTransitionView, PendingLearningIntent,
    Warning, adapter_disconnected_message, adapter_readiness, playback_is_retryable,
};
pub use client_transition::{
    ClientTransitionRequirement, ClientTransitionRuntime, ClientTransitionTicket,
    StopTransitionRequest, TransitionCommitError, TransitionRequest, TransitionStartError,
    TransitionViews, TransitionWaitOutcome,
};
pub use course::{
    Action, ActionNarration, ActionNarrationClip, ActionNarrationPhase, ActionNarrationPhaseClips,
    AdapterId, Anchor, AnchorAt, CaptionSegment, CloseFileAction, ClosePreviewAction, Course,
    CourseError, CourseNode, CreateDirectoryAction, CreateFileAction, CursorDeletion,
    CursorDeletionTarget, DeleteDirection, DeleteDirectoryAction, DeleteFileAction,
    DeleteTextAction, DeleteTextTarget, Diagnostic, EditRange, EnvironmentRequirement,
    InsertTextAction, InsertTextTarget, JupyterLabCellDeleteTextAction,
    JupyterLabCellInsertTextAction, JupyterLabCellReplaceTextAction, JupyterLabCellRunAction,
    JupyterLabCellScrollAlign, JupyterLabCellSelectAction, JupyterLabInterruptKernelAction,
    JupyterLabOpenNotebookAction, JupyterLabRestartKernelAction, LoadedStep, MissingTargetPolicy,
    OpenFileAction, PackageLoad, PackageReport, Placement, PlaybackAction, PlaybackStep,
    ReplaceTextAction, ReplaceTextEditMode, Step, StepReference, TerminalCommandAction,
    TerminalEnsureAction, TextAnchor, Unit, UnitAsset, UnitEntry, WaitAction,
    caption_formula_layout, caption_segments, check_course, check_package, load_package,
    load_package_with_report, validate_caption_markup,
};
pub use course_session::{
    ActionNarrationAudioView, ChallengeStatus, ChallengeView, CommandExecutionStart,
    CommandInterruption, CommandRecoveryView, CourseSession, CourseSessionError,
    CourseSessionState, CourseTreeNodeView, CourseView, RecentCourse, StepView, UnitLocation,
    UnitPlaybackPlan, UnitResetScope, UnitStatus, UnitTransitionPlan, UnitView, find_unit_by_path,
    unit_session_key,
};
pub use discovery::{
    DiscoveryFile, DiscoveryHandle, global_runtime_dir, global_runtime_path, global_state_db_path,
    new_discovery,
};
pub use environment::{
    EnvironmentCheckItem, EnvironmentCheckReport, Version, output_satisfies_minimum_version,
};
pub use jupyterlab::{
    DEFAULT_CONTROL_PORT, JupyterLabLaunch, JupyterLabProcess, JupyterLabProvisioner,
    JupyterLabReadinessProbe, JupyterLabRuntime, JupyterLabServer, JupyterLabServerConfig,
    JupyterLabSpawner, JupyterLabStartError, STALE_SERVER_MARKER_FILE, StaleServerCleanup,
    StaleServerMarker, SystemJupyterLabProvisioner, SystemJupyterLabSpawner,
    SystemStaleServerCleanup, TcpJupyterLabProbe,
};
pub use learning::{
    LearningController, LearningControllerError, LearningMode, LearningPhase, LearningView,
};
pub use protocol::{
    AckPayload, AckStatus, CancelPayload, CancelReason, CheckPayload, Envelope, HelloAckPayload,
    HelloPayload, MessageType, PROTOCOL_VERSION,
};
pub use runtime::{
    BASELINE_EXTENSION_PACKAGE, BASELINE_JUPYTERLAB_VERSION, COURSE_ENVIRONMENT_DIR,
    CommandRequest, CommandResult, CommandRunner, CourseEnvironment, CourseEnvironmentBaseline,
    PythonVersion, RuntimeManager, RuntimeSetupError, SystemCommandRunner, course_environment_dir,
};
pub use server::{
    AdapterAuth, AdapterTarget, ConnectionState, NarrationClipContinuation, NarrationClipRef,
    PlaybackCommand, PlaybackCommandSender, PlaybackInterruption, PlaybackInterruptionReason,
    PlaybackPosition, PlaybackRecovery, PlaybackResume, PlaybackState, PlaybackUnavailable,
    ServerError, ServerHandle, start_server_with_path_and_resume,
    start_server_with_path_and_resume_for_adapter,
    start_server_with_path_and_resume_for_adapter_on_port,
    start_server_with_path_and_resume_for_adapter_on_port_with_auth,
};

mod atomic_file;
pub use atomic_file::write_atomic;

mod window_position;
pub use window_position::{
    MonitorBounds, StoreError, WindowGeometry, WindowGeometryStore, WindowPosition,
    centered_position, monitor_containing, rect_intersects_monitor, restore_position,
};
