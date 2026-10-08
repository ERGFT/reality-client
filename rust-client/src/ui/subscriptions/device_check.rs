//! Opt-in runtime regression for an isolated emulator APK, never normal builds.
//! Drives production callbacks on the real Android Slint event loop and uses
//! real Android Keystore storage. Does not invoke connection/VpnService APIs.

use super::*;
use crate::profiles::ProfileStore;
use slint::Model;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

const A: &str = "vless://00000000-0000-4000-8000-000000000000@a.example.invalid:443#Server%20A";
const B: &str = "vless://00000000-0000-4000-8000-000000000001@b.example.invalid:443#Server%20B";
const URL: &str = "https://example.invalid/synthetic-subscription";

fn ui<T: Send + 'static>(
    window: &slint::Weak<MainWindow>,
    action: impl FnOnce(MainWindow) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = mpsc::channel();
    let window = window.clone();
    slint::invoke_from_event_loop(move || {
        let result = window
            .upgrade()
            .ok_or("Window closed".to_owned())
            .and_then(action);
        let _ = sender.send(result);
    })
    .map_err(|_| "Event loop unavailable".to_owned())?;
    receiver
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| "UI timed out".to_owned())?
}

fn idle(state: &UiState, window: &slint::Weak<MainWindow>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(30);
    while state.is_starting.load(Ordering::Acquire)
        || ui(window, |window| Ok(window.get_subscription_busy()))?
    {
        if Instant::now() >= until {
            return Err("Storage worker timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn require(condition: bool, error: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(error.into()) }
}

fn run(window: slint::Weak<MainWindow>, state: UiState, root: PathBuf) -> Result<(), String> {
    let test_root = root.join(format!("subscription-check-{}", std::process::id()));
    std::fs::create_dir_all(&test_root).map_err(|_| "Create test directory failed")?;
    let path = test_root.join("profiles.dat");
    let mut store = ProfileStore::open_at(path.clone())?;
    store.save("Manual fixture", A, None)?;
    let manual = std::fs::read(&path).map_err(|_| "Read manual fixture failed")?;
    let initial = crate::subscriptions::parse(format!("{A}\n{B}").as_bytes())?;
    let shared = state.clone();
    ui(&window, move |window| {
        *shared.profile_store.lock().unwrap() = Some(store);
        window.set_active_tab(1);
        window.set_profile_model(model(vec!["Manual fixture".into()]));
        window.set_selected_profile_index(-1);
        shared.is_starting.store(true, Ordering::Release);
        window.set_subscription_busy(true);
        commit_in_background(
            &window,
            shared,
            Action::Add,
            None,
            "Device fixture".into(),
            Some(Ok((Zeroizing::new(URL.into()), initial))),
        );
        Ok(())
    })?;
    idle(&state, &window)?;
    ui(&window, |window| {
        require(
            window.get_profile_model().row_count() == 3,
            "Add model mismatch",
        )?;
        window.set_selected_profile_index(2);
        window.invoke_profile_selected(2);
        Ok(())
    })?;
    // Selection secret retrieval runs in its own background worker.
    let until = Instant::now() + Duration::from_secs(30);
    while !ui(&window, |window| Ok(window.get_vless_link().as_str() == B))? {
        require(
            Instant::now() < until,
            "Android Keystore selection timed out",
        )?;
        std::thread::sleep(Duration::from_millis(20));
    }
    let group = state
        .profile_store
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .subscriptions
        .as_ref()
        .unwrap()
        .groups()[0]
        .id;
    let selected = state
        .profile_store
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .subscription_identity(2);
    let reordered = crate::subscriptions::parse(format!("{B}\n{A}").as_bytes())?;
    let shared = state.clone();
    ui(&window, move |window| {
        shared.is_starting.store(true, Ordering::Release);
        window.set_subscription_busy(true);
        commit_in_background(
            &window,
            shared,
            Action::Refresh,
            Some(group),
            String::new(),
            Some(Ok((Zeroizing::new(URL.into()), reordered))),
        );
        Ok(())
    })?;
    idle(&state, &window)?;
    require(
        state
            .profile_store
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .subscription_identity(1)
            == selected,
        "Stable server identity changed",
    )?;
    ui(&window, |window| {
        require(
            window.get_selected_profile_index() == 1,
            "Selection did not follow reorder",
        )?;
        window.set_subscription_name("Renamed device fixture".into());
        window.invoke_subscription_rename();
        Ok(())
    })?;
    idle(&state, &window)?;
    let reopened = ProfileStore::open_at(path.clone())?;
    require(
        reopened
            .names()
            .iter()
            .any(|name| name.starts_with("Renamed device fixture")),
        "Rename persistence failed",
    )?;
    let subscriptions = reopened
        .subscriptions
        .as_ref()
        .ok_or("Subscription store missing")?;
    require(
        subscriptions.url(group)?.as_str() == URL,
        "Protected URL restart failed",
    )?;
    require(
        subscriptions
            .link(group, selected.ok_or("Selected identity missing")?.1)?
            .as_str()
            == B,
        "Protected server restart failed",
    )?;
    ui(&window, |window| {
        window.invoke_subscription_delete();
        Ok(())
    })?;
    idle(&state, &window)?;
    ui(&window, |window| {
        require(
            window.get_profile_model().row_count() == 1,
            "Delete model mismatch",
        )?;
        require(
            window.get_selected_profile_index() == -1,
            "Deleted selection retained",
        )
    })?;
    require(
        std::fs::read(&path).map_err(|_| "Read manual fixture failed")? == manual,
        "Manual vault changed",
    )?;
    require(
        state.core_session.lock().unwrap().is_none(),
        "Unexpected core session",
    )?;
    Ok(())
}

pub(in crate::ui) fn start(window: &MainWindow, state: &UiState) {
    let Ok(root) = crate::platform::app_data_dir() else {
        return;
    };
    // Guard both feature selection and Android app sandbox identity.
    if !root
        .components()
        .any(|part| part.as_os_str() == "com.ergft.realityclient.subscriptiontest")
    {
        return;
    }
    let window = window.as_weak();
    let state = state.clone();
    std::thread::spawn(move || {
        let result = run(window, state, root.clone());
        let report = match result {
            Ok(()) => serde_json::json!({"status":"PASS", "checks":["real Slint callbacks",
                "add", "select", "refresh reorder", "stable identity", "rename", "Keystore restart",
                "delete", "unchanged manual vault", "no core session"]}),
            Err(error) => serde_json::json!({"status":"FAIL", "error":error}),
        };
        let _ = std::fs::write(
            root.join("subscription-device-check.json"),
            report.to_string(),
        );
    });
}
