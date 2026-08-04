use crate::database::repositories::{
    meeting::MeetingsRepository, setting::SettingsRepository,
    summary::SummaryProcessesRepository, transcript_chunk::TranscriptChunksRepository,
};
use crate::state::AppState;
use crate::summary::metadata::{
    read_detected_summary_language_from_metadata, read_summary_language_from_metadata,
    write_detected_summary_language_to_metadata, write_summary_language_to_metadata,
};
use crate::summary::language_detection::{
    detect_summary_language, SummaryLanguageDetection,
};
use crate::summary::service::SummaryService;
use crate::summary::clean_llm_markdown_output;
use crate::summary::llm_client::{generate_summary, LLMProvider};
use log::{error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub status: String,
    #[serde(rename = "meetingName")]
    pub meeting_name: Option<String>,
    pub meeting_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTranscriptResponse {
    pub message: String,
    pub process_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLanguageStorage {
    Metadata,
    LocalFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummaryLanguagePreference {
    pub language: Option<String>,
    pub storage: SummaryLanguageStorage,
}

impl MeetingSummaryLanguagePreference {
    fn metadata(language: Option<String>) -> Self {
        Self {
            language,
            storage: SummaryLanguageStorage::Metadata,
        }
    }

    fn local_fallback() -> Self {
        Self {
            language: None,
            storage: SummaryLanguageStorage::LocalFallback,
        }
    }
}

enum MeetingFolderResolution {
    Folder(PathBuf),
    NoFolder,
}

/// Saves a meeting summary (Native SQLx implementation)
///
/// Expected format: { "markdown": "...", "summary_json": [...BlockNote blocks...] }
#[tauri::command]
pub async fn api_save_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: serde_json::Value,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_meeting_summary (native) called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::update_meeting_summary(pool, &meeting_id, &summary).await {
        Ok(true) => {
            log_info!("Summary saved successfully for meeting_id: {}", meeting_id);
            Ok(serde_json::json!({
                "message": "Meeting summary saved successfully"
            }))
        }
        Ok(false) => {
            log_warn!(
                "Meeting not found or invalid JSON for meeting_id: {}",
                meeting_id
            );
            Err("Meeting not found or can't convert the json".into())
        }
        Err(e) => {
            log_error!("Failed to save meeting summary for {}: {}", meeting_id, e);
            Err(e.to_string())
        }
    }
}

/// Exports a meeting summary as `summary.md` into the meeting's recording folder
/// (the same folder that holds `transcripts.json`), so the summary lives next to the transcript.
///
/// The `markdown` argument is written verbatim; the caller assembles the document
/// (title + metadata + body). Returns the absolute path of the written file, and errors
/// if the meeting has no recording folder to write into.
#[tauri::command]
pub async fn api_export_meeting_summary_markdown<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    markdown: String,
) -> Result<String, String> {
    log_info!(
        "api_export_meeting_summary_markdown called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            let path = write_summary_markdown_file(&folder, &markdown).map_err(|e| {
                log_error!("Failed to write summary.md for {}: {}", meeting_id, e);
                format!("Failed to write summary file: {}", e)
            })?;
            log_info!("Summary exported to {} for meeting_id: {}", path, meeting_id);
            Ok(path)
        }
        MeetingFolderResolution::NoFolder => Err(
            "This meeting has no recording folder, so the summary can't be saved next to the transcript."
                .to_string(),
        ),
    }
}

/// Gets the per-meeting summary language override from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => read_summary_language_from_metadata(&folder)
            .map(MeetingSummaryLanguagePreference::metadata)
            .map_err(|e| e.to_string()),
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the per-meeting summary language override in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_summary_language_to_metadata(&folder, summary_language.as_deref())
                .map_err(|e| e.to_string())?;
            read_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Gets the cached Auto-detected summary language from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_detected_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => read_detected_summary_language_from_metadata(&folder)
            .map(MeetingSummaryLanguagePreference::metadata)
            .map_err(|e| e.to_string()),
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the cached Auto-detected summary language in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    detected_summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_detected_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        detected_summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_detected_summary_language_to_metadata(&folder, detected_summary_language.as_deref())
                .map_err(|e| e.to_string())?;
            read_detected_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Detects the dominant supported summary language from transcript segments.
