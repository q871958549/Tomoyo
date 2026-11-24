use crate::SearchFilter;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Category tag displayed as a compact pill badge on each result row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCategory {
    App,
    File,
    Folder,
    Calc,
    Clipboard,
    Window,
    System,
    Plugin,
    Web,
}

impl ResultCategory {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::App => "APP",
            Self::File => "FILE",
            Self::Folder => "DIR",
            Self::Calc => "CALC",
            Self::Clipboard => "CLIP",
            Self::Window => "WIN",
            Self::System => "SYS",
            Self::Plugin => "EXT",
            Self::Web => "WEB",
        }
    }
}

/// Icon descriptor resolved natively by the UI shell.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum IconSpec {
    /// Extract native Windows shell icon for a file, folder, or `.lnk` path.
    ShellPath(String),
    /// Render a local PNG/ICO/WebP/JPG plugin or item logo image (`<= 256x256`).
    ImageFile(String),
    /// Use a built-in lightweight vector glyph or Unicode symbol.
    Symbol(String),
    /// Short monochrome monogram (1-2 chars) rendered inside a rounded pill.
    Monogram(String),
}

impl Default for IconSpec {
    fn default() -> Self {
        Self::Monogram("T".to_string())
    }
}

/// Snapshot of the latest clipboard item passed as optional search context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSnapshot {
    pub text: String,
    pub captured_at_unix: i64,
}

/// Rich pasted attachment ("Super Match" capsule) pinned into the search bar via Ctrl+V.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachedContext {
    Image {
        path: String,
        width: u32,
        height: u32,
    },
    Files {
        paths: Vec<String>,
    },
}

/// Query passed to both built-in providers and external plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryContext {
    /// Exact raw user input string.
    pub raw: String,
    /// Trimmed user input string.
    pub trimmed: String,
    /// Matched trigger keyword (if invoked via keyword routing).
    #[serde(default)]
    pub keyword: Option<String>,
    /// Remaining argument text after the trigger keyword.
    #[serde(default)]
    pub args: String,
    /// Monotonic query generation for cooperative cancellation.
    #[serde(default)]
    pub generation: u64,
    /// Maximum number of items expected from this provider.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Optional latest clipboard snippet for context-aware matching.
    #[serde(default)]
    pub clipboard: Option<ClipboardSnapshot>,
    /// Optional pasted image or file capsule attached directly into the search bar.
    #[serde(default)]
    pub attached: Option<AttachedContext>,
    /// Active group filter chip (`All`, `App`, `Folder`, `Pdf`, `Excel`, `Word`, `Ppt`, ...).
    #[serde(default)]
    pub filter: SearchFilter,
    /// Protocol v2: user-configured setting values for the target plugin (populated by `PluginHost`).
    #[serde(default)]
    pub plugin_settings: BTreeMap<String, Value>,
}

fn default_limit() -> usize {
    20
}

impl QueryContext {
    pub fn new(raw: impl Into<String>, generation: u64, limit: usize) -> Self {
        let raw = raw.into();
        let trimmed = raw.trim().to_string();
        Self {
            raw,
            trimmed: trimmed.clone(),
            keyword: None,
            args: trimmed,
            generation,
            limit,
            clipboard: None,
            attached: None,
            filter: SearchFilter::All,
            plugin_settings: BTreeMap::new(),
        }
    }

    pub fn with_keyword(mut self, keyword: impl Into<String>, args: impl Into<String>) -> Self {
        self.keyword = Some(keyword.into());
        self.args = args.into();
        self
    }

    pub fn with_filter(mut self, filter: SearchFilter) -> Self {
        self.filter = filter;
        self
    }
}

