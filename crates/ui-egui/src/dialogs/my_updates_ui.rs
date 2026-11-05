//! MypdfCraft's own updates (Help ▸ Check for updates…), in place of the official release check,
//! which would replace this build with the official app.
//!
//! The check asks GitHub's public API for two numbers: commits on the official project that the
//! fork doesn't have yet (the community), and commits on the fork that this build doesn't have yet
//! (the person's own, from another Mac or from Claude). Update opens Terminal on
//! `scripts/update-mypdfcraft.command`, which merges, tests, pushes, rebuilds and restarts.
//! When the last check is older than [`REMIND_AFTER_DAYS`], the dialog opens once at start to say so.

use std::sync::mpsc;

use crate::{PdfCraftApp, widgets};

const FORK: &str = "hermi72/pdfcraft";
const UPSTREAM_OWNER: &str = "storytold";
/// Days without a check before the dialog reminds at start.
pub(crate) const REMIND_AFTER_DAYS: u64 = 14;
const LAST_CHECK_FILE: &str = "last-update-check";
/// Commit titles listed per source.
const MAX_TITLES: usize = 8;

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Report {
    /// Commits on the official project the fork doesn't have.
    community: u64,
    community_titles: Vec<String>,
    /// Commits on the fork this build doesn't have; `None` when this build doesn't know its commit,
    /// or GitHub doesn't know it (a commit never pushed).
    yours: Option<u64>,
    yours_titles: Vec<String>,
}

#[derive(Default)]
pub(crate) struct State {
    /// Set by the desktop app; tests and automation never get the start reminder.
    reminder_enabled: bool,
    reminded: bool,
    /// Days since the last check when the dialog opened as a reminder (`Some(None)`: never checked).
    reminder: Option<Option<u64>>,
    job: Option<mpsc::Receiver<Result<Report, String>>>,
    result: Option<Result<Report, String>>,
    launched: Option<Result<(), String>>,
}

impl State {
    fn start_check(&mut self, ctx: &egui::Context, inline: bool) {
        if self.job.is_some() {
            return;
        }
        self.result = None;
        self.launched = None;
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        let ctx = ctx.clone();
        let work = move || {
            let result = check(pdfcraft_engine::build_info::COMMIT);
            if result.is_ok() {
                save_last_check(now());
            }
            let _ = sender.send(result);
            ctx.request_repaint();
        };
        #[cfg(not(target_arch = "wasm32"))]
        if !inline {
            if let Err(error) = std::thread::Builder::new().name("update-check".into()).spawn(work) {
                self.job = None;
                self.result = Some(Err(error.to_string()));
            }
            return;
        }
        let _ = inline;
        work();
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.try_recv() {
            Ok(result) => {
                self.result = Some(result);
                self.job = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result = Some(Err(tl!("The update check stopped unexpectedly.").into()));
                self.job = None;
            }
        }
    }
}

impl PdfCraftApp {
    /// Help ▸ Check for updates…: open the dialog and check right away.
    pub(crate) fn check_my_updates(&mut self) {
        self.my_updates.reminder = None;
        self.dialog = Some(crate::Dialog::MyUpdates);
        if let Some(ctx) = self.ctx.clone() {
            self.my_updates.start_check(&ctx, self.run_inline);
        }
    }

    /// Let the desktop app remind at start when the last check is old.
    pub fn enable_update_reminder(&mut self) {
        self.my_updates.reminder_enabled = true;
    }

    /// Once per run, with nothing else open: open the dialog when the last check is too old.
    pub(crate) fn remind_about_updates(&mut self) {
        let state = &mut self.my_updates;
        if !state.reminder_enabled || state.reminded || !self.update_check_at_start || self.dialog.is_some() {
            return;
        }
        state.reminded = true;
        let days = load_last_check().map(|at| now().saturating_sub(at) / 86_400);
        if days.is_none_or(|d| d >= REMIND_AFTER_DAYS) {
            state.reminder = Some(days);
            self.dialog = Some(crate::Dialog::MyUpdates);
        }
    }
}

/// Seconds since 1970, by the system clock (0 when it is set before then).
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn load_last_check() -> Option<u64> {
    std::fs::read_to_string(super::claude_ui::settings_file(LAST_CHECK_FILE)?).ok()?.trim().parse().ok()
}

fn save_last_check(at: u64) {
    let Some(path) = super::claude_ui::settings_file(LAST_CHECK_FILE) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, at.to_string()) {
        log::warn!("saving the time of the update check: {e}");
    }
}

