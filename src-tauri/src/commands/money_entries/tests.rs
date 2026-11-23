use super::super::local_store::tests::temp_dir;
use super::super::money::{
    account_add_in, fx_set_manual_in, import_statement_in, MoneyAccountDraft,
};
use super::*;

fn day(s: &str) -> NaiveDate {
    parse_day(s).unwrap()
}

const TODAY: &str = "2026-11-19";

fn dr(kind: &str, amount: i64, cur: &str, date: &str) -> EntryDraft {
    EntryDraft {
        kind: kind.into(),
        account_id: None,
        amount_minor: amount,
        currency: cur.into(),
        date: date.into(),
        due: None,
        recurrence: None,
        category: None,
        note: None,
        source: None,
    }
}

fn add(d: &Path, e: EntryDraft) -> MoneyEntry {
    entry_add_in(d, &e, day(TODAY)).unwrap()
}

fn view(d: &Path) -> MonthView {
    month_view_in(d, day(TODAY), 3).unwrap()
}

fn account(d: &Path, route: &str, scope: &str, cur: &str) -> String {
    account_add_in(
        d,
        &MoneyAccountDraft {
            nickname: "Main".into(),
            institution: if route == "csv" { "smbc" } else { "chime" }.into(),
            kind: if route == "csv" {
                "ordinary"
            } else {
                "checking"
            }
            .into(),
            route: route.into(),
            currency: cur.into(),
            scope: scope.into(),
        },
    )
    .unwrap()
    .id
}

#[test]
fn append_void_list_keep_history() {
    let d = temp_dir("me-basic");
    let mut e = dr("expense", 1200, "JPY", "2026-11-10");
    e.category = Some("food".into());
    e.note = Some("lunch\nwith  notes".into());
    let a = add(&d, e);
    assert_eq!(a.note.as_deref(), Some("lunch with notes"));
    let b = add(&d, dr("income", 300_000, "JPY", "2026-11-05"));
    let l = entries_list_in(&d, &EntryRange::default()).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert_eq!(l.entries[0].id, b.id, "newest recorded first");
    // Undo = void; the row stays in the file, marked.
    entry_void_in(&d, &a.id, None, day(TODAY)).unwrap();
    let l = entries_list_in(&d, &EntryRange::default()).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert!(l
        .entries
        .iter()
        .find(|e| e.id == a.id)
        .unwrap()
        .voided_by
        .is_some());
    assert_eq!(
        entry_void_in(&d, &a.id, None, day(TODAY)),
        Err("already_voided".into())
    );
    assert_eq!(
        entry_void_in(&d, "nope", None, day(TODAY)),
        Err("not_found".into())
    );
    // Edit = void + new row, atomically.
    let new = entry_void_in(
        &d,
        &b.id,
        Some(&dr("income", 310_000, "JPY", "2026-11-05")),
        day(TODAY),
    )
    .unwrap()
    .unwrap();
    let live: Vec<_> = entries_list_in(&d, &EntryRange::default())
        .unwrap()
        .entries
        .into_iter()
        .filter(|e| e.voided_by.is_none())
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, new.id);
    // A bad replacement voids nothing.
    assert!(entry_void_in(
        &d,
        &new.id,
        Some(&dr("income", 0, "JPY", "2026-11-05")),
        day(TODAY)
    )
    .is_err());
    assert!(entries_list_in(&d, &EntryRange::default())
        .unwrap()
        .entries
        .iter()
        .find(|e| e.id == new.id)
        .unwrap()
        .voided_by
        .is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = fs::metadata(entries_path(&d)).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600);
    }
    let _ = fs::remove_dir_all(d);
}

