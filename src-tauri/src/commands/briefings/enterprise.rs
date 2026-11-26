//! The Enterprise briefing and the business rules (decision 0066). Reads only the business stores:
//! the workspace (`business/workspaces.db`), its stored connector data, and `amara_business.db`.
//! Nothing personal or neural is read here, and business recommendations cite business records only.

use super::super::amara_store::commands::ActivityRow;
use super::super::business_workspace::intelligence::BusinessReport;
use super::super::business_workspace::{biz_ref, Tenant};
use super::rules::{evaluation, ymd, Draft, Ev, CORRELATION_LIMIT};
use super::summaries::{BriefingLine, BriefingSection};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Activity kinds that count as Enterprise work done (not reads or settings).
pub(crate) const EXECUTION_KINDS: &[&str] = &[
    "business.context_changed",
    "business.context_approved",
    "business.memory_changed",
    "business.document_added",
    "business.document_changed",
    "business.proposal_decided",
    "business.action_done",
];

#[derive(Debug, Clone, Default)]
pub(crate) struct Business {
    pub business_id: String,
    pub name: String,
    pub today: NaiveDate,
    /// Approved items in the workspace's "Business objectives" section: (item id, title).
    pub objectives: Vec<(String, String)>,
    pub activity: Vec<ActivityRow>,
    /// (change id, title, status)
    pub site_changes: Vec<(String, String, String)>,
    /// (section label, approved, draft, unverified)
    pub sections: Vec<(String, i64, i64, i64)>,
    /// (connector id, last check outcome, when)
    pub connectors: Vec<(String, Option<String>, Option<String>)>,
    pub website: Option<BusinessReport>,
    pub customers: Option<BusinessReport>,
    pub missing: Vec<String>,
}

pub(crate) fn gather(
    data: &std::path::Path,
    t: &Tenant,
    activity: Result<Vec<ActivityRow>, String>,
    today: NaiveDate,
) -> Business {
    let _ = data;
    let mut b = Business {
        business_id: t.id().as_str().to_string(),
        today,
        ..Default::default()
    };
    match t.business() {
        Ok(x) => b.name = x.name,
        Err(_) => b.missing.push("workspace".into()),
    }
    match t.context_list(Some("objectives")) {
        Ok(items) => {
            b.objectives = items
                .into_iter()
                .filter(|i| i.status == "approved")
                .map(|i| (i.item_id, i.title))
                .collect()
        }
        Err(_) => b.missing.push("business objectives".into()),
    }
    match activity {
        Ok(a) => b.activity = a,
        Err(_) => b.missing.push("business activity".into()),
    }
    match t.site_changes() {
        Ok(v) => {
            b.site_changes = v
                .into_iter()
                .map(|c| (c.change_id, c.title, c.status))
                .collect()
        }
        Err(_) => b.missing.push("website changes".into()),
    }
    match t.section_counts() {
        Ok(v) => {
            b.sections = v
                .into_iter()
                .map(|s| (s.label, s.approved, s.draft, s.unverified))
                .collect()
        }
        Err(_) => b.missing.push("context counts".into()),
    }
    if let Ok(ids) = t.connector_ids() {
        for id in ids {
            let c = t.last_check(&id).ok().flatten();
            b.connectors
                .push((id, c.as_ref().map(|c| c.outcome.clone()), c.map(|c| c.at)));
        }
    }
    b.website = super::super::business_workspace::intelligence::website_report(t).ok();
    b.customers = super::super::business_workspace::intelligence::customer_report(t).ok();
    if b.website.is_none() {
        b.missing.push("website report".into());
    }
    if b.customers.is_none() {
        b.missing.push("customer report".into());
    }
    b
}

fn day_of(a: &ActivityRow) -> Option<NaiveDate> {
    super::inputs::day_jst(&a.occurred_at)
}

fn executions(b: &Business, w: (NaiveDate, NaiveDate)) -> Vec<&ActivityRow> {
    b.activity
        .iter()
        .filter(|a| EXECUTION_KINDS.contains(&a.kind.as_str()))
        .filter(|a| day_of(a).is_some_and(|d| d >= w.0 && d <= w.1))
        .collect()
}

