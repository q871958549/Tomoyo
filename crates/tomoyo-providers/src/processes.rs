use std::sync::{Arc, RwLock};
use tomoyo_core::{inspect_pid_working_set, Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, QueryContext,
    ResultCategory, ResultItem,
};

/// Native Windows process inspector & killer (`kill <name/pid>` or `ps <name>`).
pub struct ProcessProvider {
    config: Arc<RwLock<TomoyoConfig>>,
}

impl ProcessProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self { config }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

impl Provider for ProcessProvider {
    fn id(&self) -> &'static str {
        "builtin.processes"
    }

    fn name(&self) -> &'static str {
        "Process Inspector & Killer"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["kill", "ps"]
    }

    fn supports_global(&self) -> bool {
        false
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let filter = ctx.args.trim().to_lowercase();
        let my_pid = std::process::id();

        let mut procs = list_running_processes();
        procs.retain(|(pid, name, _)| {
            if *pid == 0 || *pid == 4 || *pid == my_pid {
                return false;
            }
            if filter.is_empty() {
                return true;
            }
            name.to_lowercase().contains(&filter) || pid.to_string().contains(&filter)
        });

        // Sort highest memory first so runaway resource hogs surface immediately!
        procs.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));

        procs
            .into_iter()
            .take(ctx.limit.min(25))
            .enumerate()
            .map(|(idx, (pid, name, ws_bytes))| {
                let ram_mb = ws_bytes as f64 / (1024.0 * 1024.0);
                let subtitle = match lang {
                    Lang::Zh => format!("PID {pid} · 占用内存 {ram_mb:.1} MB · 回车结束该进程"),
                    Lang::En => format!("PID {pid} · RAM {ram_mb:.1} MB · Enter to terminate process"),
                };
                ResultItem {
                    id: format!("proc:{pid}"),
                    provider_id: self.id().into(),
                    title: name.clone(),
                    subtitle,
                    category: ResultCategory::System,
                    score: 9_200 - (idx as i64 * 15),
                    icon: IconSpec::Monogram("PID".into()),
                    badge: Some(format!("{ram_mb:.1} MB")),
                    copy_text: Some(pid.to_string()),
                    actions: vec![
                        ActionSpec {
                            id: "kill".into(),
                            label: lang.pick("结束进程 (Terminate)", "Terminate Process").into(),
                            shortcut: Some("Enter".into()),
                            kind: ActionKind::InternalCommand {
                                command: InternalCommandKind::KillProcess {
                                    pid,
                                    name: name.clone(),
                                },
                            },
                        },
                        ActionSpec {
                            id: "copy_pid".into(),
                            label: lang.pick("复制 PID", "Copy PID").into(),
                            shortcut: Some("Ctrl+C".into()),
                            kind: ActionKind::CopyText {
                                text: pid.to_string(),
                                paste_immediately: false,
                            },
                        },
                    ],
                    preview: Some(PreviewSpec::KeyValue {
                        title: name,
                        entries: vec![
                            ("PID".into(), pid.to_string()),
                            (lang.pick("工作集内存", "Working Set").into(), format!("{ram_mb:.2} MB")),
                            (lang.pick("操作提示", "Action").into(), lang.pick("回车立即结束进程", "Enter to kill").into()),
                        ],
                    }),
                }
            })
            .collect()
    }
}

#[cfg(windows)]
fn list_running_processes() -> Vec<(u32, String, u64)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::with_capacity(128);
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let len = pe
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..len]);
                let pid = pe.th32ProcessID;
                let ws = inspect_pid_working_set(pid).unwrap_or(0);
                out.push((pid, name, ws));
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn list_running_processes() -> Vec<(u32, String, u64)> {
    Vec::new()
}