#[tauri::command]
pub async fn api_detect_transcript_summary_language(
    transcript_texts: Vec<String>,
) -> Result<SummaryLanguageDetection, String> {
    Ok(detect_summary_language(&transcript_texts))
}

async fn resolve_meeting_folder(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<MeetingFolderResolution, String> {
    let meeting = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting metadata: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;

    let Some(folder_path) = meeting.folder_path.filter(|p| !p.trim().is_empty()) else {
        return Ok(MeetingFolderResolution::NoFolder);
    };

    Ok(MeetingFolderResolution::Folder(PathBuf::from(folder_path)))
}

/// Writes `summary.md` into the given meeting folder using an atomic temp-file + rename,
/// mirroring how `transcripts.json` is persisted. Returns the absolute path of the file.
fn write_summary_markdown_file(folder: &PathBuf, markdown: &str) -> std::io::Result<String> {
    std::fs::create_dir_all(folder)?;

    let summary_path = folder.join("summary.md");
    let temp_path = folder.join(".summary.md.tmp");

    std::fs::write(&temp_path, markdown)?;
    std::fs::rename(&temp_path, &summary_path)?;

    Ok(summary_path.to_string_lossy().into_owned())
}

/// Gets summary status and data (Native SQLx implementation)
///
/// Returns summary status (pending/processing/completed/failed) and parsed result data
#[tauri::command]
pub async fn api_get_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    _auth_token: Option<String>,
) -> Result<SummaryResponse, String> {
    log_info!(
        "api_get_summary (native) called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::get_summary_data_for_meeting(pool, &meeting_id).await {
        Ok(Some(process)) => {
            let status = process.status.to_lowercase();
            let error = process.error;

            // Parse result data if it exists (regardless of status)
            // This allows displaying restored summaries after cancellation or failure
            let data = if let Some(result_str) = process.result {
                match serde_json::from_str::<serde_json::Value>(&result_str) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        log_error!("Failed to parse summary result JSON: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            // Fetch meeting title from database
            let meeting_name = match MeetingsRepository::get_meeting(pool, &meeting_id).await {
                Ok(Some(meeting_details)) => {
                    log_info!("Fetched meeting title: {}", &meeting_details.title);
                    Some(meeting_details.title)
                }
                Ok(None) => {
                    log_warn!("Meeting not found for meeting_id: {}", meeting_id);
                    None
                }
                Err(e) => {
                    log_error!("Failed to fetch meeting title: {}", e);
                    None
                }
            };

            let response = SummaryResponse {
                status: status.clone(),
                meeting_name,
                meeting_id: meeting_id.clone(),
                start: process.start_time.map(|t| t.to_rfc3339()),
                end: process.end_time.map(|t| t.to_rfc3339()),
                data,
                error,
            };

            log_info!(
                "Summary status for {}: {}, has_data: {}, meeting_name: {:?}",
                meeting_id,
                status,
                response.data.is_some(),
                response.meeting_name
            );
            Ok(response)
        }
        Ok(None) => {
            log_info!("No summary process found for meeting_id: {}", meeting_id);

            // Still fetch meeting title for idle state
            let meeting_name = match MeetingsRepository::get_meeting(pool, &meeting_id).await {
                Ok(Some(meeting_details)) => Some(meeting_details.title),
                _ => None,
            };

            Ok(SummaryResponse {
                status: "idle".to_string(),
                meeting_name,
                meeting_id,
                start: None,
                end: None,
                data: None,
                error: None,
            })
        }
        Err(e) => {
            log_error!("Error retrieving summary for {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve summary: {}", e))
        }
    }
}

/// Processes transcript and generates summary (Native SQLx implementation)
///
/// Spawns a background task and returns immediately with process_id
#[tauri::command]
pub async fn api_process_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    model: String,
    model_name: String,
    meeting_id: Option<String>,
    _chunk_size: Option<i32>,
    _overlap: Option<i32>,
    custom_prompt: Option<String>,
    template_id: Option<String>,
    summary_language: Option<String>,
    _auth_token: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    use uuid::Uuid;

    let m_id = meeting_id.unwrap_or_else(|| format!("meeting-{}", Uuid::new_v4()));
    log_info!(
        "api_process_transcript (native) called for meeting_id: {}, model: {}",
        &m_id,
        &model
    );

    let pool = state.db_manager.pool().clone();
    let final_prompt = custom_prompt.unwrap_or_else(|| "".to_string());
    let final_template_id = template_id.unwrap_or_else(|| "daily_standup".to_string());

    // Normalise empty / whitespace-only to None so "" and null behave identically
    let summary_language = summary_language.and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    });

    // Create or reset the process entry in the database
    SummaryProcessesRepository::create_or_reset_process(&pool, &m_id)
        .await
        .map_err(|e| format!("Failed to initialize process: {}", e))?;

    log_info!("✓ Summary process initialized for meeting_id: {}", &m_id);

    // Save transcript chunks data (matching Python backend behavior)
    let chunk_size = _chunk_size.unwrap_or(40000);
    let overlap = _overlap.unwrap_or(1000);

    TranscriptChunksRepository::save_transcript_data(
        &pool,
        &m_id,
        &text,
        &model,
        &model_name,
        chunk_size,
        overlap,
    )
    .await
    .map_err(|e| format!("Failed to save transcript data: {}", e))?;

    log_info!("✓ Transcript chunks saved for meeting_id: {}", &m_id);

    // Spawn background task for actual processing
    let meeting_id_clone = m_id.clone();
    tauri::async_runtime::spawn(async move {
        SummaryService::process_transcript_background(
            app,
            pool,
            meeting_id_clone.clone(),
            text,
            model,
            model_name,
            final_prompt,
            final_template_id,
            summary_language,
        )
        .await;
    });

    log_info!("🚀 Background task spawned for meeting_id: {}", &m_id);

    Ok(ProcessTranscriptResponse {
        message: "Summary generation started".to_string(),
        process_id: m_id,
    })
}

