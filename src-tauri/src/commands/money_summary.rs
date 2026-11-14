//! Personal money, READ-ONLY summary and take facts (design-1111t §9.4, decision 0008).
//!
//! Two commands, both pure reads over what `money.rs` already stores. Nothing here moves, pays, transfers,
//! trades or converts anything, writes anything, or touches the network.
//! - `money_summary_read`: month spend, usual monthly spend, buffer months, savings rate and recurring debits,
//!   from the imported statement rows of PERSONAL csv accounts.
//! - `money_facts_read`: the only facts a money take may cite. Every fact carries a source ref and an as-of
//!   date; a missing value is absent, never 0. `partial` is true when any personal account is stale or unknown.
//!
//! Rules stated once, here (nothing is shown that these rules do not support):
//! - Amounts stay in the account's own currency. Only JPY statement accounts exist; no USD value is made here.
//! - Spend = debits, income = credits, per calendar month (JST). Transfers between a person's own accounts
//!   cannot be told apart from spending, so month flows, usual spend, buffer and savings rate exist only with
//!   exactly ONE personal csv account.
//! - A month is complete when it is before the current month and the statement reaches its last day or a later
//!   month has rows. Rows dated after today are ignored.
//! - Usual monthly spend = the median of the last 3 complete months, and needs at least 2.
//! - Buffer months = the one account's latest balance (statement or manual, not stale) / usual monthly spend,
//!   only for an ordinary, checking or savings account (a time deposit is not a buffer).
//! - Savings rate = (income - spend) / income for the latest complete month with income. Credits are not
//!   split into salary and transfers; the caller must say so.
//! - Recurring = the same description and the same debit amount in at least 2 distinct months, and nothing at
//!   all while fewer than 2 months have rows.
//! - Stale = a balance older than 35 days (a monthly statement plus a few days' grace).
//! - Venture accounts are never read here (`money::personal_statements_in` and the scoped balance read skip
//!   them before opening a file).

