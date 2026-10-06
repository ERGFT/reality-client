#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
mod android_bridge;
mod clipboard;
mod config_json;
mod core;
mod ffi_core;
mod ffi_session;
#[cfg(any(target_os = "android", feature = "android-bridge-check", test))]
mod material;
mod platform;
mod profiles;
mod runtime_stats;
mod security;
mod server_info;
mod ui;
#[cfg(windows)]
mod windows_proxy;

slint::include_modules!();

pub use ui::run_ui;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: slint::android::AndroidApp) {
    platform::initialize_android_data_dir(&app)
        .expect("failed to locate private Android data directory");
    android_bridge::cleanup_stale_profile_secret_if_inactive();
    slint::android::init(app).expect("failed to initialize Slint Android backend");
    run_ui().expect("Reality Client UI failed on Android");
}