/// Commit titles, newest first: the first line of each message, merge commits left out.
fn titles(compare: &serde_json::Value) -> Vec<String> {
    let commits = compare["commits"].as_array().map(Vec::as_slice).unwrap_or_default();
    commits
        .iter()
        .rev()
        .filter_map(|c| c["commit"]["message"].as_str())
        .map(|m| m.lines().next().unwrap_or_default().trim())
        .filter(|t| !t.is_empty() && !t.starts_with("Merge "))
        .take(MAX_TITLES)
        .map(|t| if t.chars().count() > 100 { format!("{}…", t.chars().take(99).collect::<String>()) } else { t.to_string() })
        .collect()
}

/// The report from GitHub's two compare answers (`yours` is `None` when it couldn't be asked).
fn report(community: &serde_json::Value, yours: Option<&serde_json::Value>) -> Result<Report, String> {
    let ahead = |v: &serde_json::Value| v["ahead_by"].as_u64();
    let Some(community_count) = ahead(community) else {
        let message = community["message"].as_str().unwrap_or("unexpected answer");
        return Err(format!("{} ({message})", tl!("GitHub didn't answer the update check.")));
    };
    let yours_count = yours.and_then(ahead);
    Ok(Report {
        community: community_count,
        community_titles: titles(community),
        yours: yours_count,
        yours_titles: yours.filter(|_| yours_count.is_some()).map(titles).unwrap_or_default(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn check(commit: Option<&str>) -> Result<Report, String> {
    use std::time::Duration;

    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    let agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .user_agent("MypdfCraft")
        .build()
        .new_agent();
    let get = |url: String| -> Result<serde_json::Value, String> {
        let response = agent
            .get(&url)
            .header("accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("{} ({e})", tl!("Could not reach GitHub. Check your internet connection.")))?;
        let text = response.into_body().read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    };
    let community = get(format!("https://api.github.com/repos/{FORK}/compare/main...{UPSTREAM_OWNER}:pdfcraft:main"))?;
    // A commit GitHub doesn't know (never pushed) answers "Not Found": unknown, not a failure.
    let yours = match commit.filter(|c| c.chars().all(|ch| ch.is_ascii_hexdigit()) && !c.is_empty()) {
        Some(commit) => Some(get(format!("https://api.github.com/repos/{FORK}/compare/{commit}...main"))?),
        None => None,
    };
    report(&community, yours.as_ref())
}

#[cfg(target_arch = "wasm32")]
fn check(_: Option<&str>) -> Result<Report, String> {
    Err(tl!("Updates are checked in the desktop app only.").into())
}

/// The update script in the project folder (`MYPDFCRAFT_REPO`, else `~/git/pdfcraft`).
fn update_script() -> Option<std::path::PathBuf> {
    let repo = std::env::var_os("MYPDFCRAFT_REPO")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("git/pdfcraft")))?;
    Some(repo.join("scripts/update-mypdfcraft.command")).filter(|p| p.is_file())
}

/// Open Terminal on the update script.
fn launch_update(script: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(script)
            .spawn()
            .map(drop)
            .map_err(|e| format!("{} ({e})", tl!("Could not open Terminal.")))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = script;
        Err(tl!("The Update button works on macOS only.").into())
    }
}

fn list(ui: &mut egui::Ui, titles: &[String], total: u64) {
    for title in titles {
        ui.label(format!("\u{2022} {title}"));
    }
    let shown = u64::try_from(titles.len()).unwrap_or(u64::MAX);
    if total > shown {
        ui.small(crate::i18n::fmt(tl!("…and merges and other changes, {n} in all."), &[("n", &total.to_string())]));
    }
}

/// Draw the dialog. Returns `true` when it should close.
pub(crate) fn body(ui: &mut egui::Ui, app: &mut PdfCraftApp) -> bool {
    ui.heading(tl!("Updates"));
    let inline = app.run_inline;
    let state = &mut app.my_updates;
    state.poll();
    if let Some(days) = state.reminder {
        let text = match days {
            Some(d) => crate::i18n::fmt(tl!("You haven't checked for updates for {d} days."), &[("d", &d.to_string())]),
            None => tl!("MypdfCraft hasn't checked for updates yet.").to_string(),
        };
        ui.label(text);
        ui.add_space(6.0);
    }
    let mut check = false;
    match &state.result {
        None if state.job.is_some() => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tl!("Checking GitHub…"));
            });
        }
        None => {
            check = widgets::pill_button(ui, tl!("Check now"), true).clicked();
        }
        Some(Err(error)) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            check = widgets::pill_button(ui, tl!("Try again"), true).clicked();
        }
        Some(Ok(report)) => {
            ui.strong(crate::i18n::fmt(tl!("Community (official PdfCraft): {n} new"), &[("n", &report.community.to_string())]));
            list(ui, &report.community_titles, report.community);
            ui.add_space(6.0);
            match report.yours {
                Some(n) => {
                    ui.strong(crate::i18n::fmt(tl!("Yours (your fork): {n} new"), &[("n", &n.to_string())]));
                    list(ui, &report.yours_titles, n);
                }
                None => {
                    ui.strong(tl!("Yours (your fork): unknown"));
                    ui.small(tl!("This build doesn't know which commit it was made from. The next update fixes that."));
                }
            }
            ui.add_space(8.0);
            let behind = report.community > 0 || report.yours != Some(0);
            if !behind {
                ui.label(tl!("MypdfCraft is up to date."));
            }
            ui.horizontal(|ui| {
                if behind {
                    match update_script() {
                        Some(script) => {
                            if widgets::pill_button(ui, tl!("Update MypdfCraft"), true).clicked() {
                                state.launched = Some(launch_update(&script));
                            }
                        }
                        None => {
                            ui.label(tl!("Updating needs the project folder ~/git/pdfcraft with its update script."));
                        }
                    }
                }
                check = widgets::pill_button(ui, tl!("Check again"), false).clicked();
            });
            if behind {
                ui.small(tl!("Update opens Terminal: it brings in the changes, runs the tests, uploads to your fork, rebuilds MypdfCraft and restarts it (5–10 minutes). It stops without changing anything if a change conflicts with yours."));
            }
        }
    }
    match &state.launched {
        Some(Ok(())) => {
            ui.label(tl!("Terminal is updating MypdfCraft. Save your work: the app restarts when the update is done."));
        }
        Some(Err(error)) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        None => {}
    }
    ui.add_space(8.0);
    let built = pdfcraft_engine::build_info::COMMIT.unwrap_or("?");
    let last = match load_last_check() {
        Some(at) => crate::i18n::fmt(tl!("last check {d} days ago"), &[("d", &(now().saturating_sub(at) / 86_400).to_string())]),
        None => tl!("never checked").to_string(),
    };
    ui.small(format!("{} {} · {built} · {last}", tl!("Version"), pdfcraft_engine::build_info::version()));
    if check {
        state.reminder = None;
        state.start_check(ui.ctx(), inline);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_report_counts_both_sides_and_lists_newest_titles_without_merges() {
        let community = json!({"ahead_by": 3, "commits": [
            {"commit": {"message": "fix: older\n\nbody"}},
            {"commit": {"message": "Merge pull request #1 from x/y"}},
            {"commit": {"message": "feat: newest"}},
        ]});
        let yours = json!({"ahead_by": 1, "commits": [{"commit": {"message": "Clearer text"}}]});
        let r = report(&community, Some(&yours)).unwrap();
        assert_eq!(r.community, 3);
        assert_eq!(r.community_titles, ["feat: newest", "fix: older"]);
        assert_eq!(r.yours, Some(1));
        assert_eq!(r.yours_titles, ["Clearer text"]);
    }

    #[test]
    fn an_unknown_build_commit_is_unknown_and_an_api_error_is_an_error() {
        let community = json!({"ahead_by": 0, "commits": []});
        let r = report(&community, Some(&json!({"message": "Not Found"}))).unwrap();
        assert_eq!(r.yours, None);
        assert!(r.yours_titles.is_empty());
        assert_eq!(report(&community, None).unwrap().yours, None);
        let error = report(&json!({"message": "API rate limit exceeded"}), None).unwrap_err();
        assert!(error.contains("rate limit"));
    }

    #[test]
    fn long_titles_are_cut_at_a_character_boundary() {
        let long = "é".repeat(150);
        let t = titles(&json!({"commits": [{"commit": {"message": long}}]}));
        assert_eq!(t[0].chars().count(), 100);
        assert!(t[0].ends_with('…'));
    }

    #[test]
    fn the_reminder_only_runs_when_enabled() {
        let mut app = PdfCraftApp::new();
        app.remind_about_updates();
        assert!(app.dialog.is_none(), "tests and automation never get the reminder");
    }
}
