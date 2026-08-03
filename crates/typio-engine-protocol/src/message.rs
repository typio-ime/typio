use crate::{ProtocolError, Result, decode_hex, decode_hex_string, encode_hex, encode_hex_string};

/// Engine category carried by the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// Keyboard input engine.
    Keyboard,
    /// Voice transcription engine.
    Voice,
}

impl EngineKind {
    /// Canonical wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Voice => "voice",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "keyboard" => Ok(Self::Keyboard),
            "voice" => Ok(Self::Voice),
            _ => Err(ProtocolError::invalid(format!(
                "unknown engine kind '{value}'"
            ))),
        }
    }
}

/// Configuration field type published during the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaType {
    /// UTF-8 string.
    String,
    /// Signed 32-bit integer.
    Integer,
    /// Boolean.
    Boolean,
    /// Finite 64-bit float.
    Float,
}

impl SchemaType {
    const fn code(self) -> &'static str {
        match self {
            Self::String => "0",
            Self::Integer => "1",
            Self::Boolean => "2",
            Self::Float => "3",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "0" => Ok(Self::String),
            "1" => Ok(Self::Integer),
            "2" => Ok(Self::Boolean),
            "3" => Ok(Self::Float),
            _ => Err(ProtocolError::invalid(format!(
                "unknown schema type '{value}'"
            ))),
        }
    }
}

/// Typed configuration default.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaDefault {
    /// String default.
    String(String),
    /// Integer default.
    Integer(i32),
    /// Boolean default.
    Boolean(bool),
    /// Float default.
    Float(f64),
}

impl SchemaDefault {
    /// Type implied by this value.
    pub const fn field_type(&self) -> SchemaType {
        match self {
            Self::String(_) => SchemaType::String,
            Self::Integer(_) => SchemaType::Integer,
            Self::Boolean(_) => SchemaType::Boolean,
            Self::Float(_) => SchemaType::Float,
        }
    }

    fn encode(&self) -> String {
        match self {
            Self::String(value) => encode_hex_string(value),
            Self::Integer(value) => value.to_string(),
            Self::Boolean(value) => u8::from(*value).to_string(),
            Self::Float(value) => value.to_string(),
        }
    }

    fn parse(field_type: SchemaType, value: &str) -> Result<Self> {
        match field_type {
            SchemaType::String => decode_hex_string(value).map(Self::String),
            SchemaType::Integer => value.parse::<i32>().map(Self::Integer).map_err(|_| {
                ProtocolError::invalid(format!("invalid integer schema default '{value}'"))
            }),
            SchemaType::Boolean => parse_bool(value).map(Self::Boolean),
            SchemaType::Float => {
                let value = value.parse::<f64>().map_err(|_| {
                    ProtocolError::invalid(format!("invalid float schema default '{value}'"))
                })?;
                if value.is_finite() {
                    Ok(Self::Float(value))
                } else {
                    Err(ProtocolError::invalid("non-finite float schema default"))
                }
            }
        }
    }
}

/// One engine-owned configuration field.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    /// Fully-qualified `engines.<name>.*` key.
    pub key: String,
    /// Typed default value.
    pub default: SchemaDefault,
    /// Human-readable settings label.
    pub label: Option<String>,
    /// Settings section identifier.
    pub section: Option<String>,
    /// Integer UI minimum.
    pub minimum: i32,
    /// Integer UI maximum.
    pub maximum: i32,
    /// Integer UI step.
    pub step: i32,
    /// Enumerated string choices.
    pub options: Vec<String>,
    /// Optional runtime-property identifier.
    pub runtime_property: Option<String>,
}

impl SchemaField {
    fn encode(&self) -> String {
        let options = self
            .options
            .iter()
            .map(|value| encode_hex_string(value))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "SCHEMA\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            encode_hex_string(&self.key),
            self.default.field_type().code(),
            self.default.encode(),
            encode_optional(&self.label),
            encode_optional(&self.section),
            self.minimum,
            self.maximum,
            self.step,
            options,
            encode_optional(&self.runtime_property),
        )
    }

    fn parse(line: &str) -> Result<Self> {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 11 || fields[0] != "SCHEMA" {
            return Err(ProtocolError::invalid(format!(
                "SCHEMA record expected 11 fields, got {}",
                fields.len()
            )));
        }
        let key = decode_hex_string(fields[1])?;
        if key.is_empty() {
            return Err(ProtocolError::invalid("schema key is empty"));
        }
        let field_type = SchemaType::parse(fields[2])?;
        let default = SchemaDefault::parse(field_type, fields[3])?;
        let options = if fields[9].is_empty() {
            Vec::new()
        } else {
            fields[9]
                .split(',')
                .map(decode_hex_string)
                .collect::<Result<Vec<_>>>()?
        };
        Ok(Self {
            key,
            default,
            label: decode_optional(fields[4])?,
            section: decode_optional(fields[5])?,
            minimum: parse_i32(fields[6], "schema minimum")?,
            maximum: parse_i32(fields[7], "schema maximum")?,
            step: parse_i32(fields[8], "schema step")?,
            options,
            runtime_property: decode_optional(fields[10])?,
        })
    }
}

