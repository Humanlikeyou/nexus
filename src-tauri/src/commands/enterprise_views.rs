//! 1118a lane B4: read-only Enterprise views that compose what already exists (decisions 0032, 0036).
//!
//! - `finances_view`: the business ledger's funds and dated fx, plus one status row per Treasury source
//!   (stream): last good import, days since, and whether the monitor has flagged it stale.
//! - `venture_tracking_list`: each ledger venture with its Treasury streams, this month's income, the last
//!   30 days of logged attention and per-hour figures.
//!
//! Rules this file keeps (each has a test):
//! - Business only. Nothing here reads the personal money store or its rate file (source pin below, and
//!   the personal code never names this file).
//! - Yen becomes USD only with the ledger's own hand-entered, dated rate; the view always carries that
//!   rate's `asOf` and `stale`. No rate: `usd` is `None`, never a guess. No ledger: `ledger: "missing"`
//!   and empty lists, never a made-up fund or venture.
//! - A missing figure is `None`, never 0.
//! - Read-only: no write, no network, no process. Opens `treasury.db` only when the file already exists, so
//!   a read never creates the store.
//! - Path goals do not carry a `#venture:<id>` tag (a goal has no tags field; only tasks and content
//!   ideas do), so no goal link is returned.

use super::business_ledger::{read_ledger_in, BusinessLedger, Health, Progress};
use super::shared::task::{blocking, JoinErr};
use super::treasury::{
    attention_list_in, db_path, fx_stale, income_month_in, open_db, per_hour_in, store_err, today,
    PerHour, StreamMonth,
};
use super::treasury_monitor::{flags_in, status_in};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "FinancesFx", rename_all = "camelCase")]
pub struct FxView {
    pub usd_per_jpy: f64,
    /// `YYYY-MM-DD`, the day the rate was typed in.
    pub as_of: String,
    pub age_days: Option<i64>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "FinancesFund", rename_all = "camelCase")]
pub struct FundView {
    pub name: String,
    pub balance_jpy: i64,
    /// `None` without a ledger rate.
    pub balance_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "FinancesSource", rename_all = "camelCase")]
pub struct SourceView {
    pub stream_id: String,
    pub venture_id: String,
    pub name: String,
    pub stream_type: String,
    /// `active | paused | closed`.
    pub status: String,
    /// ISO time of the last import that worked; `None` when there has been none.
    pub last_import_at: Option<String>,
    pub days_since_import: Option<i64>,
    /// Imports that worked (ok or partial); not rows, not amounts.
    pub imports: i64,
    /// The Treasury monitor has an unacknowledged-or-not `stale_data` flag on this stream.
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "FinancesView", rename_all = "camelCase")]
pub struct FinancesView {
    /// `ok | missing | invalid`.
    pub ledger: String,
    /// `ok | missing | error`: whether the Treasury store could be read.
    pub treasury: String,
    pub fx: Option<FxView>,
    pub funds: Vec<FundView>,
    pub sources: Vec<SourceView>,
    /// The monitor's `stale | flags | ok | no_data`; `None` when there is no store.
    pub monitor_state: Option<String>,
    pub monitor_as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "VentureTrack", rename_all = "camelCase")]
pub struct VentureTrack {
    pub id: String,
    pub name: String,
    pub stage: String,
    pub health: Health,
    pub progress: Option<Progress>,
    pub next_step: Option<String>,
    pub target_jpy: Option<i64>,
    /// Only with a ledger rate.
    pub target_usd: Option<f64>,
    pub stream_ids: Vec<String>,
    /// This month, per stream and currency. `total_minor` is `None` for a month with no events.
    pub income_month: Vec<StreamMonth>,
    /// Logged minutes over the last 30 days; `None` when nothing was logged.
    pub attention_minutes_30d: Option<i64>,
    pub per_hour_30d: Vec<PerHour>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "VentureTracking", rename_all = "camelCase")]
