use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        Html, Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::config::{self, ProfileStore, Settings};
use crate::patch;
use crate::AppState;

pub async fn run_server(state: Arc<AppState>) {
    let app = Router::new()
        .route("/", get(handle_index))
        .route("/api/config", get(handle_config_get).post(handle_config_post))
        .route("/api/start", post(handle_start))
        .route("/api/logs", get(handle_logs))
        .route("/api/pick-file", post(handle_pick_file))
        .route("/api/pick-folder", post(handle_pick_folder))
        .route(
            "/api/patch-content",
            get(handle_patch_content_get).post(handle_patch_content_post),
        )
        .route("/api/ensure-file", post(handle_ensure_file))
        .with_state(state);

    // 从 18080 开始查找可用端口
    let (listener, port) = bind_available(18080, 18090).await
        .expect("无法找到可用端口 (18080-18090)");

    println!("FreePatch 补丁打包工具已启动");
    println!("请访问 http://localhost:{}", port);

    // 尝试用默认浏览器打开
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", &format!("http://localhost:{}", port)])
        .spawn();

    axum::serve(listener, app).await.expect("服务器启动失败");
}

async fn bind_available(start: u16, end: u16) -> Option<(tokio::net::TcpListener, u16)> {
    for port in start..=end {
        if let Ok(listener) = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
            return Some((listener, port));
        }
    }
    None
}

async fn handle_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn handle_config_get(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.lock().await;
    Json(serde_json::to_value(&config.data).unwrap_or(json!({})))
}

async fn handle_config_post(
    State(state): State<Arc<AppState>>,
    Json(ps): Json<ProfileStore>,
) -> Json<Value> {
    let mut config = state.config.lock().await;
    config.data = ps.clone();
    config::save_config(&state.config_path, &ps);
    Json(json!({"status": "ok"}))
}

/// 从项目路径自动推导其他空字段
fn derive_settings(s: &mut Settings) {
    if s.project_path.is_empty() {
        return;
    }
    let proj = s.project_path.replace('/', "\\");
    let proj = proj.trim_end_matches('\\');

    if s.class_path.is_empty() {
        s.class_path = format!("{}\\target\\classes", proj);
    }
    if s.des_path.is_empty() {
        s.des_path = format!("{}\\target\\patch_pkg", proj);
    }
    if s.patch_file.is_empty() {
        s.patch_file = format!("{}\\diff.txt", proj);
    }
    if s.version.is_empty() {
        // 从项目路径提取最后一级目录名作为版本前缀
        let dir_name = std::path::Path::new(&s.project_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        s.version = format!("{}-{}", dir_name, chrono::Local::now().format("%Y%m%d-%H%M%S"));
    }
    if s.src_java_prefix.is_empty() {
        s.src_java_prefix = "src/main/java".to_string();
    }
    if s.src_resource_prefix.is_empty() {
        s.src_resource_prefix = "src/main/resources".to_string();
    }
    if s.src_webapp_prefix.is_empty() {
        s.src_webapp_prefix = "src/main/webapp".to_string();
    }
    if s.web_content.is_empty() {
        s.web_content = "WebContent".to_string();
    }
}

async fn handle_start(
    State(state): State<Arc<AppState>>,
    Json(mut s): Json<Settings>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // 检查是否正在运行
    {
        let log_state = state.log_state.lock().await;
        if log_state.running {
            return Err((
                StatusCode::BAD_REQUEST,
                "正在打包中，请等待完成".to_string(),
            ));
        }
    }

    // 从项目路径自动推导空字段
    derive_settings(&mut s);

    // 参数校验
    if s.project_path.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "项目路径不能为空".to_string(),
        ));
    }
    if s.patch_file.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "补丁文件路径不能为空".to_string(),
        ));
    }

    // 创建广播通道
    let (tx, _) = broadcast::channel::<String>(256);

    // 设置运行状态
    {
        let mut log_state = state.log_state.lock().await;
        log_state.running = true;
        log_state.lines.clear();
        log_state.listeners = vec![tx.clone()];
    }

    // 在后台线程中执行打包
    let state_clone = state.clone();
    tokio::task::spawn_blocking(move || {
        patch::run_patch(&s, |msg| {
            let ts = chrono::Local::now().format("%H:%M:%S").to_string();
            let line = format!("[{}] {}", ts, msg);

            // 使用 block_on 来获取 lock
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let mut log_state = state_clone.log_state.lock().await;
                log_state.lines.push(line.clone());
                for sender in &log_state.listeners {
                    let _ = sender.send(line.clone());
                }
            });
        });

        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut log_state = state_clone.log_state.lock().await;
            log_state.running = false;
        });
    });

    Ok(Json(json!({"status": "started"})))
}

async fn handle_logs(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (existing_lines, rx) = {
        let mut log_state = state.log_state.lock().await;

        let existing = log_state.lines.clone();

        // 如果没有广播器，创建一个
        if log_state.listeners.is_empty() {
            let (tx, _) = broadcast::channel::<String>(256);
            log_state.listeners.push(tx);
        }

        let rx = log_state.listeners[0].subscribe();
        (existing, rx)
    };

    let existing_stream = tokio_stream::iter(
        existing_lines
            .into_iter()
            .map(|line| Ok(Event::default().data(line))),
    );

    let live_stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(line) => Some(Ok(Event::default().data(line))),
        Err(_) => None,
    });

    let combined = existing_stream.chain(live_stream);

    Sse::new(combined)
}

async fn handle_pick_file() -> Json<Value> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("选择补丁文件")
            .add_filter("补丁文件", &["txt"])
            .add_filter("所有文件", &["*"])
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    Json(json!({"path": path}))
}

async fn handle_pick_folder() -> Json<Value> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("选择项目目录")
            .pick_folder()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    Json(json!({"path": path}))
}

/// GET /api/patch-content?path=... — 读取补丁文件内容
async fn handle_patch_content_get(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let file_path = match params.get("path") {
        Some(p) if !p.is_empty() => p,
        _ => return Json(json!({"content": "", "error": "缺少path参数"})),
    };

    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => String::new(), // 文件不存在或无法读取返回空
    };

    let exists = std::path::Path::new(file_path).exists();
    Json(json!({"content": content, "exists": exists}))
}

/// POST /api/patch-content — 保存补丁文件内容
/// Body: {"path": "...", "content": "..."}
async fn handle_patch_content_post(
    Json(body): Json<serde_json::Value>,
) -> Json<Value> {
    let file_path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json(json!({"status": "error", "message": "缺少path参数"})),
    };

    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");

    // 确保目录存在
    if let Some(parent) = Path::new(file_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::write(file_path, content) {
        Ok(_) => Json(json!({"status": "ok"})),
        Err(e) => Json(json!({"status": "error", "message": e.to_string()})),
    }
}

/// POST /api/ensure-file — 确保补丁文件存在，不存在则自动创建
async fn handle_ensure_file(Json(body): Json<serde_json::Value>) -> Json<Value> {
    let file_path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json(json!({"status": "error", "message": "缺少path参数"})),
    };
    let path = Path::new(file_path);
    if path.exists() {
        return Json(json!({"status": "ok", "created": false}));
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(file_path, "") {
        Ok(_) => Json(json!({"status": "ok", "created": true})),
        Err(e) => Json(json!({"status": "error", "message": e.to_string()})),
    }
}

const INDEX_HTML: &str = include_str!("index.html");