/// Metadata sent by an engine before heavy initialization.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineHello {
    /// Manifest engine identifier.
    pub engine: String,
    /// Engine category.
    pub kind: EngineKind,
    /// Engine-owned configuration schema.
    pub schema: Vec<SchemaField>,
}

impl EngineHello {
    /// Encode the current handshake payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut lines = vec![
            "protocol\t1.0".to_string(),
            format!("engine\t{}", self.engine),
            format!("type\t{}", self.kind.as_str()),
        ];
        lines.extend(self.schema.iter().map(SchemaField::encode));
        lines.join("\n").into_bytes()
    }

    /// Decode and validate an engine handshake payload.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(payload)
            .map_err(|error| ProtocolError::invalid(format!("invalid hello UTF-8: {error}")))?;
        let mut protocol = None;
        let mut engine = None;
        let mut kind = None;
        let mut schema = Vec::new();
        for line in text.lines() {
            if line.starts_with("SCHEMA\t") {
                schema.push(SchemaField::parse(line)?);
                continue;
            }
            let (key, value) = line.split_once('\t').unwrap_or((line, ""));
            match key {
                "protocol" => protocol = Some(value),
                "engine" => engine = Some(value.to_string()),
                "type" => kind = Some(EngineKind::parse(value)?),
                _ => {}
            }
        }
        if protocol != Some("1.0") {
            return Err(ProtocolError::invalid(
                "hello is missing compatible protocol 1.0",
            ));
        }
        Ok(Self {
            engine: engine.ok_or_else(|| ProtocolError::invalid("hello is missing engine id"))?,
            kind: kind.ok_or_else(|| ProtocolError::invalid("hello is missing engine kind"))?,
            schema,
        })
    }
}

/// Host handshake acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHello {
    /// Expected manifest engine identifier.
    pub engine: String,
    /// Expected engine category.
    pub kind: EngineKind,
    /// Directory containing the host-owned `core.toml`.
    pub config_dir: String,
    /// Root directory for persistent engine data.
    pub data_dir: String,
    /// Root directory for runtime state.
    pub state_dir: String,
}

impl HostHello {
    /// Encode the current host handshake payload.
    pub fn encode(&self) -> Vec<u8> {
        format!(
            "protocol\t1.0\nengine\t{}\ntype\t{}\nconfig-dir\t{}\ndata-dir\t{}\nstate-dir\t{}",
            self.engine,
            self.kind.as_str(),
            encode_hex_string(&self.config_dir),
            encode_hex_string(&self.data_dir),
            encode_hex_string(&self.state_dir),
        )
        .into_bytes()
    }

    /// Decode a host handshake payload.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(payload)
            .map_err(|error| ProtocolError::invalid(format!("invalid hello UTF-8: {error}")))?;
        let mut protocol = None;
        let mut engine = None;
        let mut kind = None;
        let mut config_dir = String::new();
        let mut data_dir = String::new();
        let mut state_dir = String::new();
        for line in text.lines() {
            let (key, value) = line.split_once('\t').unwrap_or((line, ""));
            match key {
                "protocol" => protocol = Some(value),
                "engine" => engine = Some(value.to_string()),
                "type" => kind = Some(EngineKind::parse(value)?),
                "config-dir" => config_dir = decode_hex_string(value)?,
                "data-dir" => data_dir = decode_hex_string(value)?,
                "state-dir" => state_dir = decode_hex_string(value)?,
                "SCHEMA" => {
                    return Err(ProtocolError::invalid(
                        "host hello must not contain schema records",
                    ));
                }
                _ => {}
            }
        }
        if protocol != Some("1.0") {
            return Err(ProtocolError::invalid(
                "hello is missing compatible protocol 1.0",
            ));
        }
        Ok(Self {
            engine: engine.ok_or_else(|| ProtocolError::invalid("hello is missing engine id"))?,
            kind: kind.ok_or_else(|| ProtocolError::invalid("hello is missing engine kind"))?,
            config_dir,
            data_dir,
            state_dir,
        })
    }
}

