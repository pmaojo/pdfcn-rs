use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use notify::{RecursiveMode, Watcher};
use pdfcn_core::PageConfig;

#[derive(Clone)]
struct DevState {
    template: PathBuf,
    data: PathBuf,
    version: Arc<AtomicU64>,
}

const RELOAD_SCRIPT: &str = r#"
<script>
  let lastVersion = null;
  async function poll() {
    try {
      const res = await fetch('/__pdfcn_version');
      const v = await res.text();
      if (lastVersion !== null && v !== lastVersion) { location.reload(); }
      lastVersion = v;
    } catch (e) {}
    setTimeout(poll, 700);
  }
  poll();
</script>
"#;

async fn render_handler(State(state): State<DevState>) -> impl IntoResponse {
    let render = || -> anyhow::Result<String> {
        let source = std::fs::read_to_string(&state.template)?;
        let data_source = std::fs::read_to_string(&state.data)?;
        let format = state
            .data
            .extension()
            .and_then(|e| e.to_str())
            .and_then(pdfcn_core::DataFormat::from_extension)
            .unwrap_or(pdfcn_core::DataFormat::Json);
        let data = pdfcn_core::load_data(&data_source, format)?;
        let base_dir = state
            .template
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let loader = pdfcn_core::FsPartialLoader::new(base_dir);
        let html = pdfcn_core::render_html(&source, &data, &loader)?;
        Ok(html.replacen("</body>", &format!("{RELOAD_SCRIPT}</body>"), 1))
    };

    match render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!(
            "<pre style=\"color:red;white-space:pre-wrap\">{}</pre>{}",
            html_escape(&e.to_string()),
            RELOAD_SCRIPT
        ))
        .into_response(),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn version_handler(State(state): State<DevState>) -> impl IntoResponse {
    state.version.load(Ordering::Relaxed).to_string()
}

pub fn run(template: &Path, data: &Path, port: u16) -> anyhow::Result<()> {
    // Validate up front so a typo fails immediately instead of on first request.
    let _ = PageConfig::default();
    if !template.exists() {
        anyhow::bail!("template not found: {template:?}");
    }
    if !data.exists() {
        anyhow::bail!("data file not found: {data:?}");
    }

    let state = DevState {
        template: template.to_path_buf(),
        data: data.to_path_buf(),
        version: Arc::new(AtomicU64::new(0)),
    };

    let watch_version = state.version.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    if let Some(dir) = template.parent().filter(|d| !d.as_os_str().is_empty()) {
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
    }
    if let Some(dir) = data.parent().filter(|d| !d.as_os_str().is_empty()) {
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
    }
    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive for the thread's lifetime
        for event in rx {
            if event.is_ok() {
                watch_version.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let app = Router::new()
        .route("/", get(render_handler))
        .route("/__pdfcn_version", get(version_handler))
        .with_state(state);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        println!("pdfcn dev server: http://127.0.0.1:{port}");
        axum::serve(listener, app).await?;
        Ok::<_, anyhow::Error>(())
    })
}
