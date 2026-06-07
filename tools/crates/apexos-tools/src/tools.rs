use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

// ─── Tool list ───────────────────────────────────────────────────────────────

pub fn list() -> Value {
    json!([
        {
            "name": "run_command",
            "description": "Execute a shell command. Subject to a hard denylist for destructive operations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cmd": { "type": "string", "description": "Command to run (passed to /bin/sh -c)" },
                    "cwd": { "type": "string", "description": "Working directory (optional)" },
                    "env": { "type": "object", "description": "Extra environment variables (optional)", "additionalProperties": { "type": "string" } },
                    "timeout_secs": { "type": "integer", "description": "Timeout in seconds (default 30, max 300)" }
                },
                "required": ["cmd"]
            }
        },
        {
            "name": "read_file",
            "description": "Read a file from the filesystem.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_bytes": { "type": "integer", "description": "Maximum bytes to read (default 1MB)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "write_file",
            "description": "Write or append to a file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "append": { "type": "boolean", "description": "Append instead of overwrite (default false)" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "list_dir",
            "description": "List directory contents.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "recursive": { "type": "boolean", "description": "Recurse into subdirectories (max 3 levels)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "create_dir",
            "description": "Create a directory (and parents).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "delete_path",
            "description": "Delete a file or directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "recursive": { "type": "boolean", "description": "Required true to delete a directory" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "http_fetch",
            "description": "Make an HTTP request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "method": { "type": "string", "description": "HTTP method (default GET)" },
                    "headers": { "type": "object", "additionalProperties": { "type": "string" } },
                    "body": { "type": "string" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "cpu_temp",
            "description": "Read CPU temperature from thermal sensors.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "disk_usage",
            "description": "Report disk usage for mounted filesystems.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Filter to filesystem containing this path (optional)" }
                }
            }
        },
        {
            "name": "memory_info",
            "description": "Report system memory usage from /proc/meminfo.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "uptime",
            "description": "Report system uptime and load averages.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

// ─── Dispatch ────────────────────────────────────────────────────────────────

pub fn call(name: &str, args: &Value) -> Value {
    match name {
        "run_command" => run_command(args),
        "read_file" => read_file(args),
        "write_file" => write_file(args),
        "list_dir" => list_dir(args),
        "create_dir" => create_dir(args),
        "delete_path" => delete_path(args),
        "http_fetch" => http_fetch(args),
        "cpu_temp" => cpu_temp(),
        "disk_usage" => disk_usage(args),
        "memory_info" => memory_info(),
        "uptime" => uptime(),
        _ => tool_error(format!("unknown tool: {}", name)),
    }
}

fn tool_ok(content: Value) -> Value {
    json!({ "content": [{ "type": "text", "text": content.to_string() }] })
}

fn tool_error(msg: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": json!({"error": msg.into()}).to_string() }], "isError": true })
}

// ─── Denylist ────────────────────────────────────────────────────────────────

fn denylist_check(cmd: &str) -> Option<&'static str> {
    let trimmed = cmd.trim();

    // Disk destruction
    if trimmed.starts_with("mkfs") {
        return Some("mkfs commands are blocked");
    }
    if trimmed.contains("wipefs") {
        return Some("wipefs is blocked");
    }

    // Raw device writes via dd
    if trimmed.starts_with("dd") {
        let lower = trimmed.to_lowercase();
        if lower.contains("of=/dev/sd")
            || lower.contains("of=/dev/nvme")
            || lower.contains("of=/dev/mmcblk")
        {
            return Some("dd to raw block devices is blocked");
        }
    }

    // Partition table editors on real devices
    for tool in &["fdisk", "parted", "gdisk"] {
        if trimmed.starts_with(tool) && trimmed.contains("/dev/") {
            return Some("partition table editing on real devices is blocked");
        }
    }

    // rm -rf / (and variants)
    if trimmed.contains("rm") && trimmed.contains("-r") {
        let lower = trimmed.to_lowercase();
        // Match rm -rf / or rm -rf --no-preserve-root /
        if lower.contains("--no-preserve-root")
            || (trimmed.ends_with(" /") || trimmed.contains(" / "))
        {
            // Check it's actually targeting root
            if lower.contains(" /")
                && !lower.contains("/var")
                && !lower.contains("/tmp")
                && !lower.contains("/home")
                && !lower.contains("/opt")
            {
                return Some("rm -rf / is blocked");
            }
        }
    }

    // System directory destruction
    for protected in &[
        "rm -rf /usr",
        "rm -rf /bin",
        "rm -rf /lib",
        "rm -rf /sbin",
        "rm -rf /boot",
        "rm -rf /etc/passwd",
        "rm -rf /etc/shadow",
    ] {
        if trimmed.contains(protected) {
            return Some("destruction of system directories is blocked");
        }
    }

    // Truncation of critical auth files
    if (trimmed.starts_with("> /etc/passwd") || trimmed.starts_with("> /etc/shadow"))
        || trimmed.contains("truncate") && trimmed.contains("/etc/passwd")
        || trimmed.contains("truncate") && trimmed.contains("/etc/shadow")
    {
        return Some("truncating auth files is blocked");
    }

    // Fork bomb pattern
    if trimmed.contains(":(){ :|:") {
        return Some("fork bomb pattern is blocked");
    }

    None
}