#[test]
fn validation_refuses_bad_rows() {
    let d = temp_dir("me-valid");
    let t = day(TODAY);
    for bad in [
        dr("transferish", 1, "JPY", "2026-11-01"),
        dr("expense", 0, "JPY", "2026-11-01"),
        dr("expense", -5, "JPY", "2026-11-01"),
        dr("expense", 5, "EUR", "2026-11-01"),
        dr("expense", 5, "JPY", "2026-13-01"),
        dr("expense", 5, "JPY", "2026-11-25"), // future
        dr("bill", 5, "JPY", "2026-11-25"),    // bill needs a due date
        dr("expense", MAX_AMOUNT, "JPY", "2026-11-01"),
    ] {
        assert!(entry_add_in(&d, &bad, t).is_err(), "{bad:?}");
    }
    let mut c = dr("expense", 5, "JPY", "2026-11-01");
    c.category = Some("gambling".into());
    assert!(entry_add_in(&d, &c, t).is_err());
    let mut r = dr("expense", 5, "JPY", "2026-11-01");
    r.recurrence = Some("monthly".into());
    assert!(
        entry_add_in(&d, &r, t).is_err(),
        "only bills and commitments repeat"
    );
    // A balance may be 0; a bill may be due in the future.
    assert!(entry_add_in(&d, &dr("balance", 0, "JPY", "2026-11-01"), t).is_ok());
    let mut b = dr("bill", 5000, "JPY", "2026-12-01");
    b.due = Some("2026-12-01".into());
    assert!(entry_add_in(&d, &b, t).is_ok());
    assert!(!entries_path(&d).with_file_name("x").exists());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn accounts_must_be_personal_and_same_currency() {
    let d = temp_dir("me-acct");
    let venture = account(&d, "manual", "venture", "JPY");
    let personal = account(&d, "manual", "personal", "JPY");
    let t = day(TODAY);
    let mut e = dr("balance", 100, "JPY", "2026-11-18");
    e.account_id = Some(venture);
    assert_eq!(entry_add_in(&d, &e, t), Err("invalid_input".into()));
    e.account_id = Some(personal.clone());
    e.currency = "USD".into();
    assert_eq!(entry_add_in(&d, &e, t), Err("invalid_input".into()));
    e.currency = "JPY".into();
    e.amount_minor = 250_000;
    entry_add_in(&d, &e, t).unwrap();
    // balances.json keeps getting the manual balance (back-compat).
    let b = super::super::money::balances_list_scoped_in(&d, t, Some("personal")).unwrap();
    assert_eq!(b.balances.len(), 1);
    assert_eq!(b.balances[0].amount, 250_000.0);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn rollup_month_math_and_per_currency_totals() {
    let d = temp_dir("me-roll");
    add(&d, dr("balance", 500_000, "JPY", "2026-11-01"));
    add(&d, dr("income", 300_000, "JPY", "2026-11-05"));
    let mut f = dr("expense", 40_000, "JPY", "2026-11-06");
    f.category = Some("food".into());
    add(&d, f);
    let mut g = dr("expense", 10_000, "JPY", "2026-11-07");
    g.category = Some("fun".into());
    add(&d, g);
    add(&d, dr("savings", 50_000, "JPY", "2026-11-08"));
    add(&d, dr("expense", 20_000, "JPY", "2026-10-07"));
    add(&d, dr("expense", 900, "USD", "2026-11-07"));
    let v = view(&d);
    assert_eq!(v.months, vec!["2026-09", "2026-10", "2026-11"]);
    let jpy = &v.currencies[0];
    let nov = jpy.months.last().unwrap();
    assert_eq!(
        (
            nov.income_minor,
            nov.expense_minor,
            nov.savings_minor,
            nov.net_minor
        ),
        (300_000, 50_000, 50_000, 250_000)
    );
    assert_eq!(
        nov.by_category[0],
        CategoryTotal {
            category: "food".into(),
            minor: 40_000
        }
    );
    assert!(
        !jpy.months[0].has_data,
        "an empty month is 'no data', not zero"
    );
    // Available = balance + income - expense - savings after the balance date.
    let a = jpy.available.as_ref().unwrap();
    assert_eq!(a.amount_minor, 500_000 + 300_000 - 50_000 - 50_000);
    // USD is separate, with no balance: no figure.
    let usd = &v.currencies[1];
    assert!(usd.available.is_none());
    assert_eq!(usd.months.last().unwrap().expense_minor, 900);
    // Two currencies, no rate: no combined total.
    assert!(v.needs_conversion);
    assert!(v.combined_usd.is_none());
    fx_set_manual_in(&d, 150.0, "2026-11-18").unwrap();
    let v = view(&d);
    let c = v.combined_usd.unwrap();
    assert!(!v.needs_conversion);
    assert_eq!(
        c.expense_cents,
        900 + (50_000.0f64 / 150.0 * 100.0).round() as i64
    );
    assert_eq!(c.rate_date, "2026-11-18");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn no_balance_means_no_available_figure() {
    let d = temp_dir("me-nobal");
    add(&d, dr("income", 1000, "JPY", "2026-11-05"));
    assert!(view(&d).currencies[0].available.is_none());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn bills_recur_and_commitments_count_monthly() {
    let d = temp_dir("me-bills");
    let mut rent = dr("bill", 80_000, "JPY", "2026-11-01");
    rent.due = Some("2026-10-31".into());
    rent.recurrence = Some("monthly".into());
    rent.note = Some("rent".into());
    add(&d, rent);
    let mut phone = dr("bill", 3000, "JPY", "2026-11-01");
    phone.due = Some("2026-11-25".into());
    add(&d, phone);
    let mut gym = dr("bill", 1000, "JPY", "2026-11-01");
    gym.due = Some("2026-11-20".into());
    gym.recurrence = Some("weekly".into());
    add(&d, gym);
    let mut loan = dr("commitment", 12_000, "JPY", "2026-11-01");
    loan.recurrence = Some("yearly".into());
    loan.due = Some("2027-01-01".into());
    add(&d, loan);
    let c = &view(&d).currencies[0];
    let due: Vec<(&str, &str)> = c
        .upcoming
        .iter()
        .map(|u| (u.due.as_str(), u.label.as_str()))
        .collect();
    // Monthly rent due on the 31st: Nov 30 (clamped), Dec 31 is past the 30 day horizon.
    assert!(due.contains(&("2026-11-30", "rent")));
    assert!(!due.iter().any(|(d, l)| *l == "rent" && *d == "2026-12-31"));
    assert_eq!(
        c.upcoming.iter().filter(|u| u.amount_minor == 1000).count(),
        5,
        "weekly: Nov 20, 27, Dec 4, 11, 18"
    );
    assert!(due.contains(&("2026-11-25", "bill")));
    assert_eq!(c.upcoming[0].due, "2026-11-20");
    // committed monthly: rent 80,000 + gym 1,000*52/12 (4,333) + loan 12,000/12 (1,000); the one-off phone bill is not monthly.
    assert_eq!(c.committed_monthly_minor, 80_000 + 4_333 + 1_000);
    assert_eq!(c.commitments[0].monthly_minor, 1_000);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn occurrence_helpers() {
    assert_eq!(add_months(day("2026-01-31"), 1), day("2026-02-28"));
    assert_eq!(add_months(day("2026-01-31"), 2), day("2026-03-31"));
    assert_eq!(
        occurrences(
            day("2026-11-25"),
            "none",
            day("2026-11-19"),
            day("2026-12-19")
        ),
        vec![day("2026-11-25")]
    );
    assert!(occurrences(
        day("2026-10-01"),
        "none",
        day("2026-11-19"),
        day("2026-12-19")
    )
    .is_empty());
    assert_eq!(
        occurrences(
            day("2026-11-19"),
            "yearly",
            day("2026-11-19"),
            day("2028-11-19")
        )
        .len(),
        3
    );
}

#[test]
fn no_double_count_with_a_statement() {
    let d = temp_dir("me-csv");
    let id = account(&d, "csv", "personal", "JPY");
    let p = d.join("s.csv");
    fs::write(
        &p,
        "日付,取引金額,差引残高,摘要\n2026/11/02,300000,500000,給与\n2026/11/10,-20000,480000,ｽｰﾊﾟｰ\n".as_bytes(),
    )
    .unwrap();
    import_statement_in(&d, &id, &p).unwrap();
    // Already in the statement (date <= last row): left out. Cash (no account) and later dates count.
    let mut dup = dr("expense", 20_000, "JPY", "2026-11-10");
    dup.account_id = Some(id.clone());
    add(&d, dup);
    let mut later = dr("expense", 1_500, "JPY", "2026-11-12");
    later.account_id = Some(id);
    add(&d, later);
    add(&d, dr("expense", 700, "JPY", "2026-11-03"));
    let c = &view(&d).currencies[0];
    let nov = c.months.last().unwrap();
    assert_eq!(nov.statement_income_minor, 300_000);
    assert_eq!(nov.statement_expense_minor, 20_000);
    assert_eq!(nov.manual_expense_minor, 2_200);
    assert_eq!(nov.expense_minor, 22_200);
    assert_eq!(c.excluded_duplicates, 1);
    assert!(c.has_statement);
    assert_eq!(c.statement_through.as_deref(), Some("2026-11-10"));
    // With no balance entry, available falls back to the stored statement balance (480,000 on 11-10) + later entries.
    let a = c.available.as_ref().unwrap();
    assert_eq!(a.base_source, "accounts");
    assert_eq!(a.base_minor, 480_000);
    assert_eq!(a.amount_minor, 480_000 - 1_500);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn savings_goal_progress_from_entries_and_goals_file() {
    let d = temp_dir("me-goal");
    add(&d, dr("savings", 100_000, "JPY", "2026-09-10"));
    add(&d, dr("savings", 50_000, "JPY", "2026-11-10"));
    assert!(view(&d).currencies[0].goal.is_none());
    assert!(savings_goal_set_in(&d, "JPY", 600_000, Some("2026-11-19"), day(TODAY)).is_err());
    savings_goal_set_in(&d, "JPY", 600_000, Some("2027-05-19"), day(TODAY)).unwrap();
    savings_goal_set_in(&d, "JPY", 500_000, Some("2027-05-19"), day(TODAY)).unwrap();
    let g = view(&d).currencies[0].goal.clone().unwrap();
    assert_eq!(
        (g.target_minor, g.saved_minor, g.remaining_minor, g.pct),
        (500_000, 150_000, 350_000, 30.0)
    );
    assert_eq!(g.per_month_needed_minor, Some(350_000 / 6 + 1)); // 181 days = 5.95 months, ceil 6
    let goals: Vec<super::super::goals::Goal> =
        serde_json::from_str(&fs::read_to_string(d.join("goals.json")).unwrap()).unwrap();
    assert_eq!(goals.len(), 1, "replaced, not duplicated");
    let s = &view(&d).currencies[0].savings_series;
    assert_eq!(
        s.iter().map(|p| p.value_minor).collect::<Vec<_>>(),
        vec![100_000, 100_000, 150_000]
    );
    let _ = fs::remove_dir_all(d);
}

#[test]
fn old_files_load_and_garbage_is_skipped_and_counted() {
    let d = temp_dir("me-old");
    // The old stores alone: no entries file, balances.json shape untouched.
    assert!(view(&d).currencies.is_empty());
    assert!(entries_list_in(&d, &EntryRange::default())
        .unwrap()
        .entries
        .is_empty());
    let a = add(&d, dr("income", 1000, "JPY", "2026-11-05"));
    let mut text = fs::read_to_string(entries_path(&d)).unwrap();
    text.push_str("not json\n");
    text.push_str(&format!("{}\n", "x".repeat(MAX_LINE_BYTES + 1)));
    text.push_str("{\"id\":\"zz\",\"kind\":\"expense\",\"amount_minor\":-5,\"currency\":\"JPY\",\"date\":\"2026-11-01\",\"source\":\"app\",\"created_at\":\"x\"}\n");
    text.push_str("{\"void_of\":5}\n");
    text.push_str("{\"id\":\"torn\"");
    fs::write(entries_path(&d), text).unwrap();
    let l = entries_list_in(&d, &EntryRange::default()).unwrap();
    assert_eq!(l.entries.len(), 1);
    assert_eq!(l.skipped_lines, 5);
    // A torn last line does not glue onto the next append.
    add(&d, dr("income", 2000, "JPY", "2026-11-06"));
    let l = entries_list_in(&d, &EntryRange::default()).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert_eq!(l.entries[1].id, a.id);
    assert_eq!(view(&d).skipped_lines, 5);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn size_cap_and_symlink_are_refused() {
    let d = temp_dir("me-size");
    fs::create_dir_all(money_dir(&d)).unwrap();
    fs::write(entries_path(&d), vec![b' '; MAX_FILE_BYTES as usize]).unwrap();
    assert_eq!(
        entry_add_in(&d, &dr("income", 1, "JPY", "2026-11-05"), day(TODAY)),
        Err("store_full".into())
    );
    fs::write(entries_path(&d), vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
    assert!(entries_list_in(&d, &EntryRange::default()).is_err());
    fs::remove_file(entries_path(&d)).unwrap();
    #[cfg(unix)]
    {
        let target = d.join("elsewhere");
        fs::write(&target, b"").unwrap();
        std::os::unix::fs::symlink(&target, entries_path(&d)).unwrap();
        assert!(entries_list_in(&d, &EntryRange::default()).is_err());
        assert!(entry_add_in(&d, &dr("income", 1, "JPY", "2026-11-05"), day(TODAY)).is_err());
    }
    let _ = fs::remove_dir_all(d);
}

#[test]
fn context_section_and_checkin() {
    let d = temp_dir("me-ctx");
    assert!(money_context_section_on(&d, day(TODAY)).is_none());
    assert!(
        money_checkin_due(&d, day(TODAY)),
        "no balance on record: ask"
    );
    let b = add(&d, dr("balance", 612_000, "JPY", "2026-11-14"));
    assert!(!money_checkin_due(&d, day(TODAY)), "5 days old");
    assert!(
        money_checkin_due(&d, day("2026-11-21")),
        "7 days old is due"
    );
    assert!(!money_checkin_due(&d, day("2026-11-20")));
    add(&d, dr("income", 300_000, "JPY", "2026-11-15"));
    let mut bill = dr("bill", 5000, "JPY", "2026-11-19");
    bill.due = Some("2026-11-27".into());
    bill.note = Some("phone".into());
    add(&d, bill);
    let s = money_context_section_on(&d, day(TODAY)).unwrap();
    assert!(s.contains("Available now: 912,000 JPY"), "{s}");
    assert!(s.contains("balance 612,000 JPY on 2026-11-14"));
    assert!(s.contains("Bill due 2026-11-27: phone 5,000 JPY"));
    assert!(!s.contains('¥') && !s.contains('￥'));
    // A voided balance no longer counts as a check-in.
    entry_void_in(&d, &b.id, None, day(TODAY)).unwrap();
    assert!(money_checkin_due(&d, day(TODAY)));
    assert_eq!(fmt_minor(123_450, "USD"), "1,234.50 USD");
    assert_eq!(fmt_minor(-5, "JPY"), "-5 JPY");
    let _ = fs::remove_dir_all(d);
}

// ── source scans ──

fn production_source() -> String {
    let src = include_str!("../money_entries.rs");
    let marker = concat!("#[cfg(test)]", "\nmod tests;");
    src.split(marker).next().unwrap().to_string()
}

#[test]
fn commands_are_not_named_like_a_money_mover_and_stores_stay_separate() {
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
    assert_eq!(names.len(), 5, "{names:?}");
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
    for forbidden in ["reqwest", "business_ledger", "treasury", "venture_ledger"] {
        assert!(
            !src.contains(forbidden),
            "{forbidden} in the personal entries module"
        );
    }
}
