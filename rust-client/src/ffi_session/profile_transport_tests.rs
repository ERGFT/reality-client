//! Real core + local VLESS wire fixture. No OS proxy, TUN or public endpoint.
use super::*;
use std::io::Read;
use std::net::TcpListener;

fn read_header(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(bytes.len() < 8192);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    bytes
}

fn server(marker: &'static str) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "no VLESS connection arrived");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = [0; 18];
            stream.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 0); // VLESS version
            assert_eq!(
                &header[1..17],
                &[0, 0, 0, 0, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 0]
            );
            let mut addons = vec![0; header[17] as usize];
            stream.read_exact(&mut addons).unwrap();
            let mut target = [0; 4];
            stream.read_exact(&mut target).unwrap();
            assert_eq!(target, [1, 1, 187, 2]); // TCP, 443, domain
            let mut length = [0];
            stream.read_exact(&mut length).unwrap();
            let mut domain = vec![0; length[0] as usize];
            stream.read_exact(&mut domain).unwrap();
            assert_eq!(domain, b"exit-check.invalid");
            // The .invalid destination cannot be reached by a direct fallback.
            stream.write_all(&[0, 0]).unwrap();
            let request = read_header(&mut stream);
            assert!(request.starts_with(b"GET / HTTP/1.1"));
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{marker}",
                marker.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (port, worker)
}

fn request(port: u16, http: bool) -> String {
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    if http {
        stream
            .write_all(
                b"CONNECT exit-check.invalid:443 HTTP/1.1\r\nHost: exit-check.invalid:443\r\n\r\n",
            )
            .unwrap();
        assert!(read_header(&mut stream).starts_with(b"HTTP/1.1 200"));
    } else {
        stream.write_all(&[5, 1, 0]).unwrap();
        let mut greeting = [0; 2];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 0]);
        let domain = b"exit-check.invalid";
        let mut bytes = vec![5, 1, 0, 3, domain.len() as u8];
        bytes.extend_from_slice(domain);
        bytes.extend_from_slice(&443u16.to_be_bytes());
        stream.write_all(&bytes).unwrap();
        let mut reply = [0; 4];
        stream.read_exact(&mut reply).unwrap();
        assert_eq!(&reply[..3], &[5, 0, 0]);
        let tail = match reply[3] {
            1 => 6,
            4 => 18,
            other => panic!("{other}"),
        };
        stream.read_exact(&mut vec![0; tail]).unwrap();
    }
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: exit-check.invalid\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn selected_profile_editor_snapshot_routes_http_and_socks_through_vless() {
    let root = std::env::temp_dir().join(format!(
        "selected-profile-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("config.json");
    fs::write(
        &path,
        r#"{"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}"#,
    )
    .unwrap();
    let mut store = crate::profiles::ProfileStore::open_at(root.join("profiles.dat")).unwrap();
    for marker in ["SELECTED-A", "SELECTED-B"] {
        let (server_port, worker) = server(marker);
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let uri = format!(
            "vless://00000000-0000-4000-8000-000000000000@127.0.0.1:{server_port}?encryption=none&security=none&type=tcp"
        );
        let index = store.save(marker, &uri, None).unwrap();
        let selected = store.read_link(index).unwrap();
        let mut editor: serde_json::Value = serde_json::from_str(&profile_config()).unwrap();
        editor["inbounds"][0]["listen_port"] = serde_json::json!(port);
        // Only this synthetic loopback fixture uses plaintext VLESS.
        editor["outbounds"][0]["allow_insecure"] = serde_json::json!(true);
        let editor = serde_json::to_string(&editor).unwrap();
        let secret_path = root.join("active-server.txt");
        let mut session = CoreSession::start_editor_with_secret(
            &path,
            &editor,
            Some(&selected),
            Some(secret_path.clone()),
            &Arc::new(Mutex::new(VecDeque::new())),
        )
        .unwrap();
        assert_eq!(
            session.diagnostic_proxy_uri(),
            Some(format!("socks5h://127.0.0.1:{port}"))
        );
        assert!(request(port, true).ends_with(marker));
        assert!(request(port, false).ends_with(marker));
        worker.join().unwrap();
        session.stop().unwrap();
        assert!(!secret_path.exists());
        assert!(fs::read_to_string(&path).unwrap().contains("direct"));
    }
    fs::remove_dir_all(root).unwrap();
}

/// Explicit developer diagnostic. Reads a caller-supplied encrypted vault copy;
/// never selects real credentials during CI and never alters OS network state.
#[test]
#[ignore = "requires an explicitly supplied local vault; sends an IP-check request"]
fn probe_explicit_local_profile_vault_without_system_proxy() {
    let vault = PathBuf::from(
        std::env::var_os("REALITY_LOCAL_PROFILE_VAULT").expect("supply encrypted vault copy"),
    );
    let root = vault.parent().unwrap();
    crate::platform::ensure_private_dir(root).unwrap();
    let store =
        crate::profiles::ProfileStore::open_at(vault.clone()).expect("vault could not be opened");
    assert!(store.len() > 0, "no local profiles");
    let baseline = crate::server_info::fetch_public_ip_direct().ok();
    let mut successful = 0;
    for index in 0..store.len().min(10) {
        let selected = store
            .read_link(index)
            .expect("profile could not be decrypted");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut editor: serde_json::Value = serde_json::from_str(&profile_config()).unwrap();
        editor["inbounds"][0]["listen_port"] = serde_json::json!(port);
        let editor = serde_json::to_string(&editor).unwrap();
        let session = CoreSession::start_editor_with_secret(
            &root.join("runtime.json"),
            &editor,
            Some(&selected),
            Some(root.join("active-server.txt")),
            &Arc::new(Mutex::new(VecDeque::new())),
        );
        match session {
            Ok(mut session) => {
                let result = crate::server_info::fetch_public_ip_via_proxy(&format!(
                    "socks5h://127.0.0.1:{port}"
                ));
                match result {
                    Ok(address) => {
                        successful += 1;
                        println!(
                            "PROFILE_{}=REQUEST_OK; EXIT_{}",
                            index + 1,
                            if baseline.as_ref() == Some(&address) {
                                "SAME_AS_OS_ROUTE"
                            } else {
                                "DIFFERS_FROM_OS_ROUTE"
                            }
                        );
                    }
                    Err(_) => println!("PROFILE_{}=UPSTREAM_REQUEST_FAILED", index + 1),
                }
                session.stop().unwrap();
            }
            Err(_) => println!("PROFILE_{}=CORE_START_FAILED", index + 1),
        }
    }
    assert!(
        successful > 0,
        "no selected profile completed the HTTPS request"
    );
}