// ─── Tool implementations ────────────────────────────────────────────────────

fn run_command(args: &Value) -> Value {
    let cmd = match args["cmd"].as_str() {
        Some(c) => c,
        None => return tool_error("cmd is required"),
    };

    if let Some(reason) = denylist_check(cmd) {
        return tool_error(format!("BLOCKED: {}", reason));
    }

    let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(30).min(300);
    let cwd = args["cwd"].as_str();

    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(cmd);

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    if let Some(env_map) = args["env"].as_object() {
        for (k, v) in env_map {
            if let Some(val) = v.as_str() {
                command.env(k, val);
            }
        }
    }

    use std::sync::mpsc;
    use std::thread;

    let child = match command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return tool_error(format!("failed to spawn: {}", e)),
    };

    let (tx, rx) = mpsc::channel::<std::io::Result<std::process::Output>>();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Ok(output)) => tool_ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
            "timed_out": false
        })),
        Ok(Err(e)) => tool_error(format!("command error: {}", e)),
        Err(_) => tool_ok(json!({
            "stdout": "",
            "stderr": format!("command timed out after {}s", timeout_secs),
            "exit_code": -1,
            "timed_out": true
        })),
    }
}

fn read_file(args: &Value) -> Value {
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return tool_error("path is required"),
    };
    let max_bytes = args["max_bytes"].as_u64().unwrap_or(1_048_576) as usize;

    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return tool_error(format!("cannot open {}: {}", path, e)),
    };

    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = vec![0u8; max_bytes.min(size as usize + 1)];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(e) => return tool_error(format!("read error: {}", e)),
    };
    buf.truncate(n);

    let content = String::from_utf8_lossy(&buf).to_string();
    let truncated = n >= max_bytes && (size as usize) > max_bytes;

    tool_ok(json!({
        "content": content,
        "size_bytes": size,
        "truncated": truncated
    }))
}

fn write_file(args: &Value) -> Value {
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return tool_error("path is required"),
    };
    let content = match args["content"].as_str() {
        Some(c) => c,
        None => return tool_error("content is required"),
    };
    let append = args["append"].as_bool().unwrap_or(false);

    // Create parent dirs if needed
    if let Some(parent) = Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    use std::io::Write as IoWrite;
    use std::fs::OpenOptions;

    let mut file = match OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => return tool_error(format!("cannot open {}: {}", path, e)),
    };

    match file.write_all(content.as_bytes()) {
        Ok(_) => tool_ok(json!({ "bytes_written": content.len() })),
        Err(e) => tool_error(format!("write error: {}", e)),
    }
}

fn list_dir(args: &Value) -> Value {
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return tool_error("path is required"),
    };
    let recursive = args["recursive"].as_bool().unwrap_or(false);

    let mut entries = Vec::new();
    collect_dir(path, recursive, 0, &mut entries);
    tool_ok(json!(entries))
}