pub struct VentureTracking {
    /// `ok | missing | invalid`.
    pub ledger: String,
    pub treasury: String,
    pub fx: Option<FxView>,
    pub ventures: Vec<VentureTrack>,
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Yen to dollars with the ledger's rate. No rate: `None`.
fn usd(jpy: i64, fx: Option<&FxView>) -> Option<f64> {
    fx.map(|f| round2(jpy as f64 * f.usd_per_jpy))
}

fn fx_view(ledger: &BusinessLedger, today: NaiveDate) -> Option<FxView> {
    let r = ledger.fx.as_ref()?;
    Some(FxView {
        usd_per_jpy: r.usd_per_jpy,
        as_of: r.as_of.clone(),
        age_days: NaiveDate::parse_from_str(&r.as_of, "%Y-%m-%d")
            .ok()
            .map(|d| (today - d).num_days()),
        stale: fx_stale(r, today),
    })
}

fn read_ledger(data: &Path) -> (&'static str, Option<BusinessLedger>) {
    match read_ledger_in(data) {
        Ok(Some(l)) => ("ok", Some(l)),
        Ok(None) => ("missing", None),
        Err(_) => ("invalid", None),
    }
}

fn treasury_exists(data: &Path) -> bool {
    std::fs::symlink_metadata(db_path(data)).is_ok()
}

fn day_diff(iso: &str, today: NaiveDate) -> Option<i64> {
    let d = NaiveDate::parse_from_str(iso.get(..10)?, "%Y-%m-%d").ok()?;
    Some((today - d).num_days())
}

fn sources_in(
    data: &Path,
    today: NaiveDate,
) -> Result<(Vec<SourceView>, String, Option<String>), String> {
    let conn = open_db(data)?;
    let flags = flags_in(&conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.venture_id, s.name, s.type, s.status,
                    (SELECT MAX(at) FROM imports i WHERE i.stream_id = s.id AND i.status IN ('ok','partial')),
                    (SELECT COUNT(*) FROM imports i WHERE i.stream_id = s.id AND i.status IN ('ok','partial'))
             FROM streams s ORDER BY s.created_at, s.id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, venture_id, name, stream_type, status, last, imports) = row.map_err(store_err)?;
        let stale = flags
            .iter()
            .any(|f| f.kind == "stale_data" && f.stream_id.as_deref() == Some(id.as_str()));
        out.push(SourceView {
            days_since_import: last.as_deref().and_then(|l| day_diff(l, today)),
            last_import_at: last,
            stream_id: id,
            venture_id,
            name,
            stream_type,
            status,
            imports,
            stale,
        });
    }
    let st = status_in(&conn, today)?;
    Ok((out, st.state, st.as_of))
}

pub(crate) fn finances_in(data: &Path, today: NaiveDate) -> FinancesView {
    let (ledger_state, ledger) = read_ledger(data);
    let fx = ledger.as_ref().and_then(|l| fx_view(l, today));
    let funds = ledger
        .as_ref()
        .map(|l| {
            l.funds
                .iter()
                .map(|f| FundView {
                    name: f.name.clone(),
                    balance_jpy: f.balance_jpy,
                    balance_usd: usd(f.balance_jpy, fx.as_ref()),
                })
                .collect()
        })
        .unwrap_or_default();
    let (treasury, sources, monitor_state, monitor_as_of) = if !treasury_exists(data) {
        ("missing", Vec::new(), None, None)
    } else {
        match sources_in(data, today) {
            Ok((s, state, as_of)) => ("ok", s, Some(state), as_of),
            Err(_) => ("error", Vec::new(), None, None),
        }
    };
    FinancesView {
        ledger: ledger_state.into(),
        treasury: treasury.into(),
        fx,
        funds,
        sources,
        monitor_state,
        monitor_as_of,
    }
}