/// Approved objectives against the Enterprise work recorded this week.
pub(crate) fn objectives_vs_execution(b: &Business) -> Option<Draft> {
    if b.objectives.is_empty() {
        return None;
    }
    let cur = (b.today - Duration::days(6), b.today);
    let prev = (b.today - Duration::days(27), b.today - Duration::days(7));
    let now = executions(b, cur);
    let before = executions(b, prev);
    // Only when work was being recorded before and stopped: no history means unknown, not idle.
    if !now.is_empty() || before.is_empty() {
        return None;
    }
    let id = super::super::business_workspace::BusinessId::parse(&b.business_id).ok()?;
    let mut evidence: Vec<Ev> = b
        .objectives
        .iter()
        .map(|(item, _)| Ev {
            record_ref: biz_ref(&id, "context", Some(item)),
            role: "observation",
            quality: "self_report",
            start: cur.0,
            end: cur.1,
        })
        .collect();
    evidence.extend(before.iter().filter_map(|a| {
        day_of(a).map(|d| Ev {
            record_ref: a.subject_ref.clone(),
            role: "calculation",
            quality: "measured",
            start: d,
            end: d,
        })
    }));
    let names: Vec<&str> = b
        .objectives
        .iter()
        .take(3)
        .map(|(_, t)| t.as_str())
        .collect();
    let per_week = (before.len() as f64 / 3.0 * 10.0).round() / 10.0;
    Some(Draft {
        kind: "business_objectives_no_execution",
        domain: "enterprise",
        title: format!("No {} work recorded this week against {} approved objective(s)", b.name, b.objectives.len()),
        observation: format!(
            "{} has {} approved objective(s) ({}). No workspace work was recorded from {} to {}, against {} item(s) in the three weeks before (about {per_week} a week).",
            b.name, b.objectives.len(), names.join("; "), cur.0, cur.1, before.len()
        ),
        interpretation: "Work on the objectives may have paused this week, or it happened outside the workspace and wasn't recorded here.".into(),
        limitations: "Only work recorded in the workspace counts (context, documents, memory and decisions). Work done outside the app isn't seen.".into(),
        action: format!("Pick one objective (“{}”) and record one concrete step for it in the workspace this week.", names.first().copied().unwrap_or("")),
        evaluation: evaluation(
            "business.execution_7d",
            json!({"business_id": b.business_id}),
            Some(0.0),
            1.0,
            "at_least",
            "recorded work items",
            b.today + Duration::days(7),
            7,
            "Workspace work items recorded in the 7 days ending on the measure date.",
        ),
        confidence: 0.45,
        confidence_basis: format!("{} work items in the 3 weeks before, none this week", before.len()),
        period: (prev.0, cur.1),
        evidence,
        target_ref: Some(biz_ref(&id, "business", None)),
        key: format!("{}|{}", b.business_id, ymd(super::inputs::monday_of(b.today))),
        cooldown_days: 7,
    })
}

/// A report insight by id, when it is available.
fn insight<'a>(
    r: Option<&'a BusinessReport>,
    id: &str,
) -> Option<&'a super::super::business_workspace::intelligence::BusinessInsight> {
    r?.insights
        .iter()
        .find(|i| i.id == id && i.status == "available")
}