/// Cancels an ongoing summary generation process
///
/// This command triggers the cancellation token for the specified meeting,
/// stopping the summary generation gracefully.
#[tauri::command]
pub async fn api_cancel_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<serde_json::Value, String> {
    log_info!("api_cancel_summary called for meeting_id: {}", meeting_id);

    // Trigger cancellation via the service
    let cancelled = SummaryService::cancel_summary(&meeting_id);

    if cancelled {
        // Update database status to cancelled
        let pool = state.db_manager.pool();
        if let Err(e) = SummaryProcessesRepository::update_process_cancelled(pool, &meeting_id).await {
            log_error!("Failed to update DB status to cancelled for {}: {}", meeting_id, e);
            return Err(format!("Failed to update cancellation status: {}", e));
        }

        log_info!("Successfully cancelled summary generation for meeting_id: {}", meeting_id);
        Ok(serde_json::json!({
            "message": "Summary generation cancelled successfully",
            "meeting_id": meeting_id,
        }))
    } else {
        log_warn!("No active summary generation found for meeting_id: {}", meeting_id);
        Ok(serde_json::json!({
            "message": "No active summary generation to cancel",
            "meeting_id": meeting_id,
        }))
    }
}

/// Sentinel returned when the configured summary model is not a local model
/// (Ollama or built-in AI). The frontend matches this string to show a hint
/// instead of an error, since the live rolling summary is intentionally local-only.
const LIVE_SUMMARY_LOCAL_ONLY: &str = "LIVE_SUMMARY_LOCAL_ONLY";

/// Most-recent characters of the new transcript excerpt sent per live-summary
/// call. Bounds the local model's context window and latency. Normally a tick
/// carries ~60s of speech; this is the safety net for a delayed or resumed tick.
const LIVE_SUMMARY_MAX_CHARS: usize = 16_000;

/// Trailing characters of the summary-so-far passed back as context. Only the
/// most recent bullets matter for avoiding repetition, and this keeps the prompt
/// from growing with the meeting.
const LIVE_SUMMARY_CONTEXT_MAX_CHARS: usize = 4_000;

/// Per-call wall-clock budget — shorter than generate_summary's internal 300s so
/// a slow tick cannot pile up behind the ~60s regeneration cadence.
const LIVE_SUMMARY_TIMEOUT_SECS: u64 = 45;

/// Marker the model emits when an excerpt contains nothing worth recording;
/// mapped to an empty result so the caller appends nothing.
const LIVE_SUMMARY_NOTHING_NEW: &str = "NOTHING_NEW";

