use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{collections::hash_map::DefaultHasher, env, hash::Hasher, time::Duration};

const DEFAULT_REMOTE_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_RESPONSES_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const DEFAULT_SUMMARY_POINTS: usize = 5;
const DEFAULT_COACH_QUESTIONS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiRequestMode {
    LocalOnly,
    RemoteIfConfigured,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiSummary {
    pub provider: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiCoachPack {
    pub provider: String,
    pub questions: Vec<String>,
}

pub fn summarize_text(input: &str, max_points: usize, mode: AiRequestMode) -> AiSummary {
    let capped_points = max_points.clamp(1, 12);
    if let Some(remote) = summarize_text_remote(input, capped_points, mode) {
        return remote;
    }
    summarize_text_local(input, capped_points)
}

pub fn coach_questions(input: &str, question_count: usize, mode: AiRequestMode) -> AiCoachPack {
    let capped_questions = question_count.clamp(1, 12);
    if let Some(remote) = coach_questions_remote(input, capped_questions, mode) {
        return remote;
    }
    coach_questions_local(input, capped_questions)
}

fn summarize_text_remote(input: &str, max_points: usize, mode: AiRequestMode) -> Option<AiSummary> {
    if !remote_enabled(mode) {
        return None;
    }

    let user_prompt = format!(
        "Summarize the journal content in exactly {max_points} concise lines for a retro terminal view. \
Each line must start with a short uppercase label and colon (example: FOCUS: ...). \
No markdown bullets. No preamble. Keep each line under 90 characters.\n\nJOURNAL:\n{}",
        truncate_chars(input, 20_000)
    );
    let remote_text = call_openai_responses(
        "You are an assistant for a nostalgic terminal journal. Output plain text only.",
        &user_prompt,
        420,
    )?;
    let cleaned = clean_multiline_text(&remote_text);
    if cleaned.is_empty() {
        return None;
    }

    Some(AiSummary {
        provider: "openai".to_string(),
        text: cleaned,
    })
}

fn coach_questions_remote(
    input: &str,
    question_count: usize,
    mode: AiRequestMode,
) -> Option<AiCoachPack> {
    if !remote_enabled(mode) {
        return None;
    }

    let user_prompt = format!(
        "Generate exactly {question_count} reflective end-of-day questions for journaling. \
Tone should feel classic, calm, and practical. \
Return one question per line, each prefixed like Q1:, Q2:, and so on. \
No extra commentary.\n\nCONTEXT:\n{}",
        truncate_chars(input, 16_000)
    );
    let remote_text = call_openai_responses(
        "You are a reflective journaling coach. Keep prompts concise and useful.",
        &user_prompt,
        520,
    )?;
    let mut questions = parse_question_lines(&remote_text);
    questions.truncate(question_count);
    if questions.is_empty() {
        return None;
    }

    Some(AiCoachPack {
        provider: "openai".to_string(),
        questions,
    })
}

fn summarize_text_local(input: &str, max_points: usize) -> AiSummary {
    let normalized_lines = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let fallback = "No entry content yet. Start with one sentence about your day.";

    let first = normalized_lines
        .first()
        .cloned()
        .unwrap_or_else(|| fallback.to_string());
    let progress = first_line_matching(
        &normalized_lines,
        &[
            "built",
            "shipped",
            "finished",
            "completed",
            "fixed",
            "wrote",
        ],
    )
    .unwrap_or_else(|| first.clone());
    let blockers = first_line_matching(
        &normalized_lines,
        &["blocked", "stuck", "issue", "problem", "delay", "conflict"],
    )
    .unwrap_or_else(|| "No explicit blockers recorded.".to_string());
    let mood = first_line_matching(
        &normalized_lines,
        &["mood", "felt", "energy", "stress", "calm", "anxious"],
    )
    .unwrap_or_else(|| "Mood signal not explicitly recorded.".to_string());
    let next = first_line_matching(
        &normalized_lines,
        &["tomorrow", "next", "plan", "will", "follow up", "priority"],
    )
    .unwrap_or_else(|| "Next: write one priority for tomorrow.".to_string());

    let mut lines = vec![
        format!("FOCUS: {}", truncate_chars(&first, 84)),
        format!("PROGRESS: {}", truncate_chars(&progress, 81)),
        format!("BLOCKERS: {}", truncate_chars(&blockers, 81)),
        format!("MOOD: {}", truncate_chars(&mood, 86)),
        format!("NEXT: {}", truncate_chars(&next, 86)),
    ];
    lines.truncate(max_points.min(DEFAULT_SUMMARY_POINTS));

    AiSummary {
        provider: "local-heuristic".to_string(),
        text: lines.join("\n"),
    }
}

fn coach_questions_local(input: &str, question_count: usize) -> AiCoachPack {
    let mut question_bank = vec![
        "What mattered most today, and why?".to_string(),
        "What drained your energy faster than expected?".to_string(),
        "What did you avoid that still deserves attention?".to_string(),
        "What are you proud you handled well today?".to_string(),
        "What one thing should tomorrow start with?".to_string(),
        "Where did you feel most clear and focused?".to_string(),
        "What conversation should happen next?".to_string(),
        "What can you simplify before tomorrow begins?".to_string(),
    ];

    let lowered = input.to_ascii_lowercase();
    if lowered.contains("team") || lowered.contains("meeting") {
        question_bank.push("Which team moment changed your direction today?".to_string());
    }
    if lowered.contains("project") || lowered.contains("ship") {
        question_bank.push("What project risk should be reduced first tomorrow?".to_string());
    }
    if lowered.contains("family") {
        question_bank.push("How did family time affect your day overall?".to_string());
    }
    if lowered.contains("health") || lowered.contains("sleep") {
        question_bank.push("What health habit should you protect tomorrow?".to_string());
    }

    let mut hasher = DefaultHasher::new();
    hasher.write(input.as_bytes());
    let seed = hasher.finish() as usize;
    let offset = seed % question_bank.len();

    let mut rotated = question_bank.split_off(offset);
    rotated.extend(question_bank);
    rotated.truncate(
        question_count
            .max(1)
            .min(DEFAULT_COACH_QUESTIONS.max(question_count)),
    );

    AiCoachPack {
        provider: "local-heuristic".to_string(),
        questions: rotated,
    }
}

fn call_openai_responses(
    system_prompt: &str,
    user_prompt: &str,
    max_output_tokens: u32,
) -> Option<String> {
    let api_key = env::var("BSJ_OPENAI_API_KEY")
        .or_else(|_| env::var("OPENAI_API_KEY"))
        .ok()?;
    let endpoint = env::var("BSJ_OPENAI_RESPONSES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RESPONSES_ENDPOINT.to_string());
    let model = env::var("BSJ_AI_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REMOTE_MODEL.to_string());

    let payload = json!({
        "model": model,
        "input": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "max_output_tokens": max_output_tokens,
        "temperature": 0.4
    });

    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .ok()?;
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }

    let payload = response.json::<Value>().ok()?;
    extract_output_text(&payload)
}

fn extract_output_text(payload: &Value) -> Option<String> {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    let mut pieces = Vec::new();
    for item in payload
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for content in item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(text) = content.get("text").and_then(Value::as_str) {
                pieces.push(text.to_string());
                continue;
            }
            if let Some(text) = content.get("output_text").and_then(Value::as_str) {
                pieces.push(text.to_string());
            }
        }
    }

    if pieces.is_empty() {
        None
    } else {
        Some(pieces.join("\n").trim().to_string())
    }
}

fn remote_enabled(mode: AiRequestMode) -> bool {
    matches!(mode, AiRequestMode::RemoteIfConfigured)
        && env_truthy("BSJ_AI_ENABLE_REMOTE")
        && (env::var("BSJ_OPENAI_API_KEY").is_ok() || env::var("OPENAI_API_KEY").is_ok())
}

fn env_truthy(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn first_line_matching(lines: &[String], needles: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        let lowered = line.to_ascii_lowercase();
        needles
            .iter()
            .any(|needle| lowered.contains(needle))
            .then(|| line.clone())
    })
}