fn collect_dir(path: &str, recursive: bool, depth: usize, out: &mut Vec<Value>) {
    if depth > 3 {
        return;
    }
    let read = match fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let meta = entry.metadata().ok();
        let kind = meta.as_ref().map(|m| if m.is_dir() { "dir" } else { "file" }).unwrap_or("unknown");
        let size = meta.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None });
        let modified = meta.as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        let mut entry_json = json!({
            "name": entry.path().to_string_lossy(),
            "kind": kind,
        });
        if let Some(s) = size { entry_json["size"] = json!(s); }
        if let Some(m) = modified { entry_json["modified"] = json!(m); }
        out.push(entry_json);

        if recursive && kind == "dir" {
            collect_dir(&entry.path().to_string_lossy(), true, depth + 1, out);
        }
    }
}

fn create_dir(args: &Value) -> Value {
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return tool_error("path is required"),
    };
    match fs::create_dir_all(path) {
        Ok(_) => tool_ok(json!({ "created": path })),
        Err(e) => tool_error(format!("create_dir failed: {}", e)),
    }
}

fn delete_path(args: &Value) -> Value {
    let path = match args["path"].as_str() {
        Some(p) => p,
        None => return tool_error("path is required"),
    };

    // Denylist check on path directly
    let protected = ["/", "/usr", "/bin", "/lib", "/sbin", "/boot", "/etc/passwd", "/etc/shadow"];
    for p in &protected {
        if path == *p || path.starts_with(&format!("{}/", p)) {
            return tool_error(format!("deletion of {} is blocked", p));
        }
    }

    let recursive = args["recursive"].as_bool().unwrap_or(false);
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return tool_error(format!("cannot stat {}: {}", path, e)),
    };

    let result = if meta.is_dir() {
        if !recursive {
            return tool_error("path is a directory — set recursive=true to delete it");
        }
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(_) => tool_ok(json!({ "deleted": path })),
        Err(e) => tool_error(format!("delete failed: {}", e)),
    }
}

fn http_fetch(args: &Value) -> Value {
    let url = match args["url"].as_str() {
        Some(u) => u,
        None => return tool_error("url is required"),
    };
    let method = args["method"].as_str().unwrap_or("GET").to_uppercase();

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return tool_error(format!("client build failed: {}", e)),
    };

    let mut req = match method.as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        "HEAD" => client.head(url),
        _ => return tool_error(format!("unsupported method: {}", method)),
    };

    if let Some(headers) = args["headers"].as_object() {
        for (k, v) in headers {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    if let Some(body) = args["body"].as_str() {
        req = req.body(body.to_string());
    }

    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return tool_error(format!("request failed: {}", e)),
    };

    let status = resp.status().as_u16();
    let resp_headers: serde_json::Map<String, Value> = resp.headers().iter()
        .map(|(k, v)| (k.as_str().to_string(), json!(v.to_str().unwrap_or(""))))
        .collect();

    // Cap response body at 4MB
    let body_bytes = resp.bytes().unwrap_or_default();
    let body_str = if body_bytes.len() > 4_194_304 {
        format!("[truncated at 4MB, total {} bytes]", body_bytes.len())
    } else {
        String::from_utf8_lossy(&body_bytes).to_string()
    };

    tool_ok(json!({
        "status": status,
        "body": body_str,
        "headers": resp_headers
    }))
}

fn cpu_temp() -> Value {
    let thermal_base = "/sys/class/thermal";
    let zones = match fs::read_dir(thermal_base) {
        Ok(r) => r,
        Err(e) => return tool_error(format!("cannot read thermal zones: {}", e)),
    };

    let mut readings = Vec::new();
    for entry in zones.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("thermal_zone") {
            continue;
        }
        let temp_path = entry.path().join("temp");
        let type_path = entry.path().join("type");

        let raw = match fs::read_to_string(&temp_path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue,
        };
        let sensor_type = fs::read_to_string(&type_path)
            .unwrap_or_default()
            .trim()
            .to_string();

        if let Ok(millideg) = raw.parse::<i64>() {
            readings.push(json!({
                "sensor": if sensor_type.is_empty() { name } else { sensor_type },
                "temp_c": millideg as f64 / 1000.0
            }));
        }
    }

    if readings.is_empty() {
        return tool_error("no thermal zones found");
    }

    // Primary is usually the first / highest
    let primary = readings[0].clone();
    tool_ok(json!({
        "temp_c": primary["temp_c"],
        "sensor": primary["sensor"],
        "all_zones": readings
    }))
}