/// Key state on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// Key press.
    Press,
    /// Key release.
    Release,
}

/// Lossless key event sent to an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// Engine-context identifier.
    pub context_id: u64,
    /// Press or release.
    pub state: KeyState,
    /// Raw hardware key code.
    pub keycode: u32,
    /// Resolved keysym.
    pub keysym: u32,
    /// Modifier mask.
    pub modifiers: u32,
    /// Resolved Unicode scalar or zero.
    pub unicode: u32,
    /// Event timestamp in milliseconds.
    pub time: u64,
    /// Repeat marker.
    pub is_repeat: bool,
    /// Unshifted keysym.
    pub base_keysym: u32,
}

/// Typed host request.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Initialize heavyweight engine state.
    Initialize,
    /// Shut down the engine process.
    Shutdown,
    /// Deactivate the engine.
    Deactivate,
    /// Focus a context.
    FocusIn(u64),
    /// Unfocus a context.
    FocusOut(u64),
    /// Reset a context.
    Reset(u64),
    /// Reload engine configuration.
    ReloadConfig,
    /// Process one key.
    ProcessKey(KeyEvent),
    /// Query availability.
    Availability,
    /// Process little-endian `f32` audio samples.
    ProcessAudio(Vec<u8>),
    /// List keyboard modes.
    ListModes,
    /// Query the active mode for a context.
    GetActiveMode(u64),
    /// Set or cycle the active mode.
    SetActiveMode {
        /// Engine context.
        context_id: u64,
        /// `None` means cycle.
        mode_id: Option<String>,
    },
    /// Commit a candidate.
    CommitCandidate {
        /// Engine context.
        context_id: u64,
        /// Candidate index.
        index: i32,
    },
    /// List engine commands.
    ListCommands,
    /// Invoke an engine command.
    InvokeCommand(String),
}

