#![cfg(target_os = "windows")]

use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct AdmissionChild {
    child: Child,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl AdmissionChild {
    fn logs(&self) -> String {
        let stdout = fs::read_to_string(&self.stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&self.stderr_path).unwrap_or_default();
        format!("stdout:\n{stdout}\nstderr:\n{stderr}")
    }

    fn close(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let pid = self.child.id();
        let _ = hidden_command("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($p) {{ $null=$p.CloseMainWindow() }}"
                ),
            ])
            .status();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for AdmissionChild {
    fn drop(&mut self) {
        self.close();
    }
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("iced package is inside the workspace crates directory")
        .to_path_buf()
}

fn target_dir(workspace: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        Some(path) => workspace.join(path),
        None => workspace.join("target"),
    }
}

fn build_and_launch_admission() -> AdmissionChild {
    let workspace = workspace_root();
    let target = target_dir(&workspace);
    let executable = target.join("debug").join("slipway-example-admission.exe");
    if !executable.is_file() {
        let build = hidden_command(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["build", "-p", "slipway-example-admission", "--locked"])
            .current_dir(&workspace)
            .status()
            .expect("launch cargo build for the real admission executable");
        assert!(build.success(), "real admission executable must build");
    }
    let stdout_path = target.join("step223-live-iced.stdout.log");
    let stderr_path = target.join("step223-live-iced.stderr.log");
    let stdout = File::create(&stdout_path).expect("create preserved iced live stdout log");
    let stderr = File::create(&stderr_path).expect("create preserved iced live stderr log");
    let child = hidden_command(&executable)
        .arg("--iced")
        .current_dir(workspace)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("launch the real visible iced admission route");
    AdmissionChild {
        child,
        stdout_path,
        stderr_path,
    }
}

fn listener_port(pid: u32) -> Option<u16> {
    let output = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Get-NetTCPConnection -State Listen -OwningProcess {pid} -ErrorAction SilentlyContinue | Where-Object {{ $_.LocalAddress -eq '127.0.0.1' }} | Select-Object -First 1 -ExpandProperty LocalPort"
            ),
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn wait_for_listener(child: &mut AdmissionChild) -> Option<u16> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if child.child.try_wait().ok().flatten().is_some() {
            return None;
        }
        if let Some(port) = listener_port(child.child.id()) {
            return Some(port);
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

fn call_tool(
    reader: &mut BufReader<TcpStream>,
    writer: &mut TcpStream,
    id: &str,
    name: &str,
    arguments: Value,
) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    serde_json::to_writer(&mut *writer, &request).expect("serialize live MCP request");
    writer.write_all(b"\n").expect("write live MCP delimiter");
    writer.flush().expect("flush live MCP request");

    let mut line = String::new();
    reader.read_line(&mut line).expect("read live MCP response");
    let outer: Value = serde_json::from_str(&line).expect("parse outer MCP response");
    let text = outer["result"]["content"][0]["text"]
        .as_str()
        .expect("MCP response contains a text product");
    serde_json::from_str(text).expect("parse inner Slipway product")
}

fn assert_png_matches_product(product: &Value) {
    let path = product["artifact_path"]
        .as_str()
        .expect("captured screenshot has an artifact path");
    let bytes = fs::read(path).expect("read captured PNG artifact");
    assert!(bytes.len() > 24, "captured PNG is nonempty");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("PNG IHDR width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("PNG IHDR height"));
    assert_eq!(product["width"].as_u64(), Some(u64::from(width)));
    assert_eq!(product["height"].as_u64(), Some(u64::from(height)));
    assert!(width > 0 && height > 0);
}

#[test]
#[ignore = "requires a visible Windows WGPU adapter and surface"]
fn step223_live_iced_acquired_surface_capture() {
    let mut child = build_and_launch_admission();
    let Some(port) = wait_for_listener(&mut child) else {
        let logs = child.logs();
        let environmental_absence = logs.to_ascii_lowercase().contains("adapter")
            || logs.to_ascii_lowercase().contains("surface");
        if environmental_absence {
            eprintln!("SKIP: no WGPU adapter/surface could be created\n{logs}");
            return;
        }
        panic!("real iced admission route did not advertise an MCP listener\n{logs}");
    };

    let stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .expect("connect to advertised iced MCP endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set live MCP read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(15)))
        .expect("set live MCP write timeout");
    let mut writer = stream.try_clone().expect("clone live MCP stream");
    let mut reader = BufReader::new(stream);

    let status = call_tool(
        &mut reader,
        &mut writer,
        "step223-iced-status",
        "slipway.debug.status",
        json!({}),
    );
    assert_eq!(status["product_kind"], "status");
    assert_eq!(status["product"]["backend_id"], "slipway-backend-iced");
    let frame = status["frame"].clone();

    let screenshot = call_tool(
        &mut reader,
        &mut writer,
        "step223-iced-screenshot",
        "slipway.debug.screenshot",
        json!({ "frame": frame }),
    );
    assert_eq!(screenshot["product_kind"], "presented_screenshot");
    assert_eq!(screenshot["refused"], false);
    let product = &screenshot["product"];
    assert_eq!(
        product["capture_path"],
        "direct_acquired_surface_texture_copy"
    );
    assert_eq!(product["source"]["label"], "backend_presented");
    assert_eq!(product["source"]["backend_id"], "slipway-backend-iced");
    assert_eq!(
        product["source"]["pass_id"],
        "presented-pixels/direct-surface-copy"
    );
    assert!(
        product["pixel_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("fnv1a64:"))
    );
    assert_png_matches_product(product);
    assert!(
        !screenshot
            .to_string()
            .contains("screenshot-capture-channel-closed")
    );

    child.close();
}
