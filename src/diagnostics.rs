use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl DiagnosticLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Lifecycle,
    Startup,
    Profile,
    Input,
    Packet,
    Apply,
    Telemetry,
    Safety,
    Error,
}

impl DiagnosticCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lifecycle => "LIFECYCLE",
            Self::Startup => "STARTUP",
            Self::Profile => "PROFILE",
            Self::Input => "INPUT",
            Self::Packet => "PACKET",
            Self::Apply => "APPLY",
            Self::Telemetry => "TELEMETRY",
            Self::Safety => "SAFETY",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticField {
    pub key: String,
    pub value: String,
}

impl DiagnosticField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeDiagnosticEvent {
    pub timestamp_ms: u64,
    pub level: DiagnosticLevel,
    pub category: DiagnosticCategory,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DiagnosticField>,
}

impl BridgeDiagnosticEvent {
    pub fn field_value(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticsLog {
    path: PathBuf,
}

impl DiagnosticsLog {
    pub fn default_path() -> Result<PathBuf> {
        if let Some(base_dir) =
            std::env::var_os("APPDATA").or_else(|| std::env::var_os("LOCALAPPDATA"))
        {
            return Ok(PathBuf::from(base_dir)
                .join("Torquebridge")
                .join("bridge-diagnostics.jsonl"));
        }

        Ok(std::env::current_dir()
            .context("failed to resolve current directory for diagnostics")?
            .join(".torquebridge-bridge-diagnostics.jsonl"))
    }

    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("bridge-diagnostics-sessions"))
            .unwrap_or_else(|| PathBuf::from("bridge-diagnostics-sessions"))
    }

    pub fn clear(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("failed to create diagnostics directory")?;
        }

        fs::write(&self.path, "").context("failed to clear diagnostics log")?;
        Ok(())
    }

    pub fn append(
        &self,
        level: DiagnosticLevel,
        category: DiagnosticCategory,
        message: impl Into<String>,
        fields: Vec<DiagnosticField>,
    ) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("failed to create diagnostics directory")?;
        }

        let event = BridgeDiagnosticEvent {
            timestamp_ms: unix_timestamp_ms(),
            level,
            category,
            message: message.into(),
            fields,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context("failed to open diagnostics log for append")?;
        writeln!(file, "{}", serde_json::to_string(&event)?)
            .context("failed to append diagnostics event")?;
        Ok(())
    }

    pub fn read_recent(&self, limit: usize) -> Result<Vec<BridgeDiagnosticEvent>> {
        if limit == 0 || !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(&self.path).context("failed to open diagnostics log")?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line.context("failed to read diagnostics event line")?;
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(event) = serde_json::from_str::<BridgeDiagnosticEvent>(&line) {
                events.push(event);
            }
        }

        if events.len() > limit {
            Ok(events.split_off(events.len() - limit))
        } else {
            Ok(events)
        }
    }

    pub fn export_session(&self) -> Result<PathBuf> {
        if !self.path.exists() {
            return Err(anyhow!("no bridge diagnostics session exists to export"));
        }

        let archive_dir = self.archive_dir();
        fs::create_dir_all(&archive_dir)
            .context("failed to create diagnostics archive directory")?;

        let export_path = archive_dir.join(format!("session-{}.jsonl", unix_timestamp_ms()));
        fs::copy(&self.path, &export_path).with_context(|| {
            format!(
                "failed to export diagnostics session to {}",
                export_path.display()
            )
        })?;

        Ok(export_path)
    }

    pub fn latest_exported_session(&self) -> Result<PathBuf> {
        let archive_dir = self.archive_dir();
        let mut sessions = fs::read_dir(&archive_dir)
            .with_context(|| {
                format!(
                    "failed to read diagnostics archive {}",
                    archive_dir.display()
                )
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .collect::<Vec<_>>();

        sessions.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        sessions
            .pop()
            .ok_or_else(|| anyhow!("no exported diagnostics sessions found"))
    }
}

pub fn format_event(event: &BridgeDiagnosticEvent) -> String {
    let mut line = format!(
        "{} / {}: {}",
        event.category.label(),
        event.level.label(),
        event.message,
    );

    if !event.fields.is_empty() {
        let detail = event
            .fields
            .iter()
            .map(|field| format!("{}={}", field.key, field.value))
            .collect::<Vec<_>>()
            .join(", ");
        line.push_str(" | ");
        line.push_str(&detail);
    }

    line
}

fn unix_timestamp_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_log_round_trips_recent_events() {
        let path = std::env::temp_dir().join(format!(
            "torquebridge-diagnostics-{}.jsonl",
            unix_timestamp_ms()
        ));
        let log = DiagnosticsLog::new(&path);

        log.clear().expect("clear diagnostics");
        log.append(
            DiagnosticLevel::Info,
            DiagnosticCategory::Lifecycle,
            "Bridge launched",
            vec![DiagnosticField::new("pid", "4242")],
        )
        .expect("append event");
        log.append(
            DiagnosticLevel::Warning,
            DiagnosticCategory::Safety,
            "Safety reset delayed",
            Vec::new(),
        )
        .expect("append event");

        let recent = log.read_recent(8).expect("read diagnostics");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "Bridge launched");
        assert_eq!(recent[1].category, DiagnosticCategory::Safety);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn formatted_events_include_field_pairs() {
        let line = format_event(&BridgeDiagnosticEvent {
            timestamp_ms: 0,
            level: DiagnosticLevel::Error,
            category: DiagnosticCategory::Error,
            message: "Bridge apply failed".to_string(),
            fields: vec![DiagnosticField::new("command_count", "3")],
        });

        assert!(line.contains("ERROR / ERROR: Bridge apply failed"));
        assert!(line.contains("command_count=3"));
    }

    #[test]
    fn bridge_event_field_value_reads_named_fields() {
        let event = BridgeDiagnosticEvent {
            timestamp_ms: 0,
            level: DiagnosticLevel::Info,
            category: DiagnosticCategory::Telemetry,
            message: "Runtime telemetry snapshot".to_string(),
            fields: vec![DiagnosticField::new("uptime", "1.0 s")],
        };

        assert_eq!(event.field_value("uptime"), Some("1.0 s"));
        assert_eq!(event.field_value("missing"), None);
    }

    #[test]
    fn diagnostics_log_exports_and_discovers_latest_session() {
        let base_dir = std::env::temp_dir().join(format!(
            "torquebridge-diagnostics-export-{}",
            unix_timestamp_ms()
        ));
        let live_log_path = base_dir.join("bridge-diagnostics.jsonl");
        let log = DiagnosticsLog::new(&live_log_path);

        log.clear().expect("clear live log");
        log.append(
            DiagnosticLevel::Info,
            DiagnosticCategory::Lifecycle,
            "Bridge launched",
            Vec::new(),
        )
        .expect("append event");

        let archive_dir = log.archive_dir();
        fs::create_dir_all(&archive_dir).expect("create archive dir");
        let older_path = archive_dir.join("session-1.jsonl");
        fs::write(&older_path, "{}\n").expect("write older session");

        let export_path = log.export_session().expect("export session");
        assert!(export_path.exists());
        assert_eq!(
            log.latest_exported_session().expect("latest session"),
            export_path
        );

        let _ = fs::remove_dir_all(base_dir);
    }
}
