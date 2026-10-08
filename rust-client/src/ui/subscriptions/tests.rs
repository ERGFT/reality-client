//! Exercise the real Slint model/callback wiring without opening a native
//! window, touching an existing vault, or starting VPN/TUN/system proxy.

use super::*;
use crate::profiles::ProfileStore;
use slint::{
    Model, Rgb8Pixel,
    platform::{
        EventLoopProxy, Platform, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8},
    },
    time::{Duration, Instant},
};

fn connect_loopback_echo(
    state: &UiState,
    root: &std::path::Path,
) -> (TcpStream, std::thread::JoinHandle<()>) {
    let echo = TcpListener::bind("127.0.0.1:0").unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    *state.core_session.lock().unwrap() =
        Some(crate::ffi_session::CoreSession::isolated_direct_test_session(root, port));
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = echo.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut bytes = [0; 256];
        while let Ok(size) = stream.read(&mut bytes) {
            if size == 0 {
                break;
            }
            stream.write_all(&bytes[..size]).unwrap();
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)) {
            Ok(stream) => break stream,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(&[5, 1, 0]).unwrap();
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting).unwrap();
    assert_eq!(greeting, [5, 0]);
    let mut request = vec![5, 1, 0, 1, 127, 0, 0, 1];
    request.extend_from_slice(&echo_port.to_be_bytes());
    stream.write_all(&request).unwrap();
    let mut reply = [0; 4];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(&reply[..3], &[5, 0, 0]);
    let length = match reply[3] {
        1 => 6,
        4 => 18,
        other => panic!("unexpected SOCKS address type {other}"),
    };
    stream.read_exact(&mut vec![0; length]).unwrap();
    (stream, worker)
}

fn roundtrip(stream: &mut TcpStream, text: &[u8]) {
    stream.write_all(text).unwrap();
    let mut received = vec![0; text.len()];
    stream.read_exact(&mut received).unwrap();
    assert_eq!(received, text);
}

type Event = Box<dyn FnOnce() + Send>;
struct QueueProxy(std::sync::mpsc::Sender<Event>);
impl EventLoopProxy for QueueProxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Event) -> Result<(), slint::EventLoopError> {
        self.0
            .send(event)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}
struct RenderPlatform {
    window: Rc<MinimalSoftwareWindow>,
    sender: std::sync::mpsc::Sender<Event>,
}
impl Platform for RenderPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(QueueProxy(self.sender.clone())))
    }
}

fn drain(receiver: &std::sync::mpsc::Receiver<Event>, state: &UiState, window: &MainWindow) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        while let Ok(event) = receiver.try_recv() {
            event();
        }
        let cache_ready = window.get_selected_profile_index() < 0
            || state.selected_profile_link.lock().unwrap().is_some();
        if !window.get_subscription_busy()
            && !state.is_starting.load(Ordering::Acquire)
            && cache_ready
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "subscription UI worker did not finish"
        );
        if let Ok(event) = receiver.recv_timeout(Duration::from_millis(10)) {
            event();
        }
    }
}

fn render(window: &MainWindow, adapter: &MinimalSoftwareWindow, mobile: bool, name: &str) {
    let (width, height, scale) = if mobile {
        (1080, 2400, 2.625)
    } else {
        (1080, 1100, 1.0)
    };
    window.set_mobile_layout(mobile);
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
    adapter.set_size(slint::PhysicalSize::new(width, height));
    window.show().unwrap();
    window.window().request_redraw();
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    assert!(adapter.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    }));
    assert!(
        pixels.windows(2).any(|pair| pair[0] != pair[1]),
        "UI rendered an empty surface"
    );
    if let Some(root) = std::env::var_os("REALITY_UI_PROOF_DIR") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        let bytes: Vec<u8> = pixels
            .iter()
            .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
            .collect();
        image::save_buffer(
            root.join(format!("{name}.png")),
            &bytes,
            width,
            height,
            image::ColorType::Rgb8,
        )
        .unwrap();
    }
}

fn render_scrolled(window: &MainWindow, adapter: &MinimalSoftwareWindow, mobile: bool, name: &str) {
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(if mobile { 250.0 } else { 700.0 }, 400.0),
            delta_x: 0.0,
            delta_y: -1600.0,
        });
    render(window, adapter, mobile, name);
}