/// System prompt for the first live-summary chunk of a recording, when there is
/// no earlier summary to extend.
const LIVE_SUMMARY_SYSTEM_PROMPT: &str = "You are a real-time meeting assistant. You are given the opening excerpt of a meeting transcript that is still in progress. Summarize it as a short, skimmable Markdown bullet list.\n\nRules:\n- Output ONLY Markdown bullet points (each top-level line starting with \"- \"). No title, no headings, no preamble, and no closing remarks.\n- Capture the key discussion points, decisions made, and action items mentioned.\n- Aim for 2-5 concise bullets. Use indented sub-bullets sparingly to group related detail.\n- The transcript is live and may end mid-sentence. Summarize only what is clearly stated; do not speculate about what will be said next.\n- Write in English regardless of the transcript language.\n- If the excerpt contains nothing worth recording (silence, greetings, small talk), output exactly: NOTHING_NEW";

/// System prompt for every later chunk: the model sees only the new excerpt plus
/// the summary so far, and returns bullets to append rather than a rewrite.
const LIVE_SUMMARY_APPEND_SYSTEM_PROMPT: &str = "You are a real-time meeting assistant. You are given a NEW excerpt from a meeting transcript that is still in progress, along with the bullet summary already written for everything said before it. Write bullets covering ONLY the new excerpt; they will be appended to the existing summary.\n\nRules:\n- Output ONLY Markdown bullet points (each top-level line starting with \"- \"). No title, no headings, no preamble, and no closing remarks.\n- Summarize ONLY the new excerpt: what was newly discussed, decided, or assigned.\n- Do NOT repeat, restate, revise, or re-summarize anything already covered by the existing summary. The existing summary is context only — never reproduce its bullets.\n- Aim for 1-4 concise bullets.\n- The excerpt is live and may start or end mid-sentence. Summarize only what is clearly stated; do not speculate about what will be said next.\n- Write in English regardless of the transcript language.\n- If the excerpt adds nothing worth recording (silence, small talk, filler, or only points already summarized), output exactly: NOTHING_NEW";

/// Keep the last `max_chars` characters of `text`, or all of it when shorter.
fn tail_chars(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count > max_chars {
        text.chars().skip(char_count - max_chars).collect()
    } else {
        text.to_string()
    }
}

/// Builds the (system, user) prompt pair for one live-summary tick.
///
/// Without a summary so far this is the meeting's opening excerpt, so the model
/// simply summarizes it. With one, the model is shown the existing bullets as
/// context and asked for bullets covering only the new excerpt — both inputs are
/// capped so the prompt stays a fixed size however long the meeting runs.
fn build_live_summary_prompt(
    transcript: &str,
    previous_summary: Option<&str>,
) -> (&'static str, String) {
    let excerpt = tail_chars(transcript.trim(), LIVE_SUMMARY_MAX_CHARS);
    let previous = previous_summary.map(str::trim).unwrap_or("");

    if previous.is_empty() {
        (
            LIVE_SUMMARY_SYSTEM_PROMPT,
            format!(
                "Summarize this opening excerpt as Markdown bullet points.\n\n<transcript>\n{}\n</transcript>",
                excerpt
            ),
        )
    } else {
        (
            LIVE_SUMMARY_APPEND_SYSTEM_PROMPT,
            format!(
                "Here is the summary written so far. Do not repeat any of it.\n\n<existing_summary>\n{}\n</existing_summary>\n\nWrite Markdown bullet points covering ONLY this new excerpt of the transcript.\n\n<new_transcript>\n{}\n</new_transcript>",
                tail_chars(previous, LIVE_SUMMARY_CONTEXT_MAX_CHARS),
                excerpt
            ),
        )
    }
}