fn disk_usage(args: &Value) -> Value {
    let filter_path = args["path"].as_str();

    let mounts_raw = match fs::read_to_string("/proc/mounts") {
        Ok(s) => s,
        Err(e) => return tool_error(format!("cannot read /proc/mounts: {}", e)),
    };

    let mut results = Vec::new();
    for line in mounts_raw.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let mount = parts[1];

        // Skip pseudo filesystems
        if mount == "none" || mount.starts_with("/proc") || mount.starts_with("/sys")
            || mount.starts_with("/dev") || mount == "/run"
        {
            continue;
        }

        if let Some(fp) = filter_path {
            if !fp.starts_with(mount) {
                continue;
            }
        }

        // statvfs via /proc/mounts entry
        if let Some(stat) = statvfs(mount) {
            results.push(stat);
        }
    }

    if results.is_empty() && filter_path.is_none() {
        // Fallback: just do /
        if let Some(stat) = statvfs("/") {
            results.push(stat);
        }
    }

    tool_ok(json!(results))
}

fn statvfs(path: &str) -> Option<Value> {
    // Use `df` command as a portable alternative to calling statvfs syscall directly
    let out = Command::new("df")
        .arg("-B1")
        .arg("--output=source,target,size,used,avail,pcent")
        .arg(path)
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().skip(1); // skip header
    let line = lines.next()?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }

    let total: u64 = parts[2].parse().unwrap_or(0);
    let used: u64 = parts[3].parse().unwrap_or(0);
    let free: u64 = parts[4].parse().unwrap_or(0);
    let pct = parts[5].trim_end_matches('%').parse::<f64>().unwrap_or(0.0);

    Some(json!({
        "mount": parts[1],
        "total_gb": (total as f64) / 1e9,
        "used_gb": (used as f64) / 1e9,
        "free_gb": (free as f64) / 1e9,
        "pct": pct
    }))
}

fn memory_info() -> Value {
    let raw = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(e) => return tool_error(format!("cannot read /proc/meminfo: {}", e)),
    };

    let mut map = std::collections::HashMap::new();
    for line in raw.lines() {
        let mut parts = line.splitn(2, ':');
        if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
            let kb: u64 = val.trim().split_whitespace().next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            map.insert(key.trim().to_string(), kb);
        }
    }

    let total = *map.get("MemTotal").unwrap_or(&0);
    let available = *map.get("MemAvailable").unwrap_or(&0);
    let swap_total = *map.get("SwapTotal").unwrap_or(&0);
    let swap_free = *map.get("SwapFree").unwrap_or(&0);

    tool_ok(json!({
        "total_mb": total / 1024,
        "available_mb": available / 1024,
        "used_mb": (total - available) / 1024,
        "swap_used_mb": (swap_total - swap_free) / 1024
    }))
}

fn uptime() -> Value {
    let raw = match fs::read_to_string("/proc/uptime") {
        Ok(s) => s,
        Err(e) => return tool_error(format!("cannot read /proc/uptime: {}", e)),
    };
    let uptime_secs: f64 = raw.split_whitespace().next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);

    let loadavg = match fs::read_to_string("/proc/loadavg") {
        Ok(s) => s,
        Err(e) => return tool_error(format!("cannot read /proc/loadavg: {}", e)),
    };
    let parts: Vec<&str> = loadavg.split_whitespace().collect();
    let load1: f64 = parts.get(0).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let load5: f64 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let load15: f64 = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);

    tool_ok(json!({
        "uptime_secs": uptime_secs as u64,
        "load_avg_1": load1,
        "load_avg_5": load5,
        "load_avg_15": load15
    }))
}
