//! The skills.sh search + audit API clients (upstream `find.ts` +
//! `telemetry.ts`).

use std::time::Duration;

use skills_store::error::SkillsError;
use skills_store::sanitize::sanitize_metadata;

const SEARCH_API_BASE_DEFAULT: &str = "https://skills.sh";
const AUDIT_URL: &str = "https://www.skills.sh/tele/audit";

fn search_api_base() -> String {
    std::env::var("SKILLS_API_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| SEARCH_API_BASE_DEFAULT.to_string())
}

fn telemetry_enabled() -> bool {
    std::env::var_os("DISABLE_TELEMETRY").is_none()
        && std::env::var_os("DO_NOT_TRACK").is_none()
}

/// One search result.
#[derive(Debug, Clone)]
pub struct SearchSkill {
    /// Skill name.
    pub name: String,
    /// Page slug (e.g. `vercel-labs/agent-skills`).
    pub slug: String,
    /// Source identifier (`owner/repo`).
    pub source: String,
    /// Install count.
    pub installs: u64,
}

/// Format an install count the way `find` renders it.
#[must_use]
pub fn format_installs(count: u64) -> String {
    if count == 0 {
        return String::new();
    }
    if count >= 1_000_000 {
        let value = count as f64 / 1_000_000.0;
        let mut text = format!("{value:.1}");
        if text.ends_with(".0") {
            text.truncate(text.len() - 2);
        }
        return format!("{text}M installs");
    }
    if count >= 1_000 {
        let value = count as f64 / 1_000.0;
        let mut text = format!("{value:.1}");
        if text.ends_with(".0") {
            text.truncate(text.len() - 2);
        }
        return format!("{text}K installs");
    }
    if count == 1 {
        "1 install".to_string()
    } else {
        format!("{count} installs")
    }
}

/// Search the skills.sh registry (upstream `searchSkillsAPI`).
#[must_use]
pub fn search_skills_api(query: &str, owner: Option<&str>) -> Vec<SearchSkill> {
    let mut url = format!("{}/api/search?q={}", search_api_base(), url_encode(query));
    if let Some(owner) = owner {
        url.push_str(&format!("&owner={}", url_encode(owner)));
    }
    let Ok(response) = crate::http::get(&url, Duration::from_secs(10), &[]) else {
        return Vec::new();
    };
    if !(200..300).contains(&response.status) {
        return Vec::new();
    }
    let Ok(value) = response.json() else { return Vec::new() };
    value
        .get("skills")
        .and_then(|skills| skills.as_array())
        .map(|skills| {
            skills
                .iter()
                .filter_map(|skill| {
                    Some(SearchSkill {
                        name: sanitize_metadata(skill.get("name")?.as_str()?),
                        slug: sanitize_metadata(skill.get("id")?.as_str()?),
                        source: sanitize_metadata(skill.get("source").and_then(|s| s.as_str()).unwrap_or("")),
                        installs: skill.get("installs").and_then(|i| i.as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One partner audit result.
#[allow(dead_code)] // fields mirror the skills.sh API for future use
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PartnerAudit {
    /// `safe` | `low` | `medium` | `high` | `critical` | `unknown`.
    pub risk: String,
    /// Alert count (Socket).
    #[serde(default)]
    pub alerts: Option<u64>,
    /// Optional score.
    #[serde(default)]
    pub score: Option<f64>,
    /// ISO timestamp of the analysis.
    #[serde(rename = "analyzedAt", default)]
    pub analyzed_at: Option<String>,
}

/// Audit data per skill: partner name → audit.
#[allow(dead_code)]
pub type SkillAuditData = std::collections::BTreeMap<String, PartnerAudit>;
/// Full audit response: skill slug → partners.
#[allow(dead_code)]
pub type AuditResponse = std::collections::BTreeMap<String, SkillAuditData>;

/// Fetch partner audit data (upstream `fetchAuditData`); `None` on failure.
#[must_use]
pub fn fetch_audit_data(source: &str, skill_slugs: &[String]) -> Option<AuditResponse> {
    if !telemetry_enabled() || skill_slugs.is_empty() {
        return None;
    }
    let url = format!(
        "{AUDIT_URL}?source={}&skills={}",
        url_encode(source),
        url_encode(&skill_slugs.join(","))
    );
    let response = crate::http::get(&url, Duration::from_secs(3), &[]).ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    response.json().ok()?.as_object().map(|map| {
        map.iter()
            .filter_map(|(skill, partners)| {
                let partners = partners.as_object()?;
                let mut out = SkillAuditData::new();
                for (partner, audit) in partners {
                    if let Ok(parsed) = serde_json::from_value::<PartnerAudit>(audit.clone()) {
                        out.insert(partner.clone(), parsed);
                    }
                }
                Some((skill.clone(), out))
            })
            .collect()
    })
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(byte as char);
        } else if byte == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Error helper for search failures surfaced by `find`.
#[allow(dead_code)]
#[must_use]
pub fn search_failed_error() -> SkillsError {
    SkillsError::new("SKILLS_SEARCH_FAILED", "skills.sh search is unavailable")
}