/// Generates the next chunk of the ephemeral "rolling" summary shown during an
/// active recording. One-shot LLM call; nothing is persisted.
///
/// Incremental by design: `transcript` is only the excerpt spoken since the last
/// call, and `previous_summary` is the summary built so far. The model returns
/// bullets for the new excerpt alone, which the caller appends — so earlier
/// bullets are never rewritten, and prompt size stays flat as the meeting runs
/// instead of growing with (and eventually truncating) the full transcript.
///
/// Local-only: if the configured summary model is not Ollama or built-in AI (or
/// no model is configured), returns the `LIVE_SUMMARY_LOCAL_ONLY` sentinel and
/// makes no call. Returns cleaned Markdown bullets, or an empty string when the
/// transcript is empty.
#[tauri::command]
pub async fn generate_live_summary<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    transcript: String,
    previous_summary: Option<String>,
) -> Result<String, String> {
    let pool = state.db_manager.pool();

    // Load the configured summary model; a missing row means it isn't set up yet.
    let setting = match SettingsRepository::get_model_config(pool).await {
        Ok(Some(s)) => s,
        Ok(None) => return Err(LIVE_SUMMARY_LOCAL_ONLY.to_string()),
        Err(e) => return Err(format!("Failed to load model config: {}", e)),
    };

    let provider = LLMProvider::from_str(setting.provider.trim())
        .map_err(|_| LIVE_SUMMARY_LOCAL_ONLY.to_string())?;

    // Local-only per product decision: refuse cloud providers without calling them.
    if provider != LLMProvider::Ollama && provider != LLMProvider::BuiltInAI {
        return Err(LIVE_SUMMARY_LOCAL_ONLY.to_string());
    }

    let model_name = setting.model.trim().to_string();
    if model_name.is_empty() {
        return Err(LIVE_SUMMARY_LOCAL_ONLY.to_string());
    }

    if transcript.trim().is_empty() {
        return Ok(String::new());
    }
    let (system_prompt, user_prompt) =
        build_live_summary_prompt(&transcript, previous_summary.as_deref());

    // The built-in AI sidecar needs the app data dir (service.rs pattern).
    let app_data_dir = app.path().app_data_dir().ok();
    let ollama_endpoint = setting.ollama_endpoint.clone();

    let client = reqwest::Client::new();
    let call = generate_summary(
        &client,
        &provider,
        &model_name,
        "", // local providers need no api key
        system_prompt,
        &user_prompt,
        ollama_endpoint.as_deref(),
        None, // custom_openai_endpoint (cloud-only, unreachable here)
        None, // max_tokens
        None, // temperature
        None, // top_p
        app_data_dir.as_ref(),
        None, // cancellation handled by tokio::time::timeout below
    );

    match tokio::time::timeout(Duration::from_secs(LIVE_SUMMARY_TIMEOUT_SECS), call).await {
        Ok(Ok(raw)) => Ok(strip_nothing_new(&clean_llm_markdown_output(&raw))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Live summary timed out".to_string()),
    }
}

/// Maps the model's "nothing to add" marker to an empty result.
///
/// Models emit the marker in varying shapes — bare, bulleted, or fenced in
/// punctuation — so anything whose only alphanumeric content is the marker
/// counts. Bullets that merely mention it alongside real content are kept.
fn strip_nothing_new(markdown: &str) -> String {
    let trimmed = markdown.trim();
    let stripped: String = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if stripped.eq_ignore_ascii_case(LIVE_SUMMARY_NOTHING_NEW) {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// Persists a live rolling summary as the meeting's summary right after a
/// recording is saved, so meeting-details shows it as an editable starting draft.
///
/// Mirrors exactly what the normal generation flow writes — a `transcript_chunks`
/// row (so `api_get_summary`'s JOIN succeeds and a later Regenerate has its data)
/// plus a `completed` `summary_processes` row — but skips the LLM entirely and
/// stores the pre-computed `summary` markdown produced live during recording.
#[tauri::command]
pub async fn api_prefill_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    transcript: String,
    summary: serde_json::Value,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_prefill_meeting_summary called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();

    // Ensure a transcript_chunks row exists so api_get_summary's JOIN returns the
    // summary (and a later Regenerate finds the transcript data it expects).
    TranscriptChunksRepository::save_transcript_data(
        pool,
        &meeting_id,
        &transcript,
        "live-summary",
        "live-summary",
        40000,
        1000,
    )
    .await
    .map_err(|e| format!("Failed to save transcript chunks: {}", e))?;

    // Create the process row, then mark it completed with the live summary markdown.
    SummaryProcessesRepository::create_or_reset_process(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to initialize summary process: {}", e))?;
    SummaryProcessesRepository::update_process_completed(pool, &meeting_id, summary, 0, 0.0)
        .await
        .map_err(|e| format!("Failed to save prefilled summary: {}", e))?;

    log_info!(
        "Prefilled meeting summary from live rolling summary for meeting_id: {}",
        meeting_id
    );
    Ok(serde_json::json!({ "message": "Meeting summary prefilled" }))
}

#[cfg(test)]
mod live_summary_tests {
    use super::*;

    #[test]
    fn tail_chars_keeps_the_most_recent_text() {
        assert_eq!(tail_chars("abcdef", 3), "def");
        assert_eq!(tail_chars("abc", 10), "abc");
        assert_eq!(tail_chars("", 10), "");
    }

    #[test]
    fn tail_chars_splits_on_characters_not_bytes() {
        // Byte slicing here would panic or corrupt the text
        assert_eq!(tail_chars("añ😀bé", 2), "bé");
    }

    #[test]
    fn nothing_new_marker_becomes_empty() {
        assert_eq!(strip_nothing_new("NOTHING_NEW"), "");
        assert_eq!(strip_nothing_new("  NOTHING_NEW  "), "");
        // Models dress the marker up in bullets, quotes, or code fences
        assert_eq!(strip_nothing_new("- NOTHING_NEW"), "");
        assert_eq!(strip_nothing_new("\"NOTHING_NEW\""), "");
        assert_eq!(strip_nothing_new("- **NOTHING_NEW**"), "");
        assert_eq!(strip_nothing_new("nothing_new"), "");
    }

    #[test]
    fn first_chunk_asks_for_a_plain_summary() {
        let (system, user) = build_live_summary_prompt("We kicked off the project.", None);

        assert_eq!(system, LIVE_SUMMARY_SYSTEM_PROMPT);
        assert!(user.contains("We kicked off the project."));
        assert!(
            !user.contains("<existing_summary>"),
            "there is no earlier summary to show"
        );
    }

    #[test]
    fn blank_previous_summary_is_treated_as_the_first_chunk() {
        for previous in [Some(""), Some("   \n  "), None] {
            let (system, user) = build_live_summary_prompt("Opening remarks.", previous);
            assert_eq!(system, LIVE_SUMMARY_SYSTEM_PROMPT);
            assert!(!user.contains("<existing_summary>"));
        }
    }

    #[test]
    fn later_chunks_carry_the_summary_so_far_as_context() {
        let (system, user) =
            build_live_summary_prompt("Then we agreed to ship.", Some("- Kicked off the project"));

        assert_eq!(system, LIVE_SUMMARY_APPEND_SYSTEM_PROMPT);
        assert!(user.contains("<existing_summary>"));
        assert!(user.contains("- Kicked off the project"));
        assert!(user.contains("<new_transcript>"));
        assert!(user.contains("Then we agreed to ship."));
        assert!(system.contains("Do NOT repeat"));
    }

    #[test]
    fn prompt_size_stays_bounded_as_the_meeting_runs() {
        // A long-running meeting: both the excerpt and the summary-so-far exceed
        // their caps. The prompt must not grow with either.
        // Filler characters are ones the prompt template itself never uses.
        let transcript = "Z".repeat(LIVE_SUMMARY_MAX_CHARS * 3);
        let previous = "9".repeat(LIVE_SUMMARY_CONTEXT_MAX_CHARS * 3);

        let (_, user) = build_live_summary_prompt(&transcript, Some(&previous));

        assert_eq!(user.matches('Z').count(), LIVE_SUMMARY_MAX_CHARS);
        assert_eq!(user.matches('9').count(), LIVE_SUMMARY_CONTEXT_MAX_CHARS);
    }

    #[test]
    fn the_most_recent_excerpt_is_kept_when_capping() {
        let transcript = format!("{}TAIL", "Z".repeat(LIVE_SUMMARY_MAX_CHARS));

        let (_, user) = build_live_summary_prompt(&transcript, None);

        assert!(user.contains("TAIL"), "the newest speech must survive capping");
    }

    #[test]
    fn real_bullets_are_preserved() {
        let bullets = "- Agreed to ship on Friday\n- Alice owns the migration";
        assert_eq!(strip_nothing_new(bullets), bullets);

        // A bullet that merely mentions the marker still carries content
        let mentions = "- Discussed the NOTHING_NEW sentinel handling";
        assert_eq!(strip_nothing_new(mentions), mentions);
    }
}