fn parse_question_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(strip_question_prefix)
        .filter(|line| !line.is_empty())
        .collect()
}

fn strip_question_prefix(line: &str) -> String {
    let mut value = line.trim().to_string();
    if let Some(stripped) = value
        .strip_prefix("Q1:")
        .or_else(|| value.strip_prefix("Q2:"))
        .or_else(|| value.strip_prefix("Q3:"))
        .or_else(|| value.strip_prefix("Q4:"))
        .or_else(|| value.strip_prefix("Q5:"))
        .or_else(|| value.strip_prefix("Q6:"))
        .or_else(|| value.strip_prefix("Q7:"))
        .or_else(|| value.strip_prefix("Q8:"))
        .or_else(|| value.strip_prefix("Q9:"))
    {
        value = stripped.trim().to_string();
    }
    let normalized = value
        .trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '.' || ch == ')' || ch == '-')
        .trim()
        .to_string();
    if normalized.is_empty() {
        value
    } else {
        normalized
    }
}

fn clean_multiline_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let chars = value.chars().count();
    if chars <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return "...".chars().take(limit).collect();
    }
    let keep = limit - 3;
    let mut output = value.chars().take(keep).collect::<String>();
    output.push_str("...");
    output
}

#[cfg(test)]
mod tests {
    use super::{AiRequestMode, coach_questions, parse_question_lines, summarize_text};

    #[test]
    fn local_summary_returns_labeled_lines() {
        let summary = summarize_text(
            "Shipped sync polish.\nBlocked on flaky network.\nTomorrow: tighten retries.",
            5,
            AiRequestMode::LocalOnly,
        );
        assert_eq!(summary.provider, "local-heuristic");
        assert!(summary.text.contains("FOCUS:"));
        assert!(summary.text.contains("BLOCKERS:"));
        assert!(summary.text.contains("NEXT:"));
    }

    #[test]
    fn local_coach_returns_requested_count() {
        let coach = coach_questions(
            "Team meeting about project launch.",
            4,
            AiRequestMode::LocalOnly,
        );
        assert_eq!(coach.provider, "local-heuristic");
        assert_eq!(coach.questions.len(), 4);
    }

    #[test]
    fn parse_question_lines_strips_prefixes_and_blanks() {
        let parsed =
            parse_question_lines("Q1: What worked?\n2) What changed?\n\n- What tomorrow?\n");
        assert_eq!(
            parsed,
            vec![
                "What worked?".to_string(),
                "What changed?".to_string(),
                "What tomorrow?".to_string()
            ]
        );
    }
}
