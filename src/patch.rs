use crate::config::Settings;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

/// 从补丁文件中读取文件列表
pub fn get_patch_file_list(file_path: &str) -> Result<Vec<String>, String> {
    let file = fs::File::open(file_path)
        .map_err(|e| format!("打开补丁文件失败: {}", e))?;

    let reader = io::BufReader::new(file);
    let mut list = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("读取行失败: {}", e))?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // SVN Eclipse plugin format: "Index: relative/path/to/file"
        if let Some(idx) = line.find("Index:") {
            let path = line[idx + 6..].trim().to_string();
            if !path.is_empty() {
                list.push(path);
            }
            continue;
        }

        // Plain format: one path per line
        list.push(line);
    }

    Ok(list)
}

/// 复制文件，自动创建目标目录
pub fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败: {} -> {}", parent.display(), e))?;
    }

    fs::copy(src, dst)
        .map_err(|e| format!("复制文件失败: {} -> {}, {}", src.display(), dst.display(), e))?;

    Ok(())
}

/// 将补丁路径映射为源文件路径和目标文件路径
pub fn resolve_path(patch_path: &str, s: &Settings) -> (PathBuf, PathBuf) {
    let n = patch_path.replace('/', "\\");

    // 如果绝对路径以项目路径开头，去掉前缀得到相对路径
    let proj_norm = s.project_path.replace('/', "\\");
    let n = if n.starts_with(&proj_norm) {
        let stripped = n.strip_prefix(&proj_norm).unwrap_or(&n);
        stripped.strip_prefix('\\').unwrap_or(stripped).to_string()
    } else {
        n
    };

    let to_pfx = |p: &str| -> String {
        format!("{}\\", p.replace('/', "\\"))
    };

    // 1) src/main/java/... -> .class from classPath, to WEB-INF/classes
    if !s.src_java_prefix.is_empty() {
        let pfx = to_pfx(&s.src_java_prefix);
        if n.starts_with(&pfx) {
            let rel = &n[pfx.len()..];
            let cls = if rel.ends_with(".java") {
                format!("{}.class", &rel[..rel.len() - 5])
            } else {
                rel.to_string()
            };
            let src = PathBuf::from(&s.class_path).join(&cls);
            let dst = PathBuf::from(&s.des_path)
                .join(&s.version)
                .join("WEB-INF")
                .join("classes")
                .join(&cls);
            return (src, dst);
        }
    }

    // 2) src/main/resources/... -> from classPath, to WEB-INF/classes
    if !s.src_resource_prefix.is_empty() {
        let pfx = to_pfx(&s.src_resource_prefix);
        if n.starts_with(&pfx) {
            let rel = &n[pfx.len()..];
            let src = PathBuf::from(&s.class_path).join(rel);
            let dst = PathBuf::from(&s.des_path)
                .join(&s.version)
                .join("WEB-INF")
                .join("classes")
                .join(rel);
            return (src, dst);
        }
    }

    // 3) src/main/webapp/... -> from project, strip webapp prefix
    if !s.src_webapp_prefix.is_empty() {
        let pfx = to_pfx(&s.src_webapp_prefix);
        if n.starts_with(&pfx) {
            let rel = &n[pfx.len()..];
            let src = PathBuf::from(&s.project_path).join(&n);
            let dst = PathBuf::from(&s.des_path)
                .join(&s.version)
                .join(rel);
            return (src, dst);
        }
    }

    // 4) Old-style Eclipse project: "src\" prefix
    if n.starts_with("src\\") {
        let rel = &n[4..];
        let cls = if rel.ends_with(".java") {
            format!("{}.class", &rel[..rel.len() - 5])
        } else {
            rel.to_string()
        };
        let src = PathBuf::from(&s.class_path).join(&cls);
        let dst = PathBuf::from(&s.des_path)
            .join(&s.version)
            .join("WEB-INF")
            .join("classes")
            .join(&cls);
        return (src, dst);
    }

    // 5) Old-style Eclipse project: WebContent prefix
    if !s.web_content.is_empty() {
        let wc_pfx = format!("{}\\", s.web_content);
        if n.starts_with(&wc_pfx) {
            let rel = &n[wc_pfx.len()..];
            let src = PathBuf::from(&s.project_path).join(&n);
            let dst = PathBuf::from(&s.des_path)
                .join(&s.version)
                .join(rel);
            return (src, dst);
        }
    }

    // 6) Fallback: direct copy from project root
    let src = PathBuf::from(&s.project_path).join(&n);
    let dst = PathBuf::from(&s.des_path).join(&s.version).join(&n);
    (src, dst)
}

/// 执行补丁打包流程，通过回调函数输出日志
pub fn run_patch<F>(s: &Settings, mut log_fn: F)
where
    F: FnMut(String),
{
    log_fn("=== 开始读取补丁文件 ===".to_string());

    let file_list = match get_patch_file_list(&s.patch_file) {
        Ok(list) => list,
        Err(e) => {
            log_fn(format!("读取补丁文件失败: {}", e));
            log_fn("=== 打包失败 ===".to_string());
            return;
        }
    };

    log_fn(format!("共发现 {} 个文件需要处理", file_list.len()));

    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    for patch_path in &file_list {
        let (src, dst) = resolve_path(patch_path, s);

        match copy_file(&src, &dst) {
            Ok(()) => {
                log_fn(format!("成功: {}", patch_path));
                success_count += 1;
            }
            Err(e) => {
                log_fn(format!("失败: {}", e));
                fail_count += 1;
            }
        }
    }

    log_fn(format!(
        "=== 打包完成: 成功 {} 个, 失败 {} 个 ===",
        success_count, fail_count
    ));
}
