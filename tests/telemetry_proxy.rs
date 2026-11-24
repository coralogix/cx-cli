use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRACE_PATH: &str = "/v1/traces";
const INVALID_COMMAND: &str = "telemetry-proxy-invalid-command";

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cx-telemetry-proxy-{}", uuid::Uuid::new_v4())))
    }

    fn cx(&self, arg: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cx"));
        env_clear(&mut command);
        command
            .env("CX_HOME", &self.0)
            .env("CX_NO_UPDATE_NOTIFIER", "1")
            .env("CX_NO_SKILLS_AUTO_UPDATE", "1")
            .kill_on_drop(true)
            .arg(arg);
        command
    }

    fn configured(&self, arg: &str, endpoint: &str) -> Command {
        let mut command = self.cx(arg);
        command
            .env("CX_TELEMETRY_ENDPOINT", endpoint)
            .env("CX_TELEMETRY_API_KEY", "proxy-test-key");
        command
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn env_clear(command: &mut Command) -> &mut Command {
    command.env_clear();
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

struct ProxyCapture {
    address: std::net::SocketAddr,
    first_lines: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl ProxyCapture {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let first_lines = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&first_lines);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let captured = Arc::clone(&captured);
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    let mut first_line = String::new();
                    if reader.read_line(&mut first_line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    captured
                        .lock()
                        .unwrap()
                        .push(first_line.trim_end().to_owned());
                    let is_connect = first_line.starts_with("CONNECT ");
                    let mut content_length = 0usize;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if header == "\r\n" || header == "\n" {
                            break;
                        }
                        let lower = header.to_ascii_lowercase();
                        if let Some(value) = lower.strip_prefix("content-length:") {
                            content_length = value.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                    if !is_connect && content_length > 0 {
                        let mut body = vec![0; content_length];
                        let _ = reader.read_exact(&mut body).await;
                    }
                    let response = if is_connect {
                        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n"
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                    };
                    let _ = writer.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            address,
            first_lines,
            task,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn lines(&self) -> Vec<String> {
        self.first_lines.lock().unwrap().clone()
    }
}

impl Drop for ProxyCapture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn closed_local_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_for_proxy_requests(proxy: &ProxyCapture, minimum: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let lines = proxy.lines();
        if lines.len() >= minimum {
            return lines;
        }
        assert!(
            Instant::now() < deadline,
            "received {} proxy requests, expected at least {minimum}",
            lines.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn http_proxy_routes_inline_and_detached_sender_traffic() {
    let home = Home::new();
    let proxy = ProxyCapture::start().await;
    let destination_port = closed_local_port();
    let endpoint = format!("http://127.0.0.1:{destination_port}{TRACE_PATH}");

    let baseline = home
        .cx(INVALID_COMMAND)
        .env("CX_TELEMETRY", "0")
        .output()
        .await
        .unwrap();
    let inline = home
        .configured(INVALID_COMMAND, &endpoint)
        .env("HTTP_PROXY", proxy.url())
        .output()
        .await
        .unwrap();
    assert_eq!(inline.status.code(), Some(2));
    assert_eq!(inline.stdout, baseline.stdout);
    assert_eq!(inline.stderr, baseline.stderr);

    let background = home
        .configured(INVALID_COMMAND, &endpoint)
        .env("CX_TELEMETRY_DELIVERY", "background")
        .env("HTTP_PROXY", proxy.url())
        .output()
        .await
        .unwrap();
    assert_eq!(background.status.code(), Some(2));
    assert_eq!(background.stdout, baseline.stdout);
    assert_eq!(background.stderr, baseline.stderr);

    let target = format!("POST http://127.0.0.1:{destination_port}{TRACE_PATH}");
    let lines = wait_for_proxy_requests(&proxy, 2).await;
    assert!(
        lines
            .iter()
            .filter(|line| line.starts_with(&target))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn no_proxy_bypasses_proxy_for_loopback_collector() {
    let home = Home::new();
    let proxy = ProxyCapture::start().await;
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TRACE_PATH))
        .respond_with(ResponseTemplate::new(200))
        .mount(&collector)
        .await;
    let endpoint = format!("{}{TRACE_PATH}", collector.uri());

    let output = home
        .configured(INVALID_COMMAND, &endpoint)
        .env("HTTP_PROXY", proxy.url())
        .env("NO_PROXY", "127.0.0.1,localhost")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(collector.received_requests().await.unwrap().len(), 1);
    assert!(proxy.lines().is_empty());
}

#[tokio::test]
async fn https_proxy_receives_connect_for_local_https_target() {
    let home = Home::new();
    let proxy = ProxyCapture::start().await;
    let destination_port = closed_local_port();
    let endpoint = format!("https://127.0.0.1:{destination_port}{TRACE_PATH}");

    let output = home
        .configured(INVALID_COMMAND, &endpoint)
        .env("HTTPS_PROXY", proxy.url())
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let lines = wait_for_proxy_requests(&proxy, 1).await;
    let connect = format!("CONNECT 127.0.0.1:{destination_port} HTTP/1.1");
    assert!(lines.iter().any(|line| line.starts_with(&connect)));
}