impl Request {
    /// Operation name used for timeout policy and diagnostics.
    pub const fn operation(&self) -> &'static str {
        match self {
            Self::Initialize => "init",
            Self::Shutdown => "shutdown",
            Self::Deactivate => "deactivate",
            Self::FocusIn(_) => "focus-in",
            Self::FocusOut(_) => "focus-out",
            Self::Reset(_) => "reset",
            Self::ReloadConfig => "reload-config",
            Self::ProcessKey(_) => "process-key",
            Self::Availability => "availability",
            Self::ProcessAudio(_) => "process-audio",
            Self::ListModes => "list-modes",
            Self::GetActiveMode(_) => "get-active-mode",
            Self::SetActiveMode { .. } => "set-active-mode",
            Self::CommitCandidate { .. } => "commit-candidate",
            Self::ListCommands => "list-commands",
            Self::InvokeCommand(_) => "invoke-command",
        }
    }

    /// Encode the request payload.
    pub fn encode(&self) -> Vec<u8> {
        let line = match self {
            Self::Initialize => "init".to_string(),
            Self::Shutdown => "shutdown".to_string(),
            Self::Deactivate => "deactivate".to_string(),
            Self::FocusIn(id) => format!("focus-in\t{id}"),
            Self::FocusOut(id) => format!("focus-out\t{id}"),
            Self::Reset(id) => format!("reset\t{id}"),
            Self::ReloadConfig => "reload-config".to_string(),
            Self::ProcessKey(event) => format!(
                "process-key\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                event.context_id,
                match event.state {
                    KeyState::Press => "press",
                    KeyState::Release => "release",
                },
                event.keycode,
                event.keysym,
                event.modifiers,
                event.unicode,
                event.time,
                u8::from(event.is_repeat),
                event.base_keysym,
            ),
            Self::Availability => "availability".to_string(),
            Self::ProcessAudio(bytes) => format!("process-audio\t{}", encode_hex(bytes)),
            Self::ListModes => "list-modes".to_string(),
            Self::GetActiveMode(id) => format!("get-active-mode\t{id}"),
            Self::SetActiveMode {
                context_id,
                mode_id,
            } => format!(
                "set-active-mode\t{}\t{}",
                context_id,
                mode_id
                    .as_deref()
                    .map(encode_hex_string)
                    .unwrap_or_default()
            ),
            Self::CommitCandidate { context_id, index } => {
                format!("commit-candidate\t{context_id}\t{index}")
            }
            Self::ListCommands => "list-commands".to_string(),
            Self::InvokeCommand(id) => format!("invoke-command\t{}", encode_hex_string(id)),
        };
        line.into_bytes()
    }

    /// Decode and validate a request payload.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let line = std::str::from_utf8(payload)
            .map_err(|error| ProtocolError::invalid(format!("invalid request UTF-8: {error}")))?;
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["init"] => Ok(Self::Initialize),
            ["shutdown"] => Ok(Self::Shutdown),
            ["deactivate"] => Ok(Self::Deactivate),
            ["focus-in", id] => Ok(Self::FocusIn(parse_u64(id, "context id")?)),
            ["focus-out", id] => Ok(Self::FocusOut(parse_u64(id, "context id")?)),
            ["reset", id] => Ok(Self::Reset(parse_u64(id, "context id")?)),
            ["reload-config"] => Ok(Self::ReloadConfig),
            ["availability"] => Ok(Self::Availability),
            ["process-audio", audio] => Ok(Self::ProcessAudio(decode_hex(audio)?)),
            ["list-modes"] => Ok(Self::ListModes),
            ["get-active-mode", id] => Ok(Self::GetActiveMode(parse_u64(id, "context id")?)),
            ["set-active-mode", id, mode] => Ok(Self::SetActiveMode {
                context_id: parse_u64(id, "context id")?,
                mode_id: if mode.is_empty() {
                    None
                } else {
                    Some(decode_hex_string(mode)?)
                },
            }),
            ["commit-candidate", id, index] => Ok(Self::CommitCandidate {
                context_id: parse_u64(id, "context id")?,
                index: parse_i32(index, "candidate index")?,
            }),
            ["list-commands"] => Ok(Self::ListCommands),
            ["invoke-command", id] => Ok(Self::InvokeCommand(decode_hex_string(id)?)),
            [
                "process-key",
                id,
                state,
                keycode,
                keysym,
                modifiers,
                unicode,
                time,
                repeat,
                base,
            ] => Ok(Self::ProcessKey(KeyEvent {
                context_id: parse_u64(id, "context id")?,
                state: match *state {
                    "press" => KeyState::Press,
                    "release" => KeyState::Release,
                    _ => {
                        return Err(ProtocolError::invalid(format!(
                            "invalid key state '{state}'"
                        )));
                    }
                },
                keycode: parse_u32(keycode, "keycode")?,
                keysym: parse_u32(keysym, "keysym")?,
                modifiers: parse_u32(modifiers, "modifiers")?,
                unicode: parse_u32(unicode, "unicode")?,
                time: parse_u64(time, "time")?,
                is_repeat: parse_bool(repeat)?,
                base_keysym: parse_u32(base, "base keysym")?,
            })),
            _ => Err(ProtocolError::invalid(format!(
                "unknown or malformed request '{line}'"
            ))),
        }
    }
}

/// Engine availability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Created but not initialized.
    Uninitialized,
    /// Performing bounded warm-up.
    Preparing,
    /// Ready for input.
    Ready,
    /// Initialization or runtime failure.
    Failed,
}

/// Key-processing result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// Engine ignored the key.
    NotHandled,
    /// Engine consumed the key.
    Handled,
    /// Engine kept composition while the key passes through.
    PassThrough,
}

/// On-focus mode salience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModeSalience {
    /// Do not auto-reveal.
    #[default]
    Quiet,
    /// Reveal because the mode changes typing semantics materially.
    Notable,
}

/// Engine mode metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    /// Stable identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Short status label.
    pub display_label: Option<String>,
    /// Icon name.
    pub icon: Option<String>,
    /// Engine profile identifier.
    pub profile_id: Option<String>,
    /// Engine profile label.
    pub profile_label: Option<String>,
    /// Detailed description.
    pub description: Option<String>,
    /// Whether the mode is active.
    pub is_active: bool,
    /// On-focus salience.
    pub salience: ModeSalience,
}