/// Customer search interest against customer-reported needs. Both must be available; today search
/// data is deferred (0064), so this says unavailable and makes no recommendation.
pub(crate) fn search_vs_needs(b: &Business) -> Result<Option<Draft>, String> {
    let Some(queries) = insight(b.website.as_ref(), "search.queries") else {
        return Err("Search interest is unavailable: Search Console is deferred (decision 0064), so search queries aren't read. Nothing is estimated.".into());
    };
    let Some(needs) = insight(b.customers.as_ref(), "cust.concerns") else {
        return Err(
            "Customer-reported needs are unavailable: no skin-quiz results have been read yet."
                .into(),
        );
    };
    let q: Vec<String> = queries
        .rows
        .iter()
        .map(|r| r.label.to_lowercase())
        .collect();
    let missing: Vec<&str> = needs
        .rows
        .iter()
        .filter(|r| r.share.unwrap_or(0.0) >= 0.2)
        .filter(|r| !q.iter().any(|x| x.contains(&r.label.to_lowercase())))
        .map(|r| r.label.as_str())
        .collect();
    if missing.is_empty() {
        return Ok(None);
    }
    let id = super::super::business_workspace::BusinessId::parse(&b.business_id)?;
    let window = |w: &Option<String>| -> (NaiveDate, NaiveDate) {
        w.as_deref()
            .and_then(|s| s.split_once(" to "))
            .and_then(|(a, z)| Some((super::inputs::day(a)?, super::inputs::day(z)?)))
            .unwrap_or((b.today - Duration::days(27), b.today))
    };
    let (qw, nw) = (window(&queries.window), window(&needs.window));
    Ok(Some(Draft {
        kind: "needs_missing_from_search",
        domain: "enterprise",
        title: format!("{} common quiz concern(s) don't appear in search queries", missing.len()),
        observation: format!(
            "In the skin quiz, {} were each named by at least 20% of results; none of them appears in the top search queries for the site.",
            missing.join(", ")
        ),
        interpretation: format!("Customers report these needs, but people searching don't reach the site with them. {CORRELATION_LIMIT}"),
        limitations: "Quiz takers aren't the same people as searchers, and the top queries are only part of all searches.".into(),
        action: format!("Draft one page or FAQ entry about “{}” and stage it in Website changes.", missing[0]),
        evaluation: evaluation(
            "search.queries_containing",
            json!({"business_id": b.business_id, "term": missing[0]}),
            Some(0.0),
            1.0,
            "at_least",
            "top queries containing it",
            b.today + Duration::days(28),
            28,
            "Top search queries containing the concern, from the next search report.",
        ),
        confidence: 0.35,
        confidence_basis: format!("{} quiz results; the top search queries", needs.n.unwrap_or(0)),
        period: (qw.0.min(nw.0), qw.1.max(nw.1)),
        evidence: vec![
            Ev { record_ref: biz_ref(&id, "insight", Some("search.queries")), role: "observation", quality: "measured", start: qw.0, end: qw.1 },
            Ev { record_ref: biz_ref(&id, "insight", Some("cust.concerns")), role: "observation", quality: "self_report", start: nw.0, end: nw.1 },
        ],
        target_ref: Some(biz_ref(&id, "business", None)),
        key: format!("{}|{}", b.business_id, missing.join(",")),
        cooldown_days: 28,
    }))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseBriefing {
    pub business_id: String,
    pub name: String,
    pub day: String,
    pub built_at: String,
    pub trigger: String,
    pub sections: Vec<BriefingSection>,
    pub open_recommendations: Vec<String>,
    pub missing: Vec<String>,
    pub method: String,
}

fn ln(text: String, kind: &str, sources: Vec<String>) -> BriefingLine {
    BriefingLine {
        text,
        kind: kind.into(),
        sources,
    }
}

fn sec(
    id: &str,
    title: &str,
    lines: Vec<BriefingLine>,
    empty: &str,
    note: Option<String>,
) -> BriefingSection {
    BriefingSection {
        id: id.into(),
        title: title.into(),
        state: if lines.is_empty() { "none" } else { "data" }.into(),
        note: if lines.is_empty() {
            Some(empty.to_string())
        } else {
            note
        },
        lines,
    }
}

pub(crate) fn briefing(
    b: &Business,
    open: &[super::super::amara_store::commands::RecommendationRow],
    search_note: Option<String>,
    built_at: &str,
    trigger: &str,
) -> EnterpriseBriefing {
    let cur = (b.today - Duration::days(6), b.today);
    let work = executions(b, cur);
    let mut by_kind: std::collections::BTreeMap<&str, usize> = Default::default();
    for a in &work {
        *by_kind
            .entry(a.kind.trim_start_matches("business."))
            .or_default() += 1;
    }
    let mut sections = vec![sec(
        "work",
        "Work recorded this week",
        by_kind
            .iter()
            .map(|(k, n)| ln(format!("{n} × {}", k.replace('_', " ")), "measured", vec![]))
            .collect(),
        &format!("No workspace work recorded from {} to {}.", cur.0, cur.1),
        None,
    )];
    sections.push(sec(
        "objectives",
        "Approved objectives",
        b.objectives
            .iter()
            .map(|(_, t)| ln(t.clone(), "self_report", vec![]))
            .collect(),
        "No approved objectives in the workspace yet.",
        None,
    ));
    let waiting: Vec<BriefingLine> = b
        .site_changes
        .iter()
        .filter(|(_, _, s)| {
            matches!(
                s.as_str(),
                "draft" | "approved" | "packaged" | "verify_failed" | "rollback_failed"
            )
        })
        .map(|(id, t, s)| {
            let what = match s.as_str() {
                "draft" => "waiting for your approval",
                "approved" => "approved, not packaged yet",
                "packaged" => "packaged, waiting for your upload",
                "verify_failed" => "verification failed after upload",
                _ => "rollback failed",
            };
            ln(
                format!("{t}: {what}."),
                "measured",
                vec![format!("biz:{}/site_change/{id}", b.business_id)],
            )
        })
        .collect();
    let drafts: i64 = b.sections.iter().map(|s| s.2 + s.3).sum();
    let mut attention = waiting;
    if drafts > 0 {
        attention.push(ln(
            format!("{drafts} context item(s) are draft or unverified."),
            "measured",
            vec![],
        ));
    }
    sections.push(sec(
        "attention",
        "Needs your decision",
        attention,
        "Nothing is waiting for a decision.",
        None,
    ));
    sections.push(sec(
        "connectors",
        "Data sources",
        b.connectors
            .iter()
            .map(|(id, o, at)| {
                ln(
                    match (o.as_deref(), at) {
                        (Some("ok"), Some(at)) => format!("{id}: last check succeeded at {at}."),
                        (Some(o), Some(at)) => format!("{id}: last check {o} at {at}."),
                        _ => {
                            format!("{id}: not checked from here (local folder or never checked).")
                        }
                    },
                    "measured",
                    vec![],
                )
            })
            .collect(),
        "No connectors registered.",
        Some("A source counts as connected only after an authenticated check succeeds.".into()),
    ));
    let mut figures = Vec::new();
    for (r, ids) in [
        (
            b.website.as_ref(),
            ["web.views.total", "web.sessions.total"],
        ),
        (b.customers.as_ref(), ["quiz.total", "cust.concerns"]),
    ] {
        for id in ids {
            if let Some(i) = insight(r, id) {
                figures.push(ln(i.statement.clone(), &i.kind, i.sources.clone()));
            }
        }
    }
    sections.push(sec(
        "figures",
        "Website and customers",
        figures,
        "No website or customer figures have been read yet. Run a sync in Integrations.",
        None,
    ));
    if let Some(n) = search_note {
        sections.push(BriefingSection {
            id: "search_needs".into(),
            title: "Search interest and customer needs".into(),
            state: "unavailable".into(),
            lines: vec![],
            note: Some(n),
        });
    }
    sections.push(sec(
        "suggestions",
        "Amara's open suggestions",
        open.iter()
            .take(5)
            .map(|r| {
                ln(
                    r.title.clone(),
                    "calculated",
                    vec![format!("recommendation:{}", r.recommendation_id)],
                )
            })
            .collect(),
        "No open suggestions for this business.",
        None,
    ));
    EnterpriseBriefing {
        business_id: b.business_id.clone(),
        name: b.name.clone(),
        day: ymd(b.today),
        built_at: built_at.into(),
        trigger: trigger.into(),
        sections,
        open_recommendations: open.iter().map(|r| r.recommendation_id.clone()).collect(),
        missing: b.missing.clone(),
        method: "Fixed rules and arithmetic over this business's records only. No language model was used.".into(),
    }
}
