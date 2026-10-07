//! Boots a real app (not `TestApp`) with the plugin. Config comes from env
//! vars, so this also tests the `[meta_pixel]` env layer and strict config.
//!
//! The test runs this test binary again as a child process, with the env
//! vars set. So it changes no env in this process.
#![allow(clippy::unwrap_used, clippy::expect_used)] // Test helpers fail loudly.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use autumn_plugin_meta_pixel::{MetaPixel, MetaPixelPlugin};
use autumn_web::prelude::*;
use autumn_web::test::TestApp;

const CHILD_MARK: &str = "META_PIXEL_BOOT_TEST_CHILD";
const CSP: &str = "default-src 'self'; script-src 'self' https://connect.facebook.net; \
                   img-src 'self' https://www.facebook.com; connect-src 'self' https://www.facebook.com";

#[get("/")]
async fn index(pixel: MetaPixel) -> Markup {
    html! { head { (pixel.head()) } body { (pixel.noscript()) } }
}

/// The server. It runs only in the child process.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs only as the child of app_boots_with_env_config"]
async fn boot_child_server() {
    if std::env::var(CHILD_MARK).is_err() {
        return;
    }
    autumn_web::app()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::new())
        .run()
        .await;
}

/// A command that runs this test binary again with a clean environment:
/// only `PATH`, so no `AUTUMN_*` value of the shell leaks in.
fn child(test: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--include-ignored", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(CHILD_MARK, "1");
    cmd
}

/// `MetaPixelPlugin::new()` with no `autumn.toml` and no env: no pixel IDs,
/// so the pixel is off. Runs only in the child process.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs only as the child of new_with_no_config_starts_off"]
async fn new_child() {
    if std::env::var(CHILD_MARK).is_err() {
        return;
    }
    let client = TestApp::new()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::new())
        .build();
    let set = autumn_web::consent::accept_all_cookie(&["marketing"], 1);
    let cookie = set.split(';').next().unwrap().to_owned();
    let body = client
        .get("/")
        .header("cookie", &cookie)
        .send()
        .await
        .text();
    assert!(!body.contains("meta-pixel"), "{body}");
}

#[test]
fn new_with_no_config_starts_off() {
    let dir = std::env::temp_dir().join(format!("meta-pixel-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = child("new_child")
        .env("AUTUMN_MANIFEST_DIR", &dir)
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("1 passed"), "{text}");
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http_get(port: u16, path: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

#[test]
fn app_boots_with_env_config() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut child = KillOnDrop(
        child("boot_child_server")
            .env("AUTUMN_SERVER__PORT", port.to_string())
            .env("AUTUMN_SERVER__HOST", "127.0.0.1")
            .env("AUTUMN_SECURITY__HEADERS__CONTENT_SECURITY_POLICY", CSP)
            .env("AUTUMN_META_PIXEL__PIXEL_IDS", "1234567890")
            .env("AUTUMN_META_PIXEL__REQUIRE_CONSENT", "false")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut err = String::new();
            if let Some(mut e) = child.0.stderr.take() {
                e.read_to_string(&mut err).unwrap();
            }
            panic!("the app stopped at boot ({status}):\n{err}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the app did not answer"
        );
        if let Ok(page) = http_get(port, "/") {
            assert!(page.starts_with("HTTP/1.1 200"), "{page}");
            assert!(page.contains("1234567890"), "{page}");
            let src = page
                .split("src=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap()
                .to_owned();
            assert!(
                src.starts_with("/static/_plugins/meta-pixel/meta-pixel."),
                "{src}"
            );
            let js = http_get(port, &src).unwrap();
            assert!(js.starts_with("HTTP/1.1 200"), "{js}");
            assert!(js.to_ascii_lowercase().contains("immutable"), "{js}");
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
