//! Business ledger (decision 0005): `<data_dir>/business/ledger.json`, read-only here.
//!
//! Amounts are stored in yen (integers). USD is never invented: it exists only when the file holds a
//! manually entered rate with an as-of date. A missing file returns `None` (the UI stays "Needs
//! wiring"); a malformed one is an error, never a partial ledger. Writes come later, behind approval.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_LEDGER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FxRate {
    /// US dollars per 1 yen, entered by hand.
    pub usd_per_jpy: f64,
    /// `YYYY-MM-DD`.
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LedgerFund {
    pub name: String,
    pub balance_jpy: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub label: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub label: String,
    pub due: String,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NextAction {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    OnTrack,
    AtRisk,
    OffTrack,
    Done,
    Waiting,
    Searching,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VentureDetail {
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub next_actions: Vec<NextAction>,
    pub budget_used_jpy: Option<i64>,
    pub fund_jpy: Option<i64>,
    #[serde(default)]
    pub open_votes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LedgerVenture {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub stage: String,
    pub health: Health,
    pub progress: Option<Progress>,
    pub next_step: Option<String>,
    #[serde(default)]
    pub note: String,
    pub target_jpy: Option<i64>,
    pub detail: Option<VentureDetail>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Open,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FundingRequest {
    pub id: String,
    pub requester: String,
    pub purpose: String,
    pub amount_jpy: Option<i64>,
    pub status: RequestStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaxFlag {
    pub id: String,
    pub label: String,
    pub area: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VoteRules {
    pub needed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessLedger {
    pub version: u32,
    pub fx: Option<FxRate>,
    #[serde(default)]
    pub funds: Vec<LedgerFund>,
    #[serde(default)]
    pub ventures: Vec<LedgerVenture>,
    #[serde(default)]
    pub funding_requests: Vec<FundingRequest>,
    #[serde(default)]
    pub tax_flags: Vec<TaxFlag>,
    pub vote_rules: Option<VoteRules>,
    pub revenue_target_jpy: Option<i64>,
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

impl BusinessLedger {
    /// Rejects values that would make the UI show a made-up or misleading figure.
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("ledger: unsupported version {}", self.version));
        }
        if let Some(fx) = &self.fx {
            if !(fx.usd_per_jpy.is_finite() && fx.usd_per_jpy > 0.0 && fx.usd_per_jpy < 1.0) {
                return Err("ledger: fx.usdPerJpy must be a positive dollars-per-yen rate".into());
            }
            if !is_date(&fx.as_of) {
                return Err("ledger: fx.asOf must be YYYY-MM-DD".into());
            }
        }
        if let Some(v) = &self.vote_rules {
            if v.needed == 0 {
                return Err("ledger: voteRules.needed must be at least 1".into());
            }
        }
        let amounts = self
            .ventures
            .iter()
            .flat_map(|v| {
                let d = v.detail.as_ref();
                [
                    v.target_jpy,
                    d.and_then(|d| d.budget_used_jpy),
                    d.and_then(|d| d.fund_jpy),
                ]
            })
            .chain(self.funding_requests.iter().map(|r| r.amount_jpy))
            .chain([self.revenue_target_jpy])
            .flatten();
        if amounts.into_iter().any(|a| a < 0) {
            return Err("ledger: amounts can't be negative (fund balances may be)".into());
        }
        let mut ids: Vec<&str> = self.ventures.iter().map(|v| v.id.as_str()).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err("ledger: duplicate venture id".into());
        }
        Ok(())
    }
}

pub(crate) fn ledger_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("business").join("ledger.json")
}

/// Reads and validates the ledger under `data_dir`. `Ok(None)` when there is no file.
pub(crate) fn read_ledger_in(data_dir: &Path) -> Result<Option<BusinessLedger>, String> {
    let path = ledger_path_in(data_dir);
    match std::fs::symlink_metadata(data_dir.join("business")) {
        Ok(m) if !m.file_type().is_dir() => {
            return Err("ledger: business/ must be a real folder (no symlink)".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => {}
    }
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("ledger: can't read business/ledger.json".into()),
    };
    if !meta.file_type().is_file() {
        return Err("ledger: business/ledger.json must be a regular file (no symlink)".into());
    }
    if meta.len() > MAX_LEDGER_BYTES {
        return Err("ledger: business/ledger.json is over 1 MiB".into());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "ledger: can't read business/ledger.json".to_string())?;
    let ledger: BusinessLedger = serde_json::from_str(&text)
        // Line/column only: the serde message can quote ledger text.
        .map_err(|e| {
            format!(
                "ledger: business/ledger.json is not valid (line {}, column {})",
                e.line(),
                e.column()
            )
        })?;
    ledger.validate()?;
    Ok(Some(ledger))
}

#[tauri::command]
#[specta::specta]
pub async fn read_business_ledger() -> Result<Option<BusinessLedger>, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || read_ledger_in(&dir))
        .await
        .map_err(|_| "ledger: read task failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temp dir, removed on drop (never the real app data).
    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir() -> TempDir {
        let p = std::env::temp_dir().join(format!("nexus-ledger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn write(d: &Path, body: &str) {
        std::fs::create_dir_all(d.join("business")).unwrap();
        std::fs::write(ledger_path_in(d), body).unwrap();
    }

    const OK: &str = r#"{
      "version": 1,
      "fx": { "usdPerJpy": 0.0066, "asOf": "2026-11-11" },
      "funds": [{ "name": "Unallocated", "balanceJpy": 150000 }],
      "ventures": [{ "id": "taiyo-skin", "name": "Taiyo Skin", "stage": "Pre-launch", "health": "at_risk",
                     "progress": { "done": 5, "total": 9, "label": "launch checklist" }, "nextStep": "Label approval",
                     "targetJpy": 3000000, "detail": { "headline": "h", "budgetUsedJpy": 420000, "fundJpy": 600000, "openVotes": 1 } }],
      "fundingRequests": [{ "id": "f1", "requester": "Taiyo Skin", "purpose": "First batch", "amountJpy": 250000, "status": "open" }],
      "taxFlags": [], "voteRules": { "needed": 1 }, "revenueTargetJpy": null
    }"#;

    #[test]
    fn no_file_is_none_not_an_empty_ledger() {
        assert_eq!(read_ledger_in(dir().path()).unwrap(), None);
    }

    #[test]
    fn reads_a_valid_ledger_with_yen_and_a_dated_rate() {
        let d = dir();
        write(d.path(), OK);
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(l.funds[0].balance_jpy, 150000);
        assert_eq!(l.fx.unwrap().as_of, "2026-11-11");
        assert_eq!(l.ventures[0].health, Health::AtRisk);
        assert_eq!(l.vote_rules, Some(VoteRules { needed: 1 }));
    }

    #[test]
    fn rejects_bad_rates_votes_negatives_duplicates_and_versions() {
        for (from, to) in [
            (r#""usdPerJpy": 0.0066"#, r#""usdPerJpy": 150"#),
            (r#""asOf": "2026-11-11""#, r#""asOf": "yesterday""#),
            (r#""needed": 1"#, r#""needed": 0"#),
            (r#""amountJpy": 250000"#, r#""amountJpy": -1"#),
            (r#""version": 1"#, r#""version": 2"#),
            (r#""status": "open""#, r#""status": "maybe""#),
        ] {
            let d = dir();
            write(d.path(), &OK.replace(from, to));
            assert!(read_ledger_in(d.path()).is_err(), "{to} should be rejected");
        }
        let d = dir();
        let dup = OK.replace(
            r#""ventures": [{"#,
            r#""ventures": [{ "id": "taiyo-skin", "name": "x", "health": "done", "progress": null, "nextStep": null, "targetJpy": null, "detail": null }, {"#,
        );
        write(d.path(), &dup);
        assert!(read_ledger_in(d.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_business_folder_and_never_quotes_the_file() {
        let d = dir();
        let elsewhere = d.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("ledger.json"), OK).unwrap();
        std::os::unix::fs::symlink(&elsewhere, d.path().join("business")).unwrap();
        assert!(read_ledger_in(d.path()).is_err());
        let d2 = dir();
        write(
            d2.path(),
            r#"{"version": 1, "funds": [{"name": "SECRET-NOTE", "balanceJpy": "x"}]}"#,
        );
        let err = read_ledger_in(d2.path()).unwrap_err();
        assert!(
            !err.contains("SECRET-NOTE") && err.contains("line"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_ledger() {
        let d = dir();
        let real = d.path().join("elsewhere.json");
        std::fs::write(&real, OK).unwrap();
        std::fs::create_dir_all(d.path().join("business")).unwrap();
        std::os::unix::fs::symlink(&real, ledger_path_in(d.path())).unwrap();
        assert!(read_ledger_in(d.path()).is_err());
    }
}