#[test]
fn subscription_slint_models_preserve_selection_and_render_desktop_mobile() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let adapter = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(RenderPlatform {
        window: adapter.clone(),
        sender,
    }))
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "subscription-ui-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut store = ProfileStore::open_at(root.join("profiles.dat")).unwrap();
    let manual = "vless://00000000-0000-4000-8000-000000000000@manual.example.org:443";
    store.save("Ручной сервер", manual, None).unwrap();
    let manual_bytes = std::fs::read(root.join("profiles.dat")).unwrap();
    let state = UiState {
        profile_store: Arc::new(Mutex::new(Some(store))),
        selected_profile_link: Arc::new(Mutex::new(None)),
        core_session: Arc::new(Mutex::new(None)),
        public_ip_check_busy: Arc::new(AtomicBool::new(false)),
        core_logs: Arc::new(Mutex::new(VecDeque::new())),
        is_starting: Arc::new(AtomicBool::new(false)),
        selectable_groups: Arc::new(Mutex::new(Vec::new())),
        updating_group_controls: Arc::new(AtomicBool::new(false)),
        close_state: Arc::new(AtomicU8::new(0)),
        theme_path: None,
        android_package_ids: Arc::new(Vec::new()),
    };
    let window = MainWindow::new().unwrap();
    window.set_active_tab(1);
    window.set_profile_model(model(vec!["Ручной сервер".into()]));
    super::super::profiles::install(&window, &state);
    install(&window, &state);
    let a = "vless://00000000-0000-4000-8000-000000000000@a.example.org:443#%D0%A1%D0%B5%D1%80%D0%B2%D0%B5%D1%80%20A";
    let b = "vless://00000000-0000-4000-8000-000000000000@b.example.org:443#Server%20B";
    let url = Zeroizing::new("https://fixture.example.org/sublink/test".to_owned());
    let imported = crate::subscriptions::parse(format!("{a}\n{b}").as_bytes()).unwrap();
    window.set_subscription_busy(true);
    state.is_starting.store(true, Ordering::Release);
    commit_in_background(
        &window,
        state.clone(),
        Action::Add,
        None,
        "Тестовая подписка".into(),
        Some(Ok((url.clone(), imported))),
    );
    drain(&receiver, &state, &window);
    assert_eq!(window.get_profile_model().row_count(), 3);
    assert_eq!(window.get_subscription_model().row_count(), 1);
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
    window.set_selected_profile_index(2);
    window.invoke_profile_selected(2);
    // The profile read is asynchronous, while no subscription operation runs.
    drain(&receiver, &state, &window);
    assert_eq!(window.get_vless_link().as_str(), b);
    let (mut active_stream, echo_worker) = connect_loopback_echo(&state, &root);
    roundtrip(&mut active_stream, b"before subscription update");
    let reordered = crate::subscriptions::parse(format!("{b}\n{a}").as_bytes()).unwrap();
    window.set_connect_button_text("Отключить".into());
    window.set_subscription_busy(true);
    state.is_starting.store(true, Ordering::Release);
    commit_in_background(
        &window,
        state.clone(),
        Action::Refresh,
        Some(group),
        String::new(),
        Some(Ok((url, reordered))),
    );
    drain(&receiver, &state, &window);
    assert_eq!(window.get_selected_profile_index(), 1);
    assert_eq!(window.get_vless_link().as_str(), b);
    assert_eq!(window.get_connect_button_text().as_str(), "Отключить");
    roundtrip(&mut active_stream, b"same TCP connection after refresh");
    assert_eq!(
        std::fs::read(root.join("profiles.dat")).unwrap(),
        manual_bytes
    );
    window.set_subscription_name("Переименованная подписка".into());
    window.invoke_subscription_rename();
    drain(&receiver, &state, &window);
    assert!(
        window
            .get_profile_name()
            .starts_with("Переименованная подписка")
    );
    roundtrip(&mut active_stream, b"same TCP connection after rename");
    render(&window, &adapter, false, "subscriptions-desktop");
    render(&window, &adapter, true, "subscriptions-mobile");
    render_scrolled(&window, &adapter, true, "subscriptions-mobile-controls");
    window.invoke_subscription_delete();
    drain(&receiver, &state, &window);
    assert_eq!(window.get_profile_model().row_count(), 1);
    assert_eq!(window.get_selected_profile_index(), -1);
    assert_eq!(window.get_connect_button_text().as_str(), "Отключить");
    assert_eq!(
        std::fs::read(root.join("profiles.dat")).unwrap(),
        manual_bytes
    );
    roundtrip(
        &mut active_stream,
        b"same TCP connection after subscription delete",
    );
    assert!(state.core_session.lock().unwrap().is_some());
    drop(active_stream);
    echo_worker.join().unwrap();
    state
        .core_session
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .stop()
        .unwrap();
    window.hide().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