impl Mode {
    fn parse(payload: &str) -> Result<Self> {
        let fields = payload.split('\t').collect::<Vec<_>>();
        if fields.len() < 8 {
            return Err(ProtocolError::invalid("short mode payload"));
        }
        Ok(Self {
            id: decode_hex_string(fields[0])?,
            label: decode_hex_string(fields[1])?,
            display_label: decode_optional(fields[2])?,
            icon: decode_optional(fields[3])?,
            profile_id: decode_optional(fields[4])?,
            profile_label: decode_optional(fields[5])?,
            description: decode_optional(fields[6])?,
            is_active: parse_bool(fields[7])?,
            salience: if fields.get(8) == Some(&"1") {
                ModeSalience::Notable
            } else {
                ModeSalience::Quiet
            },
        })
    }

    /// Encode the fields following a `MODE` or `ACTIVE_MODE` record.
    pub fn encode(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            encode_hex_string(&self.id),
            encode_hex_string(&self.label),
            encode_optional(&self.display_label),
            encode_optional(&self.icon),
            encode_optional(&self.profile_id),
            encode_optional(&self.profile_label),
            encode_optional(&self.description),
            u8::from(self.is_active),
            match self.salience {
                ModeSalience::Quiet => 0,
                ModeSalience::Notable => 1,
            }
        )
    }
}

/// Engine command metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Stable command identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
}

/// Preedit segment formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreeditFormat {
    /// No decoration.
    #[default]
    None,
    /// Underline decoration.
    Underline,
    /// Selected/highlighted decoration.
    Highlight,
}

/// Owned preedit segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreeditSegment {
    /// Segment text.
    pub text: String,
    /// Rendering hint.
    pub format: PreeditFormat,
}

/// Owned candidate entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Candidate text.
    pub text: String,
    /// Optional annotation.
    pub comment: Option<String>,
    /// Optional selection label.
    pub label: Option<String>,
}

/// Atomic preedit and candidate snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Composition {
    /// Preedit segments.
    pub segments: Vec<PreeditSegment>,
    /// UTF-8 cursor position.
    pub cursor_pos: i32,
    /// Candidate entries.
    pub candidates: Vec<Candidate>,
    /// Zero-based page number.
    pub page: i32,
    /// Candidate page size.
    pub page_size: i32,
    /// Total candidate count.
    pub total: i32,
    /// Selected candidate index.
    pub selected: i32,
    /// Previous-page availability.
    pub has_prev: bool,
    /// Next-page availability.
    pub has_next: bool,
    /// Host-managed selection flags.
    pub host_managed_selection: u32,
}

impl Composition {
    fn parse(payload: &str) -> Result<Self> {
        let fields = payload.split('\t').collect::<Vec<_>>();
        if fields.len() < 10 {
            return Err(ProtocolError::invalid("short composition payload"));
        }
        let segments = decode_hex_list(fields[8])?
            .into_iter()
            .map(|text| PreeditSegment {
                text,
                format: PreeditFormat::Underline,
            })
            .collect();
        let candidates = decode_hex_list(fields[9])?
            .into_iter()
            .map(|text| Candidate {
                text,
                comment: None,
                label: None,
            })
            .collect();
        Ok(Self {
            cursor_pos: parse_i32(fields[0], "composition cursor")?,
            page: parse_i32(fields[1], "composition page")?,
            page_size: parse_i32(fields[2], "composition page size")?,
            total: parse_i32(fields[3], "composition total")?,
            selected: parse_i32(fields[4], "composition selection")?,
            has_prev: parse_bool(fields[5])?,
            has_next: parse_bool(fields[6])?,
            host_managed_selection: parse_u32(fields[7], "host selection flags")?,
            segments,
            candidates,
        })
    }

    /// Encode the fields following a `COMPOSITION` record.
    pub fn encode(&self) -> String {
        let segments = self
            .segments
            .iter()
            .map(|segment| encode_hex_string(&segment.text))
            .collect::<Vec<_>>()
            .join(",");
        let candidates = self
            .candidates
            .iter()
            .map(|candidate| encode_hex_string(&candidate.text))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.cursor_pos,
            self.page,
            self.page_size,
            self.total,
            self.selected,
            u8::from(self.has_prev),
            u8::from(self.has_next),
            self.host_managed_selection,
            segments,
            candidates,
        )
    }
}

/// One response record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyRecord {
    /// Successful operation marker.
    Ok,
    /// Engine error code and optional detail.
    Error(String),
    /// Key-processing result.
    KeyResult(KeyResult),
    /// Availability state.
    Availability(Availability),
    /// UTF-8 text result.
    Text(String),
    /// Mode list entry.
    Mode(Mode),
    /// Command list entry.
    Command(Command),
    /// Current active mode.
    ActiveMode(Mode),
    /// Atomic composition update.
    Composition(Composition),
    /// Committed text.
    Commit(String),
    /// Clear the current composition.
    Clear,
}