/// A single item displayed in Tomoyo's result list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultItem {
    /// Stable identifier used for Frecency ranking and plugin execution routing.
    pub id: String,
    /// Provider or plugin ID that produced this item.
    pub provider_id: String,
    /// Primary title line.
    pub title: String,
    /// Secondary subtitle line (path, explanation, preview snippet).
    #[serde(default)]
    pub subtitle: String,
    /// Logical category pill.
    pub category: ResultCategory,
    /// Relevance score (higher ranks first; typical range 0..=10000).
    #[serde(default)]
    pub score: i64,
    /// Icon specification.
    #[serde(default)]
    pub icon: IconSpec,
    /// Optional right-side metadata pill (e.g., file size "2.4 MB" or "HEX").
    #[serde(default)]
    pub badge: Option<String>,
    /// Text copied when user hits Ctrl+C on this result.
    #[serde(default)]
    pub copy_text: Option<String>,
    /// Ordered list of actions; `actions[0]` is executed on Enter, rest via Tab action menu.
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
    /// Optional declarative preview card (SDUI foundation).
    #[serde(default)]
    pub preview: Option<PreviewSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    pub kind: ActionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionKind {
    LaunchPath {
        path: String,
        #[serde(default)]
        args: Option<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    LaunchAsAdmin {
        path: String,
        #[serde(default)]
        args: Option<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    RevealInExplorer {
        path: String,
    },
    OpenTerminalAt {
        dir: String,
    },
    OpenUrl {
        url: String,
    },
    CopyText {
        text: String,
        #[serde(default)]
        paste_immediately: bool,
    },
    CopyImage {
        path: String,
        #[serde(default)]
        paste_immediately: bool,
    },
    FocusWindow {
        hwnd: isize,
    },
    FillQuery {
        query: String,
    },
    SystemCommand {
        command: SystemCommandKind,
    },
    InternalCommand {
        command: InternalCommandKind,
    },
    PluginExecute {
        plugin_id: String,
        action_id: String,
        #[serde(default)]
        payload: Value,
    },
    /// Spawns the ephemeral `tomoyo-webview-host` executable on demand (0 idle RAM).
    OpenWebView {
        plugin_id: String,
        title: String,
        entry_path: String,
        #[serde(default)]
        utools_compat: bool,
        #[serde(default)]
        initial_query: String,
        #[serde(default = "default_wv_width")]
        width: u32,
        #[serde(default = "default_wv_height")]
        height: u32,
        #[serde(default = "default_wv_mem")]
        max_memory_mb: u32,
        #[serde(default)]
        icon_path: Option<String>,
        #[serde(default)]
        display_mode: Option<String>,
    },
}

fn default_wv_width() -> u32 {
    860
}
fn default_wv_height() -> u32 {
    600
}
fn default_wv_mem() -> u32 {
    256
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCommandKind {
    LockWorkstation,
    Sleep,
    EmptyRecycleBin,
    OpenSettings,
    OpenTaskManager,
    OpenTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalCommandKind {
    TriggerHibernateGc,
    ReloadAll,
    QuitApp,
    RefreshQuery,
    OpenConfigFile,
    OpenSettingsWindow,
    ResetWindowGeometry,
    SetTheme { theme: String },
    SetLanguage { lang: String },
    ToggleClipboardPin { id_key: String },
    DeleteClipboardItem { id_key: String },
    ClearUnpinnedClipboard,
    OpenClipboardManager { focus_key: Option<String> },
    ToggleClipboardCapture,
    OpenTranslator { text: String, target_lang: Option<String> },
    StartCapture { mode: String },
    CheckForUpdates,
    InstallUpdate,
    ClearAttachedContext,
    CopyImageAsBase64 { path: String },
    KillProcess { pid: u32, name: String },
    ToggleAutostart,
    TogglePreviewPane,
    PinImageFile { path: String },
    ExtractZipHere { path: String },
    CopyFileContents { path: String },
    ComputeFileSha256 { path: String },
    RunFlow { flow_id: String, input: String },
}

/// Declarative Server-Driven UI (SDUI) preview card rendered natively without WebView.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreviewSpec {
    Text {
        title: String,
        body: String,
    },
    KeyValue {
        title: String,
        entries: Vec<(String, String)>,
    },
    /// Declarative native Rich Card (title + subtitle + pill badges + key/value rows + body).
    RichCard {
        title: String,
        #[serde(default)]
        subtitle: String,
        #[serde(default)]
        badges: Vec<String>,
        #[serde(default)]
        rows: Vec<(String, String)>,
        #[serde(default)]
        body: String,
    },
    /// Lazy bounded disk preview (max 8 KB text or binary metadata summary).
    FilePreview {
        path: String,
    },
    /// Lazy bounded scaled thumbnail decoded on demand and dropped on hibernate.
    ImagePreview {
        path: String,
        width: u32,
        height: u32,
    },
}

/// Declarative plugin execution response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecuteResponse {
    /// Whether Tomoyo should hide after executing this action (defaults to true).
    #[serde(default = "default_true")]
    pub hide_window: bool,
    /// Optional status/toast message displayed in the footer.
    #[serde(default)]
    pub toast: Option<String>,
    /// Optional text to write to the system clipboard.
    #[serde(default)]
    pub copy_text: Option<String>,
    /// Optional URL to open via the OS default browser.
    #[serde(default)]
    pub open_url: Option<String>,
    /// Optional replacement query to fill into the search bar (keeps launcher open).
    #[serde(default)]
    pub fill_query: Option<String>,
    /// Optional replacement declarative view (SDUI v1).
    #[serde(default)]
    pub view: Option<ViewModel>,
}

fn default_true() -> bool {
    true
}

/// Minimal Server-Driven UI view model for plugins that return rich detail or list transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewModel {
    List { items: Vec<ResultItem> },
    Detail { title: String, body: String },
}