use super::local_store::now_jst;
use super::money::{
    accounts_list_in, balances_list_scoped_in, fx_get_in, personal_statements_in, MoneyAccount,
    StmtAccount,
};
use super::shared::task::{blocking, JoinErr};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// A balance older than this many days is stale (monthly statement + grace).
const STALE_DAYS: i64 = 35;
const MAX_MONTHS: usize = 12;
const MAX_RECURRING: usize = 30;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneyMonthFlow {
    /// `YYYY-MM`
    pub month: String,
    pub income: f64,
    pub spend: f64,
    pub rows: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneyRecurring {
    /// The statement description, as stored on this Mac.
    pub label: String,
    /// A positive debit amount in the account's currency.
    pub amount: f64,
    pub months: u32,
    pub last_date: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneySavingsRate {
    pub month: String,
    pub pct: f64,
    pub prev_month: Option<String>,
    pub prev_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneySummary {
    pub currency: String,
    /// Latest statement row date.
    pub as_of: String,
    pub months_of_data: u32,
    /// True when exactly one personal csv account supplied the rows.
    pub single_account: bool,
    pub account_id: Option<String>,
    /// Empty unless `single_account`.
    pub months: Vec<MoneyMonthFlow>,
    pub current_month: String,
    /// Debits so far this month. `None` unless `single_account` (and some row this month).
    pub current_spend: Option<f64>,
    pub usual_monthly_spend: Option<f64>,
    pub buffer_months: Option<f64>,
    pub savings_rate: Option<MoneySavingsRate>,
    pub recurring: Vec<MoneyRecurring>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneyFact {
    pub key: String,
    pub label: String,
    pub value: f64,
    /// "JPY" | "USD" | "BTC" | "ETH" | "months" | "percent" | "JPY per USD"
    pub unit: String,
    pub as_of: Option<String>,
    /// Where the number came from: `balance:<account>:<statement|manual>`, `statement:<account>`, `fx:<source>`.
    pub source: String,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneyFacts {
    pub day: String,
    pub facts: Vec<MoneyFact>,
    /// True when any personal account is stale or unknown (no balance, bad date) or there is none.
    pub partial: bool,
    /// Fixed codes: `no_accounts`, `stale:<account>`, `unknown:<account>`.
    pub partial_reasons: Vec<String>,
    pub personal_accounts: u32,
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn month_of(d: NaiveDate) -> String {
    format!("{:04}-{:02}", d.year(), d.month())
}

fn month_end(month: &str) -> Option<NaiveDate> {
    let first = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok()?;
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)?
    };
    next.pred_opt()
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Latest balance of one account, as `balances_list_scoped_in` reports it.
pub(crate) struct Bal {
    pub amount: f64,
    pub as_of: String,
    pub source: String,
}

fn is_stale(as_of: &str, today: NaiveDate) -> Option<bool> {
    parse_day(as_of).map(|d| (today - d).num_days() > STALE_DAYS)
}

/// The summary over already-read statements and balances. Pure; `today` is the JST date.
pub(crate) fn summarize(
    accounts: &[StmtAccount],
    balances: &HashMap<String, Bal>,
    today: NaiveDate,
) -> Option<MoneySummary> {
    let jpy: Vec<&StmtAccount> = accounts
        .iter()
        .filter(|a| a.currency == "JPY" && !a.rows.is_empty())
        .collect();
    if jpy.is_empty() {
        return None;
    }
    let single = jpy.len() == 1;
    let today_s = today.format("%Y-%m-%d").to_string();
    let current_month = month_of(today);

    // (month -> (income, spend, rows)) over all accounts; only used when `single`.
    let mut flows: BTreeMap<String, (f64, f64, u32)> = BTreeMap::new();
    let mut months_with_rows: BTreeSet<String> = BTreeSet::new();
    let mut as_of = String::new();
    // (account, description, amount) -> (distinct months, last date)
    let mut rec: HashMap<(String, String, i64), (BTreeSet<String>, String)> = HashMap::new();
    for a in &jpy {
        for r in &a.rows {
            let Some(d) = parse_day(&r.date) else {
                continue;
            };
            if r.date.as_str() > today_s.as_str() {
                continue;
            }
            let m = month_of(d);
            months_with_rows.insert(m.clone());
            if r.date > as_of {
                as_of = r.date.clone();
            }
            let e = flows.entry(m.clone()).or_insert((0.0, 0.0, 0));
            if r.amount >= 0 {
                e.0 += r.amount as f64;
            } else {
                e.1 += (-r.amount) as f64;
            }
            e.2 += 1;
            let desc = r.description.trim();
            if r.amount < 0 && !desc.is_empty() && desc != "[removed]" {
                let slot = rec
                    .entry((a.id.clone(), desc.to_string(), -r.amount))
                    .or_insert_with(|| (BTreeSet::new(), String::new()));
                slot.0.insert(m);
                if r.date > slot.1 {
                    slot.1 = r.date.clone();
                }
            }
        }
    }
    if as_of.is_empty() {
        return None; // every row was unreadable or in the future: nothing real to summarise
    }
    let months_of_data = months_with_rows.len() as u32;

    let mut recurring: Vec<MoneyRecurring> = if months_of_data >= 2 {
        rec.into_iter()
            .filter(|(_, (m, _))| m.len() >= 2)
            .map(|((_, label, amount), (m, last))| MoneyRecurring {
                label,
                amount: amount as f64,
                months: m.len() as u32,
                last_date: last,
            })
            .collect()
    } else {
        Vec::new()
    };
    recurring.sort_by(|a, b| {
        b.amount
            .total_cmp(&a.amount)
            .then_with(|| a.label.cmp(&b.label))
    });
    recurring.truncate(MAX_RECURRING);

    let mut out = MoneySummary {
        currency: "JPY".to_string(),
        as_of: as_of.clone(),
        months_of_data,
        single_account: single,
        account_id: single.then(|| jpy[0].id.clone()),
        months: Vec::new(),
        current_month: current_month.clone(),
        current_spend: None,
        usual_monthly_spend: None,
        buffer_months: None,
        savings_rate: None,
        recurring,
    };
    if !single {
        return Some(out);
    }

    let latest = parse_day(&as_of)?;
    let complete: Vec<(&String, &(f64, f64, u32))> = flows
        .iter()
        .filter(|(m, _)| {
            m.as_str() < current_month.as_str()
                && (flows.keys().any(|k| k > *m) || month_end(m).is_some_and(|end| latest >= end))
        })
        .collect();
    out.months = flows
        .iter()
        .rev()
        .take(MAX_MONTHS)
        .rev()
        .map(|(m, (i, s, n))| MoneyMonthFlow {
            month: m.clone(),
            income: *i,
            spend: *s,
            rows: *n,
        })
        .collect();
    out.current_spend = flows.get(&current_month).map(|f| f.1);

    if complete.len() >= 2 {
        let last3: Vec<f64> = complete.iter().rev().take(3).map(|(_, f)| f.1).collect();
        out.usual_monthly_spend = median(last3).filter(|u| *u > 0.0);
    }
    let pct_of = |f: &(f64, f64, u32)| (f.0 > 0.0).then(|| round1((f.0 - f.1) / f.0 * 100.0));
    if let Some((i, (m, f))) = complete
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, (m, f))| pct_of(f).map(|_| (i, ((*m).clone(), **f))))
    {
        let prev = i.checked_sub(1).map(|p| complete[p]);
        out.savings_rate = pct_of(&f).map(|pct| MoneySavingsRate {
            month: m,
            pct,
            prev_month: prev.map(|(pm, _)| pm.clone()),
            prev_pct: prev.and_then(|(_, pf)| pct_of(pf)),
        });
    }

    // Buffer: the one account's fresh balance over usual spend, for a liquid kind only.
    if let (Some(usual), Some(a)) = (out.usual_monthly_spend, jpy.first()) {
        if matches!(a.kind.as_str(), "ordinary" | "checking" | "savings") {
            if let Some(b) = balances.get(&a.id) {
                if is_stale(&b.as_of, today) == Some(false)
                    && b.amount.is_finite()
                    && b.amount >= 0.0
                {
                    out.buffer_months = Some(round1(b.amount / usual));
                }
            }
        }
    }
    Some(out)
}

fn balance_map(data: &Path, today: NaiveDate) -> Result<HashMap<String, Bal>, String> {
    Ok(balances_list_scoped_in(data, today, Some("personal"))?
        .balances
        .into_iter()
        .map(|b| {
            (
                b.account_id,
                Bal {
                    amount: b.amount,
                    as_of: b.as_of,
                    source: b.source,
                },
            )
        })
        .collect())
}

pub(crate) fn summary_in(data: &Path, today: NaiveDate) -> Result<Option<MoneySummary>, String> {
    let statements = personal_statements_in(data)?;
    let balances = balance_map(data, today)?;
    Ok(summarize(&statements, &balances, today))
}

fn fact(
    key: String,
    label: String,
    value: f64,
    unit: &str,
    as_of: Option<String>,
    source: String,
    stale: bool,
) -> MoneyFact {
    MoneyFact {
        key,
        label,
        value,
        unit: unit.to_string(),
        as_of,
        source,
        stale,
    }
}

/// Facts for a money take. Only personal accounts; every fact has a source ref; nothing is estimated.
pub(crate) fn facts_in(data: &Path, today: NaiveDate) -> Result<MoneyFacts, String> {
    let personal: Vec<MoneyAccount> = accounts_list_in(data)?
        .into_iter()
        .filter(|a| a.scope == "personal")
        .collect();
    let balances = balance_map(data, today)?;
    let mut facts = Vec::new();
    let mut reasons = Vec::new();
    if personal.is_empty() {
        reasons.push("no_accounts".to_string());
    }
    for a in &personal {
        match balances.get(&a.id) {
            None => reasons.push(format!("unknown:{}", a.id)),
            Some(b) => match is_stale(&b.as_of, today) {
                None => reasons.push(format!("unknown:{}", a.id)),
                Some(stale) => {
                    if stale {
                        reasons.push(format!("stale:{}", a.id));
                    }
                    facts.push(fact(
                        format!("balance_{}", a.id),
                        a.nickname.clone(),
                        b.amount,
                        &a.currency,
                        Some(b.as_of.clone()),
                        format!("balance:{}:{}", a.id, b.source),
                        stale,
                    ));
                }
            },
        }
    }
    if let Some(s) = summary_in(data, today)? {
        if let Some(id) = &s.account_id {
            let src = format!("statement:{id}");
            if let Some(v) = s.usual_monthly_spend {
                facts.push(fact(
                    "usual_monthly_spend".into(),
                    "Usual monthly spend".into(),
                    v,
                    &s.currency,
                    Some(s.as_of.clone()),
                    src.clone(),
                    false,
                ));
            }
            if let Some(v) = s.buffer_months {
                facts.push(fact(
                    "buffer_months".into(),
                    "Buffer".into(),
                    v,
                    "months",
                    Some(s.as_of.clone()),
                    src.clone(),
                    false,
                ));
            }
            if let Some(r) = &s.savings_rate {
                facts.push(fact(
                    "savings_rate".into(),
                    format!("Savings rate, {}", r.month),
                    r.pct,
                    "percent",
                    Some(s.as_of.clone()),
                    format!("{src}:{}", r.month),
                    false,
                ));
            }
        }
    }
    if let Some(r) = fx_get_in(data)? {
        facts.push(fact(
            "fx_jpy_per_usd".into(),
            "Yen per dollar".into(),
            r.rate,
            "JPY per USD",
            Some(r.date.clone()),
            format!("fx:{}", r.source),
            is_stale(&r.date, today).unwrap_or(true),
        ));
    }
    Ok(MoneyFacts {
        day: today.format("%Y-%m-%d").to_string(),
        facts,
        partial: !reasons.is_empty(),
        partial_reasons: reasons,
        personal_accounts: personal.len() as u32,
    })
}

fn data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn money_summary_read() -> Result<Option<MoneySummary>, String> {
    blocking(JoinErr::Code("store_error"), || {
        summary_in(&data_dir(), now_jst().date_naive())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_facts_read() -> Result<MoneyFacts, String> {
    blocking(JoinErr::Code("store_error"), || {
        facts_in(&data_dir(), now_jst().date_naive())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::super::money::{
        account_add_in, balance_set_manual_in, fx_set_manual_in, import_statement_in,
        MoneyAccountDraft,
    };
    use super::*;
    use std::fs;

    fn day(s: &str) -> NaiveDate {
        parse_day(s).unwrap()
    }

    fn draft(nick: &str, kind: &str, scope: &str) -> MoneyAccountDraft {
        MoneyAccountDraft {
            nickname: nick.into(),
            institution: "smbc".into(),
            kind: kind.into(),
            route: "csv".into(),
            currency: "JPY".into(),
            scope: scope.into(),
        }
    }

    fn import(d: &Path, id: &str, name: &str, body: &str) {
        let p = d.join(name);
        fs::write(&p, body.as_bytes()).unwrap();
        import_statement_in(d, id, &p).unwrap();
    }

    const HDR: &str = "日付,取引金額,差引残高,摘要\n";

    /// Aug, Sep, Oct complete; Nov in progress (today 2026-11-14).
    fn three_months() -> String {
        format!(
            "{HDR}\
2026/08/05,300000,400000,給与\n2026/08/25,-60000,340000,家賃\n2026/08/26,-1500,338500,ﾈｯﾄﾌﾘｯｸｽ\n\
2026/09/05,300000,638500,給与\n2026/09/25,-60000,578500,家賃\n2026/09/26,-1500,577000,ﾈｯﾄﾌﾘｯｸｽ\n2026/09/28,-20000,557000,ｽｰﾊﾟｰ\n\
2026/10/05,300000,857000,給与\n2026/10/25,-60000,797000,家賃\n2026/10/26,-1500,795500,ﾈｯﾄﾌﾘｯｸｽ\n2026/10/29,-40000,755500,ｽｰﾊﾟｰ\n\
2026/11/05,-10000,745500,ｽｰﾊﾟｰ\n"
        )
    }

    #[test]
    fn summary_from_one_account() {
        let d = temp_dir("msum-one");
        let a = account_add_in(&d, &draft("Bank", "ordinary", "personal")).unwrap();
        import(&d, &a.id, "s.csv", &three_months());
        let s = summary_in(&d, day("2026-11-14")).unwrap().unwrap();
        assert!(s.single_account);
        assert_eq!(s.months_of_data, 4);
        assert_eq!(s.current_spend, Some(10_000.0));
        // spends: Aug 61,500; Sep 81,500; Oct 101,500 -> median 81,500
        assert_eq!(s.usual_monthly_spend, Some(81_500.0));
        // latest balance 745,500 on 2026-11-05 (9 days old) / 81,500 = 9.1
        assert_eq!(s.buffer_months, Some(9.1));
        let r = s.savings_rate.unwrap();
        assert_eq!(r.month, "2026-10");
        assert_eq!(r.pct, 66.2); // (300000-101500)/300000
        assert_eq!(r.prev_month.as_deref(), Some("2026-09"));
        assert_eq!(r.prev_pct, Some(72.8));
        let labels: Vec<&str> = s.recurring.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, vec!["家賃", "ﾈｯﾄﾌﾘｯｸｽ"]);
        assert_eq!(s.recurring[0].months, 3);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn nothing_under_two_months_and_no_zero_stand_ins() {
        let d = temp_dir("msum-short");
        let a = account_add_in(&d, &draft("Bank", "ordinary", "personal")).unwrap();
        import(
            &d,
            &a.id,
            "s.csv",
            &format!("{HDR}2026/10/25,-60000,340000,家賃\n2026/10/26,-1500,338500,ﾈｯﾄﾌﾘｯｸｽ\n"),
        );
        let s = summary_in(&d, day("2026-11-14")).unwrap().unwrap();
        assert_eq!(s.months_of_data, 1);
        assert!(s.recurring.is_empty());
        assert_eq!(s.usual_monthly_spend, None);
        assert_eq!(s.buffer_months, None);
        assert_eq!(s.savings_rate, None);
        assert_eq!(s.current_spend, None); // no row in November: unknown, not 0
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn two_accounts_give_no_flows_and_venture_is_never_read() {
        let d = temp_dir("msum-two");
        let a = account_add_in(&d, &draft("Bank A", "ordinary", "personal")).unwrap();
        let b = account_add_in(&d, &draft("Bank B", "savings", "personal")).unwrap();
        let v = account_add_in(&d, &draft("Shop", "ordinary", "venture")).unwrap();
        import(&d, &a.id, "a.csv", &three_months());
        import(&d, &b.id, "b.csv", &three_months());
        import(
            &d,
            &v.id,
            "v.csv",
            &format!("{HDR}2026/10/01,-999999,1,VENTURE-ONLY\n2026/11/01,-999999,1,VENTURE-ONLY\n"),
        );
        let s = summary_in(&d, day("2026-11-14")).unwrap().unwrap();
        assert!(!s.single_account);
        assert!(s.months.is_empty());
        assert_eq!(s.usual_monthly_spend, None);
        assert_eq!(s.buffer_months, None);
        assert_eq!(s.savings_rate, None);
        assert_eq!(s.current_spend, None);
        assert!(s.recurring.iter().all(|r| r.label != "VENTURE-ONLY"));
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("VENTURE-ONLY") && !json.contains("999999"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_time_deposit_is_not_a_buffer_and_a_stale_balance_gives_none() {
        let d = temp_dir("msum-buf");
        let a = account_add_in(&d, &draft("Term", "time_deposit", "personal")).unwrap();
        import(&d, &a.id, "s.csv", &three_months());
        let s = summary_in(&d, day("2026-11-14")).unwrap().unwrap();
        assert_eq!(s.buffer_months, None);
        let d2 = temp_dir("msum-stale");
        let a2 = account_add_in(&d2, &draft("Bank", "ordinary", "personal")).unwrap();
        import(&d2, &a2.id, "s.csv", &three_months());
        // The latest statement balance is 2026-11-05; 60 days later it is stale.
        let s2 = summary_in(&d2, day("2027-01-04")).unwrap().unwrap();
        assert_eq!(s2.buffer_months, None);
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(d2);
    }

    #[test]
    fn future_rows_are_ignored() {
        let d = temp_dir("msum-future");
        let a = account_add_in(&d, &draft("Bank", "ordinary", "personal")).unwrap();
        import(
            &d,
            &a.id,
            "s.csv",
            &format!("{HDR}2027/01/05,-5000,1,FUTURE\n"),
        );
        assert_eq!(summary_in(&d, day("2026-11-14")).unwrap(), None);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn every_fact_has_a_source_ref_and_values_trace_to_rows() {
        let d = temp_dir("msum-facts");
        let a = account_add_in(&d, &draft("Bank", "ordinary", "personal")).unwrap();
        import(&d, &a.id, "s.csv", &three_months());
        fx_set_manual_in(&d, 150.0, "2026-11-12").unwrap();
        let f = facts_in(&d, day("2026-11-14")).unwrap();
        assert!(!f.partial, "{:?}", f.partial_reasons);
        assert!(!f.facts.is_empty());
        for x in &f.facts {
            assert!(!x.source.is_empty(), "{} has no source", x.key);
            assert!(x.as_of.is_some(), "{} has no date", x.key);
            assert!(x.value.is_finite());
        }
        let by = |k: &str| f.facts.iter().find(|x| x.key == k).unwrap();
        assert_eq!(by(&format!("balance_{}", a.id)).value, 745_500.0);
        assert_eq!(by(&format!("balance_{}", a.id)).unit, "JPY");
        assert_eq!(by("usual_monthly_spend").value, 81_500.0);
        assert_eq!(by("buffer_months").value, 9.1);
        assert_eq!(by("fx_jpy_per_usd").source, "fx:manual");
        // No yen-to-dollar value is invented: only the stored rate is a dollar-related fact.
        assert!(f.facts.iter().all(|x| x.unit != "USD"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn partial_when_any_account_is_stale_or_unknown() {
        let d = temp_dir("msum-partial");
        let a = account_add_in(&d, &draft("Bank", "ordinary", "personal")).unwrap();
        import(&d, &a.id, "s.csv", &three_months());
        let w = account_add_in(
            &d,
            &MoneyAccountDraft {
                nickname: "Wallet".into(),
                institution: "ledger".into(),
                kind: "btc".into(),
                route: "manual".into(),
                currency: "BTC".into(),
                scope: "personal".into(),
            },
        )
        .unwrap();
        // The wallet has no balance: unknown.
        let f = facts_in(&d, day("2026-11-14")).unwrap();
        assert!(f.partial);
        assert_eq!(f.partial_reasons, vec![format!("unknown:{}", w.id)]);
        assert!(f.facts.iter().all(|x| x.key != format!("balance_{}", w.id)));
        // Give it an old manual balance: stale, and the fact is flagged stale.
        balance_set_manual_in(&d, &w.id, 0.5, "2026-08-01").unwrap();
        let f = facts_in(&d, day("2026-11-14")).unwrap();
        assert!(f.partial);
        assert_eq!(f.partial_reasons, vec![format!("stale:{}", w.id)]);
        assert!(
            f.facts
                .iter()
                .find(|x| x.key == format!("balance_{}", w.id))
                .unwrap()
                .stale
        );
        // A fresh balance clears it.
        balance_set_manual_in(&d, &w.id, 0.5, "2026-11-10").unwrap();
        assert!(!facts_in(&d, day("2026-11-14")).unwrap().partial);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn no_accounts_is_partial_with_no_facts_and_venture_accounts_are_not_counted() {
        let d = temp_dir("msum-none");
        let f = facts_in(&d, day("2026-11-14")).unwrap();
        assert!(f.partial && f.facts.is_empty());
        assert_eq!(f.partial_reasons, vec!["no_accounts".to_string()]);
        let v = account_add_in(&d, &draft("Shop", "ordinary", "venture")).unwrap();
        import(&d, &v.id, "v.csv", &three_months());
        let f = facts_in(&d, day("2026-11-14")).unwrap();
        assert_eq!(f.personal_accounts, 0);
        assert!(f.facts.is_empty());
        let _ = fs::remove_dir_all(d);
    }

    fn production_source() -> String {
        let src = include_str!("money_summary.rs");
        let marker = concat!("#[cfg(test)]", "\nmod tests {");
        src.split(marker).next().unwrap().to_string()
    }

    #[test]
    fn exactly_two_read_only_commands_none_named_like_a_money_mover() {
        let src = production_source();
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| !l.trim_start().starts_with("#[")).unwrap();
                names.push(
                    f.split("fn ")
                        .nth(1)
                        .and_then(|r| r.split('(').next())
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
        }
        assert_eq!(names, vec!["money_summary_read", "money_facts_read"]);
        for n in &names {
            for banned in [
                "transfer",
                "pay",
                "trade",
                "convert",
                "withdraw",
                "send",
                "buy",
                "sell",
                "move",
                "wire",
                "remit",
                "swap",
                "sign",
                "broadcast",
                "order",
                "deposit",
                "exchange",
            ] {
                assert!(!n.contains(banned), "command {n} contains {banned}");
            }
        }
        // Read-only: no write helper and no network client in this module.
        for forbidden in [
            "write_json_atomic",
            "write_text_atomic",
            "fs::write",
            "reqwest",
        ] {
            assert!(
                !src.contains(forbidden),
                "{forbidden} in a read-only module"
            );
        }
    }
}