impl ReplyRecord {
    fn encode(&self) -> String {
        match self {
            Self::Ok => "OK".to_string(),
            Self::Error(error) => format!("ERR\t{error}"),
            Self::KeyResult(result) => format!(
                "RESULT\t{}",
                match result {
                    KeyResult::NotHandled => "NOT_HANDLED",
                    KeyResult::Handled => "HANDLED",
                    KeyResult::PassThrough => "PASS_THROUGH",
                }
            ),
            Self::Availability(state) => format!(
                "AVAILABILITY\t{}",
                match state {
                    Availability::Uninitialized => "UNINITIALIZED",
                    Availability::Preparing => "PREPARING",
                    Availability::Ready => "READY",
                    Availability::Failed => "FAILED",
                }
            ),
            Self::Text(text) => format!("TEXT\t{}", encode_hex_string(text)),
            Self::Mode(mode) => format!("MODE\t{}", mode.encode()),
            Self::Command(command) => format!(
                "COMMAND\t{}\t{}",
                encode_hex_string(&command.id),
                encode_hex_string(&command.label)
            ),
            Self::ActiveMode(mode) => format!("ACTIVE_MODE\t{}", mode.encode()),
            Self::Composition(composition) => {
                format!("COMPOSITION\t{}", composition.encode())
            }
            Self::Commit(text) => format!("COMMIT\t{}", encode_hex_string(text)),
            Self::Clear => "CLEAR".to_string(),
        }
    }
}

/// Typed response payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    /// Ordered response records.
    pub records: Vec<ReplyRecord>,
}

impl Reply {
    /// Encode all response records followed by `END`.
    pub fn encode(&self) -> Vec<u8> {
        let mut lines = self
            .records
            .iter()
            .map(ReplyRecord::encode)
            .collect::<Vec<_>>();
        lines.push("END".to_string());
        lines.join("\n").into_bytes()
    }

    /// Decode ordered response records.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(payload)
            .map_err(|error| ProtocolError::invalid(format!("invalid response UTF-8: {error}")))?;
        let mut records = Vec::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line == "END" {
                continue;
            }
            let (operation, value) = line.split_once('\t').unwrap_or((line, ""));
            records.push(match operation {
                "OK" => ReplyRecord::Ok,
                "ERR" => ReplyRecord::Error(value.to_string()),
                "RESULT" => ReplyRecord::KeyResult(match value {
                    "NOT_HANDLED" => KeyResult::NotHandled,
                    "HANDLED" | "COMPOSING" | "COMMITTED" => KeyResult::Handled,
                    "PASS_THROUGH" => KeyResult::PassThrough,
                    _ => {
                        return Err(ProtocolError::invalid(format!(
                            "unknown key result '{value}'"
                        )));
                    }
                }),
                "AVAILABILITY" => ReplyRecord::Availability(match value {
                    "UNINITIALIZED" => Availability::Uninitialized,
                    "PREPARING" => Availability::Preparing,
                    "READY" => Availability::Ready,
                    "FAILED" => Availability::Failed,
                    _ => {
                        return Err(ProtocolError::invalid(format!(
                            "unknown availability '{value}'"
                        )));
                    }
                }),
                "TEXT" => ReplyRecord::Text(decode_hex_string(value)?),
                "MODE" => ReplyRecord::Mode(Mode::parse(value)?),
                "COMMAND" => {
                    let (id, label) = value
                        .split_once('\t')
                        .ok_or_else(|| ProtocolError::invalid("short command payload"))?;
                    let id = decode_hex_string(id)?;
                    if id.is_empty() {
                        return Err(ProtocolError::invalid("empty command id"));
                    }
                    ReplyRecord::Command(Command {
                        id,
                        label: decode_hex_string(label)?,
                    })
                }
                "ACTIVE_MODE" => ReplyRecord::ActiveMode(Mode::parse(value)?),
                "COMPOSITION" => ReplyRecord::Composition(Composition::parse(value)?),
                "COMMIT" => ReplyRecord::Commit(decode_hex_string(value)?),
                "CLEAR" => ReplyRecord::Clear,
                _ => {
                    return Err(ProtocolError::invalid(format!(
                        "unknown engine response operation '{operation}'"
                    )));
                }
            });
        }
        Ok(Self { records })
    }
}