pub(crate) fn tracking_in(data: &Path, today: NaiveDate) -> VentureTracking {
    let (ledger_state, ledger) = read_ledger(data);
    let Some(ledger) = ledger else {
        return VentureTracking {
            ledger: ledger_state.into(),
            treasury: if treasury_exists(data) {
                "ok"
            } else {
                "missing"
            }
            .into(),
            fx: None,
            ventures: Vec::new(),
        };
    };
    let fx = fx_view(&ledger, today);
    let have_db = treasury_exists(data);
    let mut treasury = if have_db { "ok" } else { "missing" };
    let month = format!("{:04}-{:02}", today.year(), today.month());
    let from = (today - Duration::days(29)).format("%Y-%m-%d").to_string();
    let to = today.format("%Y-%m-%d").to_string();
    // One read of each Treasury table for all ventures; a failed read leaves that part empty.
    let streams = if have_db {
        super::treasury::streams_list_in(data).unwrap_or_else(|_| {
            treasury = "error";
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let income = if have_db && treasury == "ok" {
        income_month_in(data, &month, &month).unwrap_or_default()
    } else {
        Vec::new()
    };
    let attention = if have_db && treasury == "ok" {
        attention_list_in(data, None, &from, &to).unwrap_or_default()
    } else {
        Vec::new()
    };
    let per_hour = if have_db && treasury == "ok" {
        per_hour_in(data, &from, &to, ledger.fx.as_ref(), today).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ventures = ledger
        .ventures
        .iter()
        .map(|v| {
            let ids: Vec<String> = streams
                .iter()
                .filter(|s| s.venture_id == v.id)
                .map(|s| s.id.clone())
                .collect();
            let mine = |sid: &str| ids.iter().any(|i| i == sid);
            let minutes: Vec<i64> = attention
                .iter()
                .filter(|a| mine(&a.stream_id))
                .map(|a| a.minutes)
                .collect();
            VentureTrack {
                id: v.id.clone(),
                name: v.name.clone(),
                stage: v.stage.clone(),
                health: v.health,
                progress: v.progress.clone(),
                next_step: v.next_step.clone(),
                target_jpy: v.target_jpy,
                target_usd: v.target_jpy.and_then(|t| usd(t, fx.as_ref())),
                income_month: income
                    .iter()
                    .filter(|m| mine(&m.stream_id))
                    .cloned()
                    .collect(),
                attention_minutes_30d: if minutes.is_empty() {
                    None
                } else {
                    Some(minutes.iter().sum())
                },
                per_hour_30d: per_hour
                    .iter()
                    .filter(|p| mine(&p.stream_id))
                    .cloned()
                    .collect(),
                stream_ids: ids,
            }
        })
        .collect();
    VentureTracking {
        ledger: ledger_state.into(),
        treasury: treasury.into(),
        fx,
        ventures,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn finances_view() -> Result<FinancesView, String> {
    blocking(JoinErr::Code("store_error"), move || {
        Ok(finances_in(&crate::app_paths::data_dir(), today()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn venture_tracking_list() -> Result<VentureTracking, String> {
    blocking(JoinErr::Code("store_error"), move || {
        Ok(tracking_in(&crate::app_paths::data_dir(), today()))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::treasury::test_support as ts;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn write_ledger(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir.join("business")).unwrap();
        std::fs::write(dir.join("business").join("ledger.json"), body).unwrap();
    }

    const LEDGER_FX: &str = r#"{"version":1,"fx":{"usdPerJpy":0.0065,"asOf":"2026-11-15"},
        "funds":[{"name":"Taiyo Skin","balanceJpy":100000},{"name":"Unallocated","balanceJpy":-500}],
        "ventures":[{"id":"taiyo-skin","name":"Taiyo Skin","stage":"Pre-launch","health":"at_risk","progress":null,"nextStep":null,"targetJpy":200000,"detail":null}]}"#;
    const LEDGER_NO_FX: &str = r#"{"version":1,"fx":null,
        "funds":[{"name":"Taiyo Skin","balanceJpy":100000}],
        "ventures":[{"id":"taiyo-skin","name":"Taiyo Skin","stage":"","health":"on_track","progress":null,"nextStep":null,"targetJpy":null,"detail":null}]}"#;

    #[test]
    fn a_missing_ledger_is_empty_not_invented() {
        let dir = ts::temp_dir("ev-missing");
        let f = finances_in(&dir, day("2026-11-18"));
        assert_eq!(
            (f.ledger.as_str(), f.treasury.as_str()),
            ("missing", "missing")
        );
        assert!(f.fx.is_none() && f.funds.is_empty() && f.sources.is_empty());
        assert!(f.monitor_state.is_none());
        let t = tracking_in(&dir, day("2026-11-18"));
        assert_eq!(t.ledger, "missing");
        assert!(t.ventures.is_empty() && t.fx.is_none());
        // A read never creates the Treasury store.
        assert!(!db_path(&dir).exists());
    }

    #[test]
    fn a_malformed_ledger_is_invalid_not_partial() {
        let dir = ts::temp_dir("ev-invalid");
        write_ledger(&dir, "{ nope");
        let f = finances_in(&dir, day("2026-11-18"));
        assert_eq!(f.ledger, "invalid");
        assert!(f.funds.is_empty() && f.fx.is_none());
    }

    #[test]
    fn usd_exists_only_with_the_dated_ledger_rate() {
        let dir = ts::temp_dir("ev-fx");
        write_ledger(&dir, LEDGER_NO_FX);
        let f = finances_in(&dir, day("2026-11-18"));
        assert!(f.fx.is_none());
        assert_eq!(f.funds[0].balance_jpy, 100_000);
        assert_eq!(f.funds[0].balance_usd, None, "no rate: no dollars");

        write_ledger(&dir, LEDGER_FX);
        let f = finances_in(&dir, day("2026-11-18"));
        let fx = f.fx.clone().unwrap();
        assert_eq!(fx.as_of, "2026-11-15");
        assert_eq!(fx.age_days, Some(3));
        assert!(!fx.stale);
        assert_eq!(f.funds[0].balance_usd, Some(650.0));
        // Fund balances may be negative; the conversion keeps the sign.
        assert_eq!(f.funds[1].balance_usd, Some(-3.25));
        // The same rate, eight days later, is flagged stale but still carries its date.
        let late = finances_in(&dir, day("2026-11-23"));
        assert!(late.fx.as_ref().unwrap().stale);
        assert_eq!(late.fx.unwrap().as_of, "2026-11-15");
    }

    #[test]
    fn sources_report_last_import_and_null_when_there_is_none() {
        let dir = ts::temp_dir("ev-sources");
        write_ledger(&dir, LEDGER_FX);
        let s = crate::commands::treasury::stream_add_in(
            &dir,
            "taiyo-skin",
            "Shop",
            "product_sales",
            None,
            None,
        )
        .unwrap();
        let f = finances_in(&dir, day("2026-11-18"));
        assert_eq!(f.treasury, "ok");
        assert_eq!(f.sources.len(), 1);
        let src = &f.sources[0];
        assert_eq!(src.stream_id, s.id);
        assert_eq!(src.last_import_at, None);
        assert_eq!(src.days_since_import, None);
        assert_eq!(src.imports, 0);
        assert_eq!(f.monitor_state.as_deref(), Some("no_data"));
    }

    #[test]
    fn tracking_reads_ventures_with_null_not_zero() {
        let dir = ts::temp_dir("ev-track");
        write_ledger(&dir, LEDGER_FX);
        let t = tracking_in(&dir, day("2026-11-18"));
        assert_eq!(t.ledger, "ok");
        assert_eq!(t.treasury, "missing");
        let v = &t.ventures[0];
        assert_eq!(v.id, "taiyo-skin");
        assert_eq!(v.target_usd, Some(1300.0));
        assert!(v.stream_ids.is_empty() && v.income_month.is_empty() && v.per_hour_30d.is_empty());
        assert_eq!(v.attention_minutes_30d, None);

        crate::commands::treasury::stream_add_in(
            &dir,
            "taiyo-skin",
            "Shop",
            "product_sales",
            None,
            None,
        )
        .unwrap();
        let t = tracking_in(&dir, day("2026-11-18"));
        let v = &t.ventures[0];
        assert_eq!(v.stream_ids.len(), 1);
        assert_eq!(v.attention_minutes_30d, None, "no log is None, not 0");
        assert!(v.income_month.iter().all(|m| m.total_minor.is_none()));
        assert!(v
            .per_hour_30d
            .iter()
            .all(|p| p.per_hour_minor.is_none() && p.reason.is_some()));
        // No rate, no dollar target.
        write_ledger(&dir, LEDGER_NO_FX);
        assert_eq!(
            tracking_in(&dir, day("2026-11-18")).ventures[0].target_usd,
            None
        );
    }

    fn production(src: &str) -> &str {
        &src[..src.find("#[cfg(test)]").unwrap()]
    }

    #[test]
    fn it_never_reads_personal_money_and_personal_code_never_names_it() {
        let src = production(include_str!("enterprise_views.rs"));
        for needle in [
            concat!("money", "::"),
            concat!("money", "_summary"),
            concat!("money", "_import"),
            concat!("money", "/"),
            concat!("fx", ".json"),
            "weekly_review",
            "reqwest",
            "TcpStream",
            "Command::new",
            "std::fs::write",
            "File::create",
        ] {
            assert!(!src.contains(needle), "production source contains {needle}");
        }
        for (name, other) in [
            ("money.rs", include_str!("money.rs")),
            ("money_summary.rs", include_str!("money_summary.rs")),
            ("money_import_pick.rs", include_str!("money_import_pick.rs")),
            ("weekly_review.rs", include_str!("weekly_review.rs")),
        ] {
            let p = production(other);
            assert!(
                !p.contains("enterprise_views"),
                "{name} names enterprise_views"
            );
        }
    }

    #[test]
    fn command_names_carry_no_money_moving_word() {
        let src = production(include_str!("enterprise_views.rs"));
        for name in ["finances_view", "venture_tracking_list"] {
            assert!(src.contains(&format!("pub async fn {name}")));
            for w in ts::BANNED {
                assert!(!name.contains(w), "{name} contains {w}");
            }
        }
    }
}