fn encode_optional(value: &Option<String>) -> String {
    value.as_deref().map(encode_hex_string).unwrap_or_default()
}

fn decode_optional(value: &str) -> Result<Option<String>> {
    if value.is_empty() {
        Ok(None)
    } else {
        decode_hex_string(value).map(Some)
    }
}

fn decode_hex_list(value: &str) -> Result<Vec<String>> {
    if value.is_empty() {
        Ok(Vec::new())
    } else {
        value.split(',').map(decode_hex_string).collect()
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "TRUE" => Ok(true),
        "0" | "false" | "FALSE" => Ok(false),
        _ => Err(ProtocolError::invalid(format!("invalid boolean '{value}'"))),
    }
}

fn parse_i32(value: &str, label: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .map_err(|_| ProtocolError::invalid(format!("invalid {label} '{value}'")))
}

fn parse_u32(value: &str, label: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|_| ProtocolError::invalid(format!("invalid {label} '{value}'")))
}

fn parse_u64(value: &str, label: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| ProtocolError::invalid(format!("invalid {label} '{value}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trip_preserves_schema() {
        let hello = EngineHello {
            engine: "rime".to_string(),
            kind: EngineKind::Keyboard,
            schema: vec![SchemaField {
                key: "engines.rime.schema".to_string(),
                default: SchemaDefault::String("luna_pinyin".to_string()),
                label: Some("Schema".to_string()),
                section: Some("Engine".to_string()),
                minimum: 0,
                maximum: 0,
                step: 0,
                options: vec!["luna_pinyin".to_string(), "double_pinyin".to_string()],
                runtime_property: Some("schema".to_string()),
            }],
        };
        assert_eq!(EngineHello::decode(&hello.encode()).unwrap(), hello);
    }

    #[test]
    fn host_hello_round_trip_preserves_runtime_paths() {
        let hello = HostHello {
            engine: "rime".to_string(),
            kind: EngineKind::Keyboard,
            config_dir: "/tmp/typio config".to_string(),
            data_dir: "/tmp/typio-data".to_string(),
            state_dir: "/tmp/typio-state".to_string(),
        };
        assert_eq!(HostHello::decode(&hello.encode()).unwrap(), hello);
    }

    #[test]
    fn request_round_trip_covers_key_and_audio() {
        let requests = [
            Request::ProcessKey(KeyEvent {
                context_id: 7,
                state: KeyState::Press,
                keycode: 30,
                keysym: b'a' as u32,
                modifiers: 1,
                unicode: b'A' as u32,
                time: 99,
                is_repeat: true,
                base_keysym: b'a' as u32,
            }),
            Request::ProcessAudio(vec![0, 1, 0xfe, 0xff]),
            Request::SetActiveMode {
                context_id: 7,
                mode_id: Some("native".to_string()),
            },
        ];
        for request in requests {
            assert_eq!(Request::decode(&request.encode()).unwrap(), request);
        }
    }

    #[test]
    fn reply_round_trip_preserves_ordered_outputs() {
        let reply = Reply {
            records: vec![
                ReplyRecord::Composition(Composition {
                    segments: vec![PreeditSegment {
                        text: "ni".to_string(),
                        format: PreeditFormat::Underline,
                    }],
                    candidates: vec![Candidate {
                        text: "你".to_string(),
                        comment: None,
                        label: None,
                    }],
                    page_size: 10,
                    total: 1,
                    selected: 0,
                    ..Composition::default()
                }),
                ReplyRecord::KeyResult(KeyResult::Handled),
                ReplyRecord::ActiveMode(Mode {
                    id: "native".to_string(),
                    label: "Native".to_string(),
                    display_label: Some("中".to_string()),
                    icon: None,
                    profile_id: None,
                    profile_label: None,
                    description: None,
                    is_active: true,
                    salience: ModeSalience::Notable,
                }),
            ],
        };
        assert_eq!(Reply::decode(&reply.encode()).unwrap(), reply);
    }

    #[test]
    fn strict_decoders_reject_malformed_payloads() {
        assert!(Request::decode(b"process-key\t1").is_err());
        assert!(Reply::decode(b"UNKNOWN\tvalue").is_err());
        assert!(EngineHello::decode(b"protocol\t2.0\nengine\tx\ntype\tkeyboard").is_err());
    }
}
