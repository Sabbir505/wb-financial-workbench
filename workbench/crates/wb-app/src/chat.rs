//! AI side chat (F14): a local llama.cpp engine (`llama-server`) serving small
//! 2–6B GGUF models, as the reviewer's copilot inside the verification loop.
//!
//! End-to-end flow, all local-first:
//! 1. engine probe — `llama-server.exe` from WB_LLAMA_SERVER, PATH, the winget
//!    links dir or `data_dir/tools`; if missing the panel offers
//!    `winget install llama.cpp` and the GitHub releases page;
//! 2. model catalog — curated 2–6B models, tiered against the detected
//!    device (VRAM/RAM via nvidia-smi / PowerShell); downloaded with the
//!    Windows-bundled `curl.exe` (ureq is built without TLS on purpose —
//!    the research channel is localhost-only);
//! 3. server — spawned per model on 127.0.0.1:chat_port, health-polled,
//!    killed on app quit;
//! 4. chat — OpenAI-compatible `/v1/chat/completions` with SSE streaming via
//!    ureq; the current task's brief, guardrail flags and structural checks
//!    are injected as the system prompt, so questions like "why is 1652
//!    flagged?" answer with real evidence.

use crate::i18n::{self, Lang};
use crate::input::TextInput;
use gpui::{div, prelude::*, px, Entity, Hsla, ParentElement, Styled};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wb_core::guardrail::Guardrail;
use wb_core::model::{Evidence, TaskDetail};
use wb_core::structural::StructuralReport;

// ============================================================ model catalog

pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub params_b: f32,
    /// Approximate Q4_K_M file size in GB — drives the tier badge and the
    /// progress bar's denominator.
    pub size_gb: f32,
    pub repo: &'static str,
    pub filename: &'static str,
    pub note_zh: &'static str,
    pub note_en: &'static str,
}

impl ModelInfo {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/main/{}", self.repo, self.filename)
    }
    pub fn note(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::Zh => self.note_zh,
            Lang::En => self.note_en,
        }
    }
}

/// Curated for THIS workbench's job (Nov 2026): a Chinese A-share financial
/// review assistant that quotes evidence, in zh/en, on a 6GB-VRAM laptop.
/// Ordered by job-fit, not by general leaderboard rank:
/// 1. XuanYuan-6B-Chat — the only open Chinese-finance chat LLM in this size
///    class (Du Xiaoman, pretrained from scratch on financial corpora; leads
///    the FinanceIQ benchmark vs general opensource). Domain knowledge beats
///    general chat quality when the user asks 「扣非是什么意思」mid-review.
/// 2. Qwen3.5-4B — the general baseline: best <5B overall (Artificial
///    Analysis), strongest zh/en instruction following, so the best choice
///    when answers must lean on the *injected evidence* rather than memorized
///    domain knowledge.
/// 3. MiniCPM5-2B — OpenBMB's 2026-09 on-device model, official GGUF, 128K
///    ctx, bilingual; the instant-load option for weak machines.
/// All GGUF filenames verified against the HuggingFace API (no 404s).
pub const MODEL_CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "xuanyuan-6b-chat",
        name: "XuanYuan-6B-Chat 轩辕",
        params_b: 6.0,
        size_gb: 3.9,
        repo: "mradermacher/XuanYuan-6B-Chat-GGUF",
        filename: "XuanYuan-6B-Chat.Q4_K_M.gguf",
        note_zh: "金融领域专项：度小满中文金融模型，FinanceIQ 领先 · 本工作台首选",
        note_en: "Finance-domain: Du Xiaoman's Chinese finance LLM · workbench pick",
    },
    ModelInfo {
        id: "qwen3.5-4b",
        name: "Qwen3.5-4B",
        params_b: 4.0,
        size_gb: 2.5,
        repo: "unsloth/Qwen3.5-4B-GGUF",
        filename: "Qwen3.5-4B-Q4_K_M.gguf",
        note_zh: "通用最强基线（<5B），中英双语 · 证据引用更稳",
        note_en: "Best general baseline (<5B), zh/en · steadiest evidence quoting",
    },
    ModelInfo {
        id: "lfm3-1.7b",
        name: "LFM3-1.7B",
        params_b: 1.7,
        size_gb: 1.1,
        repo: "LiquidAI/LFM3-1.7B-GGUF",
        filename: "LFM3-1.7B-Q4_K_M.gguf",
        note_zh: "Liquid AI LFM3（2026-10 三混合架构）· 官方 GGUF · 秒级加载",
        note_en: "Liquid AI LFM3 (Oct 2026 tri-hybrid) · official GGUF · instant load",
    },
    ModelInfo {
        id: "qwen3.5-2b",
        name: "Qwen3.5-2B",
        params_b: 2.0,
        size_gb: 1.3,
        repo: "unsloth/Qwen3.5-2B-GGUF",
        filename: "Qwen3.5-2B-Q4_K_M.gguf",
        note_zh: "同门轻量版，任何机器可跑",
        note_en: "Lighter sibling, runs anywhere",
    },
    ModelInfo {
        id: "nemotron-3.5-bolt-e4b",
        name: "Nemotron 3.5 Bolt E4B",
        params_b: 4.0,
        size_gb: 4.7,
        repo: "unsloth/NVIDIA-Nemotron-3.5-Bolt-E4B-GGUF",
        filename: "NVIDIA-Nemotron-3.5-Bolt-E4B-UD-Q4_K_XL.gguf",
        note_zh: "NVIDIA 端侧多模态（图/视频/工具调用）· 128K · 6GB 显存需部分卸载",
        note_en: "NVIDIA edge multimodal (img/video/tools) · 128K · partial offload on 6GB",
    },
    ModelInfo {
        id: "phi-4-mini",
        name: "Phi-4-mini",
        params_b: 3.8,
        size_gb: 2.5,
        repo: "bartowski/microsoft_Phi-4-mini-instruct-GGUF",
        filename: "microsoft_Phi-4-mini-instruct-Q4_K_M.gguf",
        note_zh: "MIT 协议，英文推理强，中文较弱",
        note_en: "MIT license, strong English reasoning",
    },
];

pub fn model_by_id(id: &str) -> Option<&'static ModelInfo> {
    MODEL_CATALOG.iter().find(|m| m.id == id)
}

// ========================================================== device profile

#[derive(Clone, Debug, Default)]
pub struct DeviceProfile {
    pub vram_gb: Option<f32>,
    pub ram_gb: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Model + context fit fully in VRAM → fast.
    FitsVram,
    /// Runs, but layers spill to CPU RAM → slower.
    PartialOffload,
    /// Not recommended on this machine.
    TooBig,
}

impl DeviceProfile {
    /// Headroom for KV cache / activations at an 8k context.
    const OVERHEAD_GB: f32 = 1.5;

    pub fn tier_for_size(&self, size_gb: f32) -> Tier {
        let need = size_gb + Self::OVERHEAD_GB;
        if let Some(vram) = self.vram_gb {
            if need <= vram {
                return Tier::FitsVram;
            }
        }
        if let Some(ram) = self.ram_gb {
            if size_gb * 1.2 + 2.0 <= ram {
                return Tier::PartialOffload;
            }
        }
        Tier::TooBig
    }

    pub fn tier(&self, m: &ModelInfo) -> Tier {
        self.tier_for_size(m.size_gb)
    }
}

/// "6144 MiB" → 6.0
pub fn parse_nvidia_smi(out: &str) -> Option<f32> {
    let line = out.lines().next()?.trim();
    let mib = line.strip_suffix("MiB")?.trim().parse::<f32>().ok()?;
    Some(mib / 1024.0)
}

/// PowerShell prints raw bytes for TotalPhysicalMemory.
pub fn parse_ps_ram(out: &str) -> Option<f32> {
    let bytes = out.lines().next()?.trim().parse::<f64>().ok()?;
    Some((bytes / 1024.0 / 1024.0 / 1024.0) as f32)
}

/// Best-effort hardware probe; both sources may fail (no NVIDIA, no PS) → None.
pub fn detect_device() -> DeviceProfile {
    let vram_gb = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| parse_nvidia_smi(&s));
    let ram_gb = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| parse_ps_ram(&s));
    DeviceProfile { vram_gb, ram_gb }
}

// ========================================================== engine discovery

/// Testable core: first directory that contains llama-server(.exe) wins.
pub fn find_llama_server_in(dirs: &[PathBuf]) -> Option<PathBuf> {
    for d in dirs {
        for name in ["llama-server.exe", "llama-server"] {
            let p = d.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Layer 1 — instant: PATH, winget shims, the app's tools dir.
fn quick_search_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft/WinGet/Links"));
    }
    dirs.push(data_dir.join("tools"));
    dirs
}

/// Layer 2 — instant existence checks over the places llama.cpp lands by
/// convention (winget package payload, manually extracted zips, source builds).
fn known_engine_locations() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| out.push(p);
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        push(local.join("llama.cpp/bin"));
        push(local.join("Programs/llama.cpp/bin"));
        push(local.join("Microsoft/WinGet/Packages")); // scanned shallow by the deep pass
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        // Source builds: llama.cpp/build/bin[/Release].
        push(profile.join("llama.cpp/build/bin"));
        push(profile.join("llama.cpp/build/bin/Release"));
        push(profile.join("llama.cpp/bin"));
        push(profile.join(".lmstudio/extensions/backends"));
        push(profile.join("Downloads"));
    }
    for base in ["C:/Program Files", "C:/Program Files (x86)", "C:/"] {
        let base = PathBuf::from(base);
        push(base.join("llama.cpp/bin"));
        push(base.join("llama.cpp/build/bin"));
        push(base.join("llama.cpp/build/bin/Release"));
    }
    out
}

/// Direct file path from `WB_LLAMA_SERVER`, then quick dirs, then known spots.
/// The full-machine sweep runs separately in the background (layer 3).
pub fn find_llama_server(data_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WB_LLAMA_SERVER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = find_llama_server_in(&quick_search_dirs(data_dir)) {
        return Some(p);
    }
    // Known locations may name the binary directly or hold it one level down.
    for base in known_engine_locations() {
        if base.is_file() {
            return Some(base);
        }
        if let Some(p) = find_llama_server_in(&[base.clone()]) {
            return Some(p);
        }
    }
    None
}

/// Directory names the deep scan never descends into.
const DEEP_SCAN_SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "windows",
    "$recycle.bin",
    "system volume information",
    "appdata", // covered precisely by its own root below
];

/// Layer 3 — the full-machine sweep: bounded depth-first walk with a wall-clock
/// budget, run on a background thread. Covers the whole user profile (source
/// builds, extracted release zips anywhere under Downloads/Documents/projects),
/// every drive root, winget's package payload and LM Studio's bundled engines.
pub fn deep_scan_roots() -> Vec<(PathBuf, u32)> {
    let mut roots: Vec<(PathBuf, u32)> = Vec::new();
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        roots.push((profile.into(), 4));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        roots.push((local.join("Microsoft/WinGet/Packages"), 5));
        roots.push((local.join("Programs"), 3));
    }
    for letter in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:/", letter as char));
        if drive.is_dir() {
            roots.push((drive, 3));
        }
    }
    roots
}

/// What the layer-3 sweep found: the engine binary and every GGUF model file
/// already on disk (Ollama/LM Studio/local downloads).
#[derive(Clone, Debug, Default)]
pub struct DeepFindings {
    pub engine: Option<PathBuf>,
    pub models: Vec<DiscoveredModel>,
}

/// A GGUF model already present on this machine.
#[derive(Clone, Debug)]
pub struct DiscoveredModel {
    /// Display name (Ollama "name:tag", otherwise the file stem).
    pub name: String,
    pub path: PathBuf,
    pub size_gb: f32,
    pub source: ModelSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSource {
    Ollama,
    LMStudio,
    Disk,
}

impl ModelSource {
    pub fn label(&self) -> &'static str {
        match self {
            ModelSource::Ollama => "Ollama",
            ModelSource::LMStudio => "LM Studio",
            ModelSource::Disk => "disk",
        }
    }
}

/// Merge newly discovered models into the UI list, deduped by path.
/// Returns true when something was added.
pub fn merge_discovered(
    dst: &mut Vec<DiscoveredModel>,
    src: Vec<DiscoveredModel>,
) -> bool {
    let mut added = false;
    for m in src {
        if !dst.iter().any(|d| d.path == m.path) {
            dst.push(m);
            added = true;
        }
    }
    dst.sort_by(|a, b| a.name.cmp(&b.name));
    added
}

/// Iterative *.gguf walk with a depth budget, sharing the deep-skip rules.
fn walk_ggufs(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, String)>) {
    walk_ggufs_min(dir, depth, MIN_MODEL_BYTES, out)
}

fn walk_ggufs_min(dir: &Path, depth: u32, min_bytes: u64, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        if p.is_file() {
            if let Some(display) = real_model_file_at(&p, min_bytes) {
                out.push((p, display));
            }
        } else if depth > 0 && !DEEP_SCAN_SKIP.contains(&name.as_str()) {
            walk_ggufs_min(&p, depth - 1, min_bytes, out);
        }
    }
}

/// Quick local-model discovery (fast dirs only): Ollama's blob store via its
/// manifests, LM Studio's model tree, the app's models dir and Downloads.
pub fn quick_scan_local_models(models_dir: &Path) -> Vec<DiscoveredModel> {
    quick_scan_local_models_at(models_dir, MIN_MODEL_BYTES)
}

fn quick_scan_local_models_at(models_dir: &Path, min_bytes: u64) -> Vec<DiscoveredModel> {
    // (path, display name, source)
    let mut found: Vec<(PathBuf, String, ModelSource)> = Vec::new();
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        let mut ggufs = Vec::new();
        walk_ggufs_min(&profile.join(".lmstudio/models"), 4, min_bytes, &mut ggufs);
        for (p, name) in ggufs {
            found.push((p, name, ModelSource::LMStudio));
        }
        found.extend(scan_ollama_models_at(&profile.join(".ollama/models")));
        let mut ggufs = Vec::new();
        walk_ggufs_min(&profile.join("Downloads"), 2, min_bytes, &mut ggufs);
        for (p, name) in ggufs {
            found.push((p, name, ModelSource::Disk));
        }
    }
    // The app's own models dir: any GGUF that is not a catalog file.
    let mut ggufs = Vec::new();
    walk_ggufs_min(models_dir, 1, min_bytes, &mut ggufs);
    for (p, name) in ggufs {
        let is_catalog = MODEL_CATALOG
            .iter()
            .any(|m| p.file_name().map(|f| f == m.filename).unwrap_or(false));
        if !is_catalog {
            found.push((p, name, ModelSource::Disk));
        }
    }
    found
        .into_iter()
        .map(|(path, name, source)| {
            let size_gb = std::fs::metadata(&path)
                .map(|m| m.len() as f32 / 1024.0 / 1024.0 / 1024.0)
                .unwrap_or(0.0);
            DiscoveredModel { name, path, size_gb, source }
        })
        .collect()
}

fn file_stem_string(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

// ---------------------------------------------------------------- filters
// Model repos are littered with non-loadable GGUFs: vision projectors,
// importance matrices, shard fragments, aborted downloads. A local-model
// entry must pass every gate below or it does not get listed.

/// Files below this are not usable chat models (aux weights, stubs).
pub const MIN_MODEL_BYTES: u64 = 200 * 1024 * 1024;

/// If the stem ends in `-000NN-of-000MM`, returns (base_name, is_first_shard).
/// Only the first shard is a llama-server entry point - it finds the rest.
fn shard_info(stem: &str) -> Option<(String, bool)> {
    let idx = stem.rfind("-of-")?;
    let tail = &stem[idx + 4..];
    if tail.len() != 5 || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let before = &stem[..idx];
    let sidx = before.rfind('-')?;
    let shard = &before[sidx + 1..];
    if shard.len() != 5 || !shard.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((before[..sidx].to_string(), shard == "00001"))
}

/// Validate one path as a real, loadable model; returns its display name.
/// `min_bytes` is a parameter so tests can use tiny fixtures.
pub fn real_model_file_at(path: &Path, min_bytes: u64) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if !name.ends_with(".gguf") {
        return None;
    }
    if name.ends_with(".part") || name.ends_with(".tmp") || name.ends_with(".incomplete") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() < min_bytes {
        return None;
    }
    let stem = file_stem_string(path);
    let (base, is_entry) = match shard_info(&stem) {
        Some((base, first)) => (base, first),
        None => (stem.clone(), true),
    };
    if !is_entry {
        return None;
    }
    let low = base.to_lowercase();
    const AUXILIARY: &[&str] = &["mmproj", "imatrix", "dflash", "vocab", "lora", "adapter"];
    if AUXILIARY.iter().any(|tag| low.contains(tag)) {
        return None;
    }
    if !has_gguf_magic(path) {
        return None;
    }
    Some(base)
}

/// The file literally starts with the four GGUF magic bytes — placeholders,
/// truncated downloads and renamed junk fail here.
fn has_gguf_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    if f.read_exact(&mut magic).is_err() {
        return false;
    }
    &magic == b"GGUF"
}

pub fn real_model_file(path: &Path) -> Option<String> {
    real_model_file_at(path, MIN_MODEL_BYTES)
}

/// Ollama keeps GGUFs as sha256-named blobs; the manifests map them to
/// human names (`…/library/<model>/<tag>` → `<model>:<tag>`).
fn scan_ollama_models_at(root: &Path) -> Vec<(PathBuf, String, ModelSource)> {
    let mut out = Vec::new();
    let manifests = root.join("manifests");
    let mut jsons = Vec::new();
    walk_files(&manifests, 4, &mut jsons);
    for mf in jsons {
        let Ok(text) = std::fs::read_to_string(&mf) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let Some(layers) = v["layers"].as_array() else { continue };
        let Some(layer) = layers
            .iter()
            .find(|l| l["mediaType"].as_str().map(|m| m.contains("image.model")).unwrap_or(false))
        else {
            continue;
        };
        let Some(digest) = layer["digest"].as_str() else { continue };
        let hex = digest.trim_start_matches("sha256:");
        if hex.is_empty() {
            continue;
        }
        let blob = root.join("blobs").join(format!("sha256-{hex}"));
        if !blob.is_file() {
            continue;
        }
        // Even manifest-backed blobs get the magic gate — aborted
        // `ollama pull`s leave short, invalid files behind. (Blobs carry no
        // .gguf extension, so the full filter does not apply.)
        if !has_gguf_magic(&blob) {
            continue;
        }
        let rel = mf
            .strip_prefix(&manifests)
            .map(|p| p.components().collect::<Vec<_>>())
            .unwrap_or_default();
        let name = if rel.len() >= 2 {
            let model = rel[rel.len() - 2].as_os_str().to_string_lossy().to_string();
            let tag = rel[rel.len() - 1].as_os_str().to_string_lossy().to_string();
            format!("{model}:{tag}")
        } else {
            file_stem_string(&blob)
        };
        out.push((blob, name, ModelSource::Ollama));
    }
    out
}

fn walk_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() {
            out.push(p);
        } else if depth > 0 {
            walk_files(&p, depth - 1, out);
        }
    }
}

pub fn deep_scan(roots: Vec<(PathBuf, u32)>) -> DeepFindings {
    deep_scan_at(roots, MIN_MODEL_BYTES)
}

fn deep_scan_at(roots: Vec<(PathBuf, u32)>, min_bytes: u64) -> DeepFindings {
    const BUDGET: Duration = Duration::from_secs(30);
    const MAX_DIRS: usize = 60_000;
    let start = Instant::now();
    let mut stack = roots;
    let mut visited = 0usize;
    let mut findings = DeepFindings::default();
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited % 256 == 0 && (start.elapsed() > BUDGET || visited > MAX_DIRS) {
            return findings; // budget exhausted — report what we have
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if p.is_file() {
                if name == "llama-server.exe" || name == "llama-server" {
                    findings.engine = Some(p);
                } else if let Some(display) = real_model_file_at(&p, min_bytes) {
                    let size_gb = p
                        .metadata()
                        .map(|m| m.len() as f32 / 1024.0 / 1024.0 / 1024.0)
                        .unwrap_or(0.0);
                    findings.models.push(DiscoveredModel {
                        name: display,
                        path: p,
                        size_gb,
                        source: ModelSource::Disk,
                    });
                }
            } else if depth > 0 && !DEEP_SCAN_SKIP.contains(&name.as_str()) {
                stack.push((p, depth - 1));
            }
        }
    }
    findings
}

// ============================================================== downloading

#[derive(Clone, Debug)]
pub enum DownloadState {
    Running,
    Done,
    Failed(String),
    Cancelled,
}

pub struct DownloadJob {
    /// Live progress in bytes, updated by the watcher from the .part file.
    pub state: Arc<Mutex<DownloadState>>,
    pub got: Arc<Mutex<u64>>,
    pub dest: PathBuf,
    pub dest_part: PathBuf,
    pub child: Arc<Mutex<Option<std::process::Child>>>,
    pub cancel: Arc<AtomicBool>,
}

impl DownloadJob {
    pub fn state(&self) -> DownloadState {
        self.state.lock().map(|g| g.clone()).unwrap_or(DownloadState::Running)
    }
    pub fn got_bytes(&self) -> u64 {
        self.got.lock().map(|g| *g).unwrap_or(0)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Ok(mut g) = self.child.lock() {
            if let Some(c) = g.as_mut() {
                let _ = c.kill();
            }
        }
    }
}

/// Download via the Windows-bundled curl.exe (TLS without adding a Rust TLS
/// stack); a watcher thread finalizes the file and the state.
pub fn start_download(m: &ModelInfo, dir: &Path) -> Result<DownloadJob, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dest = dir.join(m.filename);
    let dest_part = dir.join(format!("{}.part", m.filename));
    let url = m.url();
    let curl = find_curl().ok_or("curl.exe not found")?;
    let child = std::process::Command::new(curl)
        .args(["-L", "--fail", "--silent", "--output"])
        .arg(&dest_part)
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;

    let job = DownloadJob {
        state: Arc::new(Mutex::new(DownloadState::Running)),
        got: Arc::new(Mutex::new(0)),
        dest,
        dest_part: dest_part.clone(),
        child: Arc::new(Mutex::new(Some(child))),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let watcher_part = dest_part.clone();
    let watcher_dest = job.dest.clone();
    let watcher_state = job.state.clone();
    let watcher_child = job.child.clone();
    let watcher_cancel = job.cancel.clone();
    std::thread::spawn(move || {
        // Wait for curl to exit.
        let ok = loop {
            {
                let mut g = watcher_child.lock().unwrap();
                if let Some(c) = g.as_mut() {
                    match c.try_wait() {
                        Ok(Some(st)) => break st.success(),
                        Ok(None) => {}
                        Err(_) => break false,
                    }
                } else {
                    break false;
                }
            }
            if watcher_cancel.load(Ordering::Relaxed) {
                break false;
            }
            std::thread::sleep(Duration::from_millis(300));
        };
        let final_state = if watcher_cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&watcher_part);
            DownloadState::Cancelled
        } else if ok && watcher_part.exists() {
            match std::fs::rename(&watcher_part, &watcher_dest) {
                Ok(()) => DownloadState::Done,
                Err(e) => DownloadState::Failed(e.to_string()),
            }
        } else {
            DownloadState::Failed("curl 下载失败（网络或链接不可用）".into())
        };
        *watcher_state.lock().unwrap() = final_state;
    });
    Ok(job)
}

fn find_curl() -> Option<PathBuf> {
    // Prefer System32's curl.exe; fall back to PATH via plain name.
    let sys = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32/curl.exe"));
    if let Some(p) = sys {
        if p.is_file() {
            return Some(p);
        }
    }
    Some(PathBuf::from("curl.exe"))
}

// ============================================================ llama-server

pub struct ServerHandle {
    pub port: u16,
    pub model_name: String,
    pub started_at: Instant,
}

/// Spawn llama-server; output goes to the log file. The child is parked in
/// `slot` so the app can kill it on quit / 停止.
pub fn spawn_llama_server(
    exe: &Path,
    model: &Path,
    model_name: String,
    port: u16,
    log_path: &Path,
    slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<ServerHandle, String> {
    let log = std::fs::File::create(log_path).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let child = std::process::Command::new(exe)
        .arg("-m")
        .arg(model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .args(["-ngl", "99", "-c", "8192"])
        .stdout(log)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("spawn llama-server: {e}"))?;
    *slot.lock().unwrap() = Some(child);
    Ok(ServerHandle {
        port,
        model_name,
        started_at: Instant::now(),
    })
}

// ============================================================ chat client
// The SSE client, message types and stream buffer live in `wb-core::llm`
// (pure networking, shared with the e2e suite); re-exported here for the UI.

pub use wb_core::llm::{chat_stream, server_listening, ChatMessage, StreamShared};

// ========================================================== context builder

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// System prompt injecting the currently open task: identity, CALC values,
/// guardrail flags, structural inconsistencies and the evidence excerpt.
/// The assistant is instructed to answer only from evidence — the same
/// discipline the guardrail enforces on the model under review.
pub fn build_system_context(
    lang: Lang,
    detail: &TaskDetail,
    guardrail: Option<&Guardrail>,
    structural: Option<&StructuralReport>,
    evidence: Option<&Evidence>,
) -> String {
    build_system_context_ext(lang, detail, guardrail, structural, evidence, &[], &[])
}

/// Extended context: adds the advisory signals (per-number provenance and
/// duplicate briefs) so the assistant can explain *why* a number is red, not
/// just that it is. Passing empty slices reproduces the original context
/// exactly, which is what the plain entry point does.
pub fn build_system_context_ext(
    lang: Lang,
    detail: &TaskDetail,
    guardrail: Option<&Guardrail>,
    structural: Option<&StructuralReport>,
    evidence: Option<&Evidence>,
    citations: &[wb_core::provenance::Citation],
    duplicates: &[wb_core::duplicate::Duplicate],
) -> String {
    let mut s = String::new();
    match lang {
        Lang::Zh => {
            s.push_str("你是 WB 财报审核工作台的内置审核助手。回答规则：\n");
            s.push_str("1. 只依据下方提供的证据与计算值作答，绝不编造数字；\n");
            s.push_str("2. 建议修改时引用公告原始数字，不要给出自己计算的新数字；\n");
            s.push_str("3. 简洁、面向审核决策。\n\n");
        }
        Lang::En => {
            s.push_str("You are the built-in review assistant of the WB financial review workbench. Rules:\n");
            s.push_str("1. Answer ONLY from the evidence and calculation values below; never invent numbers;\n");
            s.push_str("2. When suggesting edits, quote the announcement's original figures;\n");
            s.push_str("3. Be concise and decision-oriented.\n\n");
        }
    }
    match lang {
        Lang::Zh => s.push_str(&format!(
            "【当前任务】{} {}（{}）\n状态：{} · 简报 v{}\n",
            detail.object_name, detail.period, detail.task_id,
            detail.status, detail.brief_version.unwrap_or(1)
        )),
        Lang::En => s.push_str(&format!(
            "[Current task] {} {} ({})\nStatus: {} · brief v{}\n",
            detail.object_name, detail.period, detail.task_id,
            detail.status, detail.brief_version.unwrap_or(1)
        )),
    }
    let calc: Vec<String> = detail
        .sections
        .calculation
        .0
        .iter()
        .map(|(k, v)| format!("- {k} = {v}"))
        .collect();
    if !calc.is_empty() {
        match lang {
            Lang::Zh => s.push_str("【计算值（确定性工具产出）】\n"),
            Lang::En => s.push_str("[CALC values (deterministic tool output)]\n"),
        }
        s.push_str(&calc.join("\n"));
        s.push('\n');
    }
    if let Some(g) = guardrail {
        if !g.ok() {
            match lang {
                Lang::Zh => s.push_str(&format!(
                    "【数字护栏未通过】以下数字无法在证据中回溯：{}\n",
                    g.violation_tokens().join("、")
                )),
                Lang::En => s.push_str(&format!(
                    "[Guardrail FAILED] these numbers do not trace to evidence: {}\n",
                    g.violation_tokens().join(", ")
                )),
            }
        }
    }
    if let Some(sr) = structural {
        if !sr.ok() {
            match lang {
                Lang::Zh => s.push_str("【结构校验不一致】\n"),
                Lang::En => s.push_str("[Structural checks failed]\n"),
            }
            for v in &sr.violations {
                s.push_str(&format!("- {}（{}）\n", v.detail, v.rule));
            }
        }
    }
    if let Some(ev) = evidence {
        match lang {
            Lang::Zh => s.push_str(&format!(
                "【证据摘录】{}\n",
                truncate(&ev.excerpt, 1200)
            )),
            Lang::En => s.push_str(&format!(
                "[Evidence excerpt] {}\n",
                truncate(&ev.excerpt, 1200)
            )),
        }
    }
    if !citations.is_empty() {
        let untraced: Vec<&wb_core::provenance::Citation> =
            wb_core::provenance::untraced(citations);
        match lang {
            Lang::Zh => s.push_str("【数字溯源】\n"),
            Lang::En => s.push_str("[Number provenance]\n"),
        }
        for c in &untraced {
            match lang {
                Lang::Zh => s.push_str(&format!("- {} 无法溯源到任何证据\n", c.raw)),
                Lang::En => s.push_str(&format!("- {} traces to no evidence\n", c.raw)),
            }
        }
        for c in citations.iter().filter(|c| c.traced()).take(20) {
            if let Some(src_ref) = &c.source {
                let quote = truncate(&src_ref.quote, 120);
                match lang {
                    Lang::Zh => s.push_str(&format!(
                        "- {} ← {}（{}）{}\n",
                        c.raw,
                        src_ref.key,
                        src_ref.kind.as_str(),
                        quote
                    )),
                    Lang::En => s.push_str(&format!(
                        "- {} <- {} ({}) {}\n",
                        c.raw,
                        src_ref.key,
                        src_ref.kind.as_str(),
                        quote
                    )),
                }
            }
        }
    }
    if !duplicates.is_empty() {
        match lang {
            Lang::Zh => s.push_str("【重复简报】\n"),
            Lang::En => s.push_str("[Duplicate briefs]\n"),
        }
        for d in duplicates {
            s.push_str(&format!(
                "- {} ({}) {}\n",
                d.other_task_id,
                d.other_object_id,
                wb_core::duplicate::describe(d)
            ));
        }
    }
    match lang {
        Lang::Zh => s.push_str(&format!(
            "【模型草稿（待审核）】\n{}\n",
            truncate(&detail.sections.inference, 2000)
        )),
        Lang::En => s.push_str(&format!(
            "[Model draft (under review)]\n{}\n",
            truncate(&detail.sections.inference, 2000)
        )),
    }
    s
}

// ============================================================== chat state

#[derive(Clone, Debug, PartialEq)]
pub enum EngineState {
    Ready(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerState {
    Stopped,
    Starting,
    Ready,
    Failed(String),
}

/// All side-chat UI state; lives on Workspace as `pub chat: ChatState`.
pub struct ChatState {
    pub open: bool,
    pub input: Entity<TextInput>,
    pub messages: Vec<ChatMessage>,
    /// Partial assistant text while a stream is in flight.
    pub streaming: Option<String>,
    pub engine: Option<EngineState>,
    pub device: Option<DeviceProfile>,
    pub server_state: ServerState,
    pub server: Option<ServerHandle>,
    pub child_slot: Arc<Mutex<Option<std::process::Child>>>,
    pub downloads: BTreeMap<&'static str, DownloadJob>,
    pub stream: Option<Arc<StreamShared>>,
    pub error: Option<String>,
    /// Set once the engine/device probes ran (lazy, on first open).
    pub probed: bool,
    /// True while the model catalog (not the conversation) fills the panel.
    pub catalog_view: bool,
    device_slot: Arc<Mutex<Option<DeviceProfile>>>,
    /// One-shot health probe result slot (filled on the background executor).
    pub(crate) health_slot: Arc<Mutex<Option<bool>>>,
    /// Guards against piling up concurrent health probes.
    pub(crate) health_pending: Arc<AtomicBool>,
    /// Layer-3 full-machine sweep results: engine + every GGUF on disk.
    deep_scan_slot: Arc<Mutex<Option<DeepFindings>>>,
    /// True while the background sweep is walking the machine.
    pub deep_scanning: bool,
    /// Incoming quick-scan local models, drained by the tick loop.
    models_slot: Arc<Mutex<Vec<DiscoveredModel>>>,
    /// GGUF models already on this machine (Ollama / LM Studio / disk).
    pub local_models: Vec<DiscoveredModel>,
}

impl ChatState {
    /// The input entity is created by Workspace::new (it needs the i18n
    /// placeholder) and handed over here.
    pub fn from_input(input: Entity<TextInput>) -> ChatState {
        ChatState {
            open: false,
            input,
            messages: Vec::new(),
            streaming: None,
            engine: None,
            device: None,
            server_state: ServerState::Stopped,
            server: None,
            child_slot: Arc::new(Mutex::new(None)),
            downloads: BTreeMap::new(),
            stream: None,
            error: None,
            probed: false,
            catalog_view: false,
            device_slot: Arc::new(Mutex::new(None)),
            health_slot: Arc::new(Mutex::new(None)),
            health_pending: Arc::new(AtomicBool::new(false)),
            deep_scan_slot: Arc::new(Mutex::new(None)),
            deep_scanning: false,
            models_slot: Arc::new(Mutex::new(Vec::new())),
            local_models: Vec::new(),
        }
    }

    /// Lazy probe: quick engine checks are instant; the full-machine sweep
    /// runs on a background thread (only if the quick checks miss); device
    /// detection spawns nvidia-smi + PowerShell.
    pub fn ensure_probed(&mut self, data_dir: &Path, models_dir: &Path, cx: &mut gpui::Context<crate::workspace::Workspace>) {
        if self.probed {
            return;
        }
        self.probed = true;
        self.engine = find_llama_server(data_dir).map(EngineState::Ready);
        if self.engine.is_none() {
            self.start_deep_scan();
        }
        // Quick local-model discovery on a background thread.
        let mslot = self.models_slot.clone();
        let mdir = models_dir.to_path_buf();
        std::thread::spawn(move || {
            let found = quick_scan_local_models(&mdir);
            if let Ok(mut g) = mslot.lock() {
                g.extend(found);
            }
        });
        let slot = self.device_slot.clone();
        cx.background_executor()
            .spawn(async move {
                let d = detect_device();
                *slot.lock().unwrap() = Some(d);
            })
            .detach();
    }

    /// Launch (or relaunch) the layer-3 engine sweep.
    pub fn start_deep_scan(&mut self) {
        if self.deep_scanning {
            return;
        }
        self.deep_scanning = true;
        let slot = self.deep_scan_slot.clone();
        let mslot = self.models_slot.clone();
        std::thread::spawn(move || {
            let findings = deep_scan(deep_scan_roots());
            if let Ok(mut g) = mslot.lock() {
                g.extend(findings.models.clone());
            }
            *slot.lock().unwrap() = Some(findings);
        });
    }

    /// Pick up finished probes (device + engine sweep + local models); true
    /// when something changed.
    pub fn tick_probe(&mut self) -> bool {
        let mut changed = false;
        let polled = self.device_slot.lock().unwrap().take();
        if let Some(d) = polled {
            self.device = Some(d);
            changed = true;
        }
        let deep = self.deep_scan_slot.lock().unwrap().take();
        if let Some(findings) = deep {
            self.deep_scanning = false;
            if findings.engine.is_some() && self.engine.is_none() {
                self.engine = findings.engine.map(EngineState::Ready);
            }
            changed = true;
        }
        let incoming: Vec<DiscoveredModel> =
            self.models_slot.lock().map(|mut g| g.drain(..).collect()).unwrap_or_default();
        if !incoming.is_empty() && merge_discovered(&mut self.local_models, incoming) {
            changed = true;
        }
        changed
    }

    pub fn stop_stream(&self) {
        if let Some(s) = &self.stream {
            s.stop.store(true, Ordering::Relaxed);
        }
    }

    /// True while the device/engine/model probes are still running - the
    /// panel shows a spinner instead of the idle dot.
    pub fn probing(&self) -> bool {
        self.deep_scanning || (self.probed && self.device.is_none())
    }
}

// ============================================================== chat panel

impl crate::workspace::Workspace {
    /// The right-side AI assistant panel: engine onboarding → model catalog →
    /// conversation. 380px, always available regardless of the active view.
    pub fn render_chat_panel(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        use crate::theme::*;
        use crate::workspace::Btn;
        let ts = i18n::strings(self.lang);
        let close = self.button("", Btn::Ghost, Some("\u{E8BB}"), _window, cx, |ws, _e, _w, cx| {
            ws.chat.open = false;
            cx.notify();
        });
        let probing = self.chat.probing();
        let (dot, status): (Hsla, String) = match &self.chat.server_state {
            ServerState::Ready => (
                c(GREEN),
                format!(
                    "{} · {} :{}",
                    ts.server_ready_label,
                    self.chat
                        .server
                        .as_ref()
                        .map(|s| s.model_name.clone())
                        .unwrap_or_default(),
                    self.chat.server.as_ref().map(|s| s.port).unwrap_or(0)
                ),
            ),
            ServerState::Starting => (c(AMBER), ts.server_starting.to_string()),
            ServerState::Failed(e) => (c(RED), format!("{e}")),
            ServerState::Stopped => {
                if probing {
                    (c(ACCENT), ts.status_scanning.to_string())
                } else {
                    (c(FG3), ts.server_stopped.to_string())
                }
            }
        };

        let mut panel = div()
            .flex()
            .flex_col()
            .w(px(380.0))
            .min_w(px(380.0))
            .h_full()
            .bg(c(SURFACE))
            .border_l_1()
            .border_color(c(HAIRLINE));

        // Header: identity + engine/model status + close. The status row is
        // flex_1 so a long label ellipsizes instead of squeezing the buttons
        // (the "Stopp" bug).
        panel = panel.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .h(px(40.0))
                .px_3()
                .border_b_1()
                .border_color(c(HAIRLINE))
                .child(
                    div()
                        .text_size(px(T13))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(c(FG))
                        .child(ts.chat_title),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .when(
                            probing || self.chat.server_state == ServerState::Starting,
                            |el| el.child(spinner(dot, 11.0)),
                        )
                        .when(
                            !probing && self.chat.server_state != ServerState::Starting,
                            |el| el.child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(T11))
                                .font_family(FONT_MONO)
                                .text_color(c(FG2))
                                .truncate()
                                .child(status),
                        ),
                )
                .when(self.chat.engine.is_some(), |el| {
                    el.child(self.button(
                        ts.mi_models,
                        Btn::Ghost,
                        None,
                        _window,
                        cx,
                        |ws, _e, _w, cx| {
                            ws.chat.catalog_view = !ws.chat.catalog_view;
                            cx.notify();
                        },
                    ))
                })
                .child(close),
        );

        let engine_missing = self.chat.engine.is_none();
        let show_catalog =
            engine_missing == false && (self.chat.catalog_view || self.chat.server_state != ServerState::Ready);
        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .id("chat-scroll")
            .overflow_y_scroll()
            .p_3()
            .gap_3();

        if engine_missing {
            body = body.child(self.render_engine_onboarding(_window, cx));
        } else if show_catalog {
            body = body.child(self.render_model_catalog(_window, cx));
        } else {
            body = body.child(self.render_chat_messages(cx));
        }

        panel = panel.child(body);

        // Footer: context chip + error + input row.
        let ctx_label = if self.detail.is_some() {
            ts.chat_ctx_attached.to_string()
        } else {
            ts.chat_ctx_none.to_string()
        };
        let streaming = self.chat.stream.is_some();
        let mut footer = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .p_3()
            .pt_2()
            .border_t_1()
            .border_color(c(HAIRLINE))
            .child(
                div()
                    .text_size(px(T11))
                    .text_color(c(FG3))
                    .truncate()
                    .child(ctx_label),
            );
        if let Some(e) = &self.chat.error {
            footer = footer.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(c(RED_BG))
                    .text_size(px(T12))
                    .text_color(c(RED))
                    .child(e.clone()),
            );
        }
        let send_btn = if streaming {
            self.button(ts.chat_stop, Btn::Secondary, None, _window, cx, |ws, _e, _w, cx| {
                ws.chat.stop_stream();
                cx.notify();
            })
        } else {
            self.button(ts.chat_send, Btn::Primary, None, _window, cx, |ws, _e, _w, cx| {
                ws.chat_send(cx);
            })
        };
        footer = footer.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                // The input must shrink; unwrapped it would push the send
                // button off the panel edge.
                .child(div().flex_1().min_w_0().child(self.chat.input.clone()))
                .child(send_btn),
        );
        panel.child(footer)
    }

    /// Engine missing: winget command + releases link + re-check.
    fn render_engine_onboarding(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        use crate::theme::*;
        use crate::workspace::Btn;
        let ts = i18n::strings(self.lang);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_lg()
            .bg(c(RAISED))
            .border_1()
            .border_color(c(BORDER))
            .child(
                div()
                    .text_size(px(T13))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(c(AMBER))
                    .child(ts.engine_missing_title),
            )
            .when(self.chat.deep_scanning, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(spinner(c(ACCENT), 11.0))
                        .child(
                            div()
                                .text_size(px(T12))
                                .text_color(c(FG2))
                                .child(ts.engine_scanning),
                        ),
                )
            })
            .child(
                div()
                    .text_size(px(T12))
                    .text_color(c(FG2))
                    .child(ts.engine_missing_hint),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .py_1p5()
                            .rounded_sm()
                            .bg(c(PANEL2))
                            .font_family(FONT_MONO)
                            .text_size(px(T12))
                            .text_color(c(FG))
                            .child(ts.engine_cmd_hint),
                    )
                    .child(self.button("", Btn::Ghost, Some("\u{E8C8}"), _window, cx, |_ws, _e, _w, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            "winget install llama.cpp".to_string(),
                        ));
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(self.button(ts.engine_releases, Btn::Primary, Some("\u{E774}"), _window, cx, |_ws, _e, _w, cx| {
                        cx.open_url("https://github.com/ggml-org/llama.cpp/releases");
                    }))
                    .child(self.button(ts.engine_recheck, Btn::Secondary, None, _window, cx, |ws, _e, _w, cx| {
                        ws.chat_recheck(cx);
                    })),
            )
    }

    /// Curated 2–6B catalog, tiered against the detected device.
    fn render_model_catalog(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        use crate::theme::*;
        use crate::workspace::Btn;
        let ts = i18n::strings(self.lang);
        let models_dir = self.cfg.models_dir_or_default();
        let mut wrap = div().flex().flex_col().gap_2();
        wrap = wrap
            .child(
                div()
                    .text_size(px(T13))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(c(FG))
                    .child(ts.catalog_title),
            )
            .child(
                div()
                    .text_size(px(T11))
                    .text_color(c(FG3))
                    .child(ts.catalog_hint),
            );
        for m in MODEL_CATALOG {
            let dest = models_dir.join(m.filename);
            let downloaded = dest.is_file();
            let job = self.chat.downloads.get(m.id);
            let tier = self
                .chat
                .device
                .as_ref()
                .map(|d| d.tier(m))
                .unwrap_or(Tier::FitsVram);
            let (tier_label, tier_color) = match tier {
                Tier::FitsVram => (ts.model_fit, c(GREEN)),
                Tier::PartialOffload => (ts.model_partial, c(AMBER)),
                Tier::TooBig => (ts.model_heavy, c(RED)),
            };
            let active = self
                .chat
                .server
                .as_ref()
                .map(|s| s.model_name == m.name)
                .unwrap_or(false)
                && self.chat.server_state == ServerState::Ready;
            let mut card = div().flex().flex_col().gap_1p5().p_3().rounded_lg().bg(c(RAISED)).border_1().border_color(if active { c(ACCENT) } else { c(BORDER) });
            card = card
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(T13))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(c(FG))
                                .child(m.name.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(T11))
                                .font_family(FONT_MONO)
                                .text_color(c(FG3))
                                .child(format!("{:.1}B · ~{:.1}GB", m.params_b, m.size_gb)),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(T11))
                                .text_color(tier_color)
                                .child(tier_label.to_string()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(T11))
                        .text_color(c(FG2))
                        .child(m.note(self.lang).to_string()),
                );

            // Action row: download (with progress) → start / stop.
            let mid = m.id;
            let action: gpui::AnyElement = if active {
                self.button(ts.model_stop, Btn::DangerGhost, None, _window, cx, |ws, _e, _w, cx| {
                    ws.chat_stop_server(cx);
                })
                .into_any_element()
            } else if downloaded {
                let mid = m.id;
                self.button(ts.model_use, Btn::Primary, None, _window, cx, move |ws, _e, _w, cx| {
                    ws.chat_start_model(mid, cx);
                })
                .into_any_element()
            } else if let Some(job) = job {
                match job.state() {
                    DownloadState::Running => {
                        let got = job.got_bytes() as f64;
                        let total = (m.size_gb as f64) * 1024.0 * 1024.0 * 1024.0;
                        let pct = if total > 0.0 { ((got / total) * 100.0).clamp(0.0, 100.0) } else { 0.0 };
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(T11))
                                            .font_family(FONT_MONO)
                                            .text_color(c(ACCENT))
                                            .child(format!("{} {:.0}%", ts.model_downloading, pct)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .h(px(4.0))
                                            .rounded_full()
                                            .bg(c(RAISED_HOVER))
                                            .child(
                                                div()
                                                    .w(px((3.0 + pct * 2.0) as f32))
                                                    .h(px(4.0))
                                                    .rounded_full()
                                                    .bg(c(ACCENT)),
                                            ),
                                    ),
                            )
                            .child(self.button(
                                ts.chat_stop,
                                Btn::Ghost,
                                None,
                                _window,
                                cx,
                                move |ws, _e, _w, cx| {
                                    ws.chat_cancel_download(mid, cx);
                                },
                            ))
                            .into_any_element()
                    }
                    DownloadState::Failed(e) => div()
                        .text_size(px(T11))
                        .text_color(c(RED))
                        .child(e)
                        .into_any_element(),
                    _ => {
                        let mid = m.id;
                        self.button(ts.model_download, Btn::Secondary, Some("\u{E896}"), _window, cx, move |ws, _e, _w, cx| {
                            ws.chat_begin_download(mid, cx);
                        })
                        .into_any_element()
                    }
                }
            } else {
                let mid = m.id;
                self.button(ts.model_download, Btn::Secondary, Some("\u{E896}"), _window, cx, move |ws, _e, _w, cx| {
                    ws.chat_begin_download(mid, cx);
                })
                .into_any_element()
            };
            card = card.child(div().flex().flex_row().items_center().gap_2().child(action));
            wrap = wrap.child(card);
        }

        // Models already on this machine (Ollama / LM Studio / downloads).
        if !self.chat.local_models.is_empty() {
            wrap = wrap
                .child(
                    div()
                        .text_size(px(T13))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(c(FG))
                        .child(format!(
                            "{}（{}）",
                            ts.local_models_title,
                            self.chat.local_models.len()
                        )),
                )
                .child(
                    div()
                        .text_size(px(T11))
                        .text_color(c(FG3))
                        .child(ts.local_models_hint),
                );
            let models: Vec<DiscoveredModel> = self.chat.local_models.clone();
            for dm in models {
                let tier = self
                    .chat
                    .device
                    .as_ref()
                    .map(|d| d.tier_for_size(dm.size_gb))
                    .unwrap_or(Tier::FitsVram);
                let (tier_label, tier_color) = match tier {
                    Tier::FitsVram => (ts.model_fit, c(GREEN)),
                    Tier::PartialOffload => (ts.model_partial, c(AMBER)),
                    Tier::TooBig => (ts.model_heavy, c(RED)),
                };
                let row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(c(RAISED))
                    .border_1()
                    .border_color(c(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(T12))
                                    .text_color(c(FG))
                                    .truncate()
                                    .child(dm.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(T11))
                                    .font_family(FONT_MONO)
                                    .text_color(c(FG3))
                                    .truncate()
                                    .child(format!(
                                        "{} · {:.1}GB · {}",
                                        dm.source.label(),
                                        dm.size_gb,
                                        tier_label
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(T11))
                            .text_color(tier_color)
                            .child(""),
                    )
                    .child({
                        let path = dm.path.clone();
                        let name = dm.name.clone();
                        self.button(ts.model_use, Btn::Secondary, None, _window, cx,
                            move |ws, _e, _w, cx| {
                                let p = path.clone();
                                let n = name.clone();
                                ws.chat_start_local(p, n, cx);
                            },
                        )
                    });
                wrap = wrap.child(row);
            }
        }
        wrap
    }

    /// Conversation: past turns + the streaming partial.
    fn render_chat_messages(&mut self, _cx: &mut gpui::Context<Self>) -> gpui::Div {
        use crate::theme::*;
        let ts = i18n::strings(self.lang);
        let role_label = match self.lang {
            Lang::Zh => "助手",
            Lang::En => "Assistant",
        };
        let mut wrap = div().flex().flex_col().gap_2();
        if self.chat.messages.is_empty() && self.chat.streaming.is_none() {
            wrap = wrap.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(c(RAISED))
                    .border_1()
                    .border_color(c(BORDER))
                    .text_size(px(T12))
                    .text_color(c(FG2))
                    .child(ts.chat_empty_hint),
            );
        }
        let msgs: Vec<ChatMessage> = self.chat.messages.clone();
        for (i, msg) in msgs.iter().enumerate() {
            let (label, color) = if msg.role == "user" {
                ("你", c(ACCENT))
            } else {
                ("助手", c(GREEN))
            };
            let card = div()
                .id(sid(format!("chat-msg-{i}")))
                .flex()
                .flex_col()
                .gap_0p5()
                .p_2p5()
                .rounded_lg()
                .bg(if msg.role == "user" { c(SURFACE) } else { c(RAISED) })
                .border_1()
                .border_color(c(HAIRLINE))
                .child(
                    div()
                        .text_size(px(T11))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(color)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(T13))
                        .line_height(px(20.0))
                        .text_color(c(FG))
                        .child(msg.content.clone()),
                );
            wrap = wrap.child(card);
        }
        if let Some(partial) = &self.chat.streaming {
            wrap = wrap.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .p_2p5()
                    .rounded_lg()
                    .bg(c(RAISED))
                    .border_1()
                    .border_color(c(HAIRLINE))
                    .child(
                        div()
                            .text_size(px(T11))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(c(GREEN))
                            .child(format!("{} {}", role_label, ts.chat_thinking)),
                    )
                    .child(
                        div()
                            .text_size(px(T13))
                            .line_height(px(20.0))
                            .text_color(c(FG))
                            .child(format!("{partial}▍")),
                    ),
            );
        }
        wrap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but *real-looking* GGUF: correct magic, padded body.
    fn gguf_fixture() -> Vec<u8> {
        let mut v = b"GGUF".to_vec();
        v.extend(std::iter::repeat_n(0u8, 64));
        v
    }

   #[test]
    fn catalog_is_sane() {
        assert!(MODEL_CATALOG.len() >= 5);
        let ids: Vec<_> = MODEL_CATALOG.iter().map(|m| m.id).collect();
        assert!(model_by_id("qwen3.5-4b").is_some());
        for m in MODEL_CATALOG {
            assert!(m.url().starts_with("https://huggingface.co/"));
            assert!(m.filename.ends_with(".gguf"));
            assert!(m.size_gb > 0.5 && m.size_gb < 8.0, "{}", m.name);
            assert!(m.params_b >= 1.5 && m.params_b <= 6.0, "{}", m.name);
        }
        // The job-fit picks must be present and first.
        assert_eq!(MODEL_CATALOG[0].id, "xuanyuan-6b-chat");
        assert!(ids.contains(&"lfm3-1.7b"));
        assert!(ids.contains(&"nemotron-3.5-bolt-e4b"));
    }


    fn ctx_task() -> TaskDetail {
        let mut d = TaskDetail {
            task_id: "RT-T".into(),
            object_id: "S300806".into(),
            ..Default::default()
        };
        d.sections.inference = "营收24.83亿元，扣非低于归母约1652万元。".into();
        d
    }

    #[test]
    fn plain_context_is_unchanged_by_the_extended_entry_point() {
        let d = ctx_task();
        let plain = build_system_context(Lang::En, &d, None, None, None);
        let ext = build_system_context_ext(Lang::En, &d, None, None, None, &[], &[]);
        assert_eq!(plain, ext, "empty advisory slices must reproduce the old prompt");
    }

    #[test]
    fn untraced_numbers_reach_the_assistant() {
        let d = ctx_task();
        let ev = wb_core::provenance::LabelledEvidence {
            metrics: vec![("revenue".into(), 24.83)],
            ..Default::default()
        };
        let cites = wb_core::provenance::cite(
            &d.sections.inference,
            &ev,
            &wb_core::guardrail::GuardrailConfig::default(),
        );
        let ctx = build_system_context_ext(Lang::En, &d, None, None, None, &cites, &[]);
        assert!(ctx.contains("1652 traces to no evidence"), "got:\n{ctx}");
        assert!(ctx.contains("24.83 <- revenue"), "got:\n{ctx}");
    }

    #[test]
    fn duplicates_reach_the_assistant() {
        let d = ctx_task();
        let dup = wb_core::duplicate::Duplicate {
            kind: wb_core::duplicate::DuplicateKind::SameEvent,
            other_task_id: "RT-OTHER".into(),
            other_object_id: "S300806".into(),
            other_period: "2026Q3".into(),
            divergent: vec![("revenue".into(), 24.83, 26.5)],
            confidence: 1.0,
        };
        let ctx = build_system_context_ext(Lang::En, &d, None, None, None, &[], &[dup]);
        assert!(ctx.contains("RT-OTHER"), "got:\n{ctx}");
        assert!(ctx.contains("figures differ"), "got:\n{ctx}");
    }

    #[test]
    fn tier_matches_6gb_card() {
        let dev = DeviceProfile { vram_gb: Some(6.0), ram_gb: Some(16.0) };
        assert_eq!(dev.tier(model_by_id("xuanyuan-6b-chat").unwrap()), Tier::FitsVram);
        assert_eq!(dev.tier(model_by_id("qwen3.5-4b").unwrap()), Tier::FitsVram);
        assert_eq!(dev.tier(model_by_id("lfm3-1.7b").unwrap()), Tier::FitsVram);
        // Bolt E4B's UD-Q4_K_XL is 5.06GB — honest amber on a 6GB card.
        assert_eq!(
            dev.tier(model_by_id("nemotron-3.5-bolt-e4b").unwrap()),
            Tier::PartialOffload
        );
        let tiny = DeviceProfile { vram_gb: None, ram_gb: Some(8.0) };
        assert_eq!(tiny.tier(model_by_id("qwen3.5-4b").unwrap()), Tier::PartialOffload);
    }

    #[test]
    fn parses_device_probes() {
        assert_eq!(parse_nvidia_smi("6144 MiB\n"), Some(6.0));
        assert_eq!(parse_nvidia_smi("8192 MiB"), Some(8.0));
        assert_eq!(parse_nvidia_smi("nope"), None);
        let b = 15.9f64 * 1024.0 * 1024.0 * 1024.0;
        assert!((parse_ps_ram(&format!("{b}\n")).unwrap() - 15.9).abs() < 0.2);
    }

    #[test]
    fn engine_search_finds_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_llama_server_in(&[dir.clone()]), None);
        std::fs::write(dir.join("llama-server.exe"), b"stub").unwrap();
        assert_eq!(
            find_llama_server_in(&[dir]).as_ref().map(|p| p.file_name()),
            Some(Some(std::ffi::OsStr::new("llama-server.exe")))
        );
    }

    #[test]
    fn deep_scan_finds_nested_engine_and_models() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("tools/llama.cpp/build/bin");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("llama-server.exe"), b"stub").unwrap();
        let models_dir = tmp.path().join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(models_dir.join("qwen2.5-7b.Q4_K_M.gguf"), gguf_fixture()).unwrap();
        let findings = deep_scan_at(vec![(tmp.path().to_path_buf(), 4)], 0);
        let engine = findings.engine.expect("engine found in nested dir");
        assert_eq!(engine.file_name(), Some(std::ffi::OsStr::new("llama-server.exe")));
        assert_eq!(findings.models.len(), 1);
        assert_eq!(findings.models[0].name, "qwen2.5-7b.Q4_K_M");
        assert_eq!(findings.models[0].source, ModelSource::Disk);
        assert!(findings.models[0].size_gb > 0.0);
    }

    #[test]
    fn deep_scan_respects_skip_dirs_and_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let skipped = tmp.path().join("node_modules/pkg");
        std::fs::create_dir_all(&skipped).unwrap();
        std::fs::write(skipped.join("llama-server.exe"), b"stub").unwrap();
        std::fs::write(skipped.join("model.gguf"), b"stub").unwrap();
        let findings = deep_scan(vec![(tmp.path().to_path_buf(), 4)]);
        assert!(findings.engine.is_none(), "node_modules must not be scanned");
        assert!(findings.models.is_empty());
        // Depth 0: the engine sits at depth 1 → unreachable.
        let plain = tmp.path().join("bin");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("llama-server.exe"), b"stub").unwrap();
        assert!(deep_scan(vec![(tmp.path().to_path_buf(), 0)]).engine.is_none());
    }

    #[test]
    fn ollama_manifest_maps_blob_to_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mf_dir = root.join("manifests/registry.ollama.ai/library/qwen3");
        std::fs::create_dir_all(&mf_dir).unwrap();
        let blob = root.join("blobs/sha256-abc123");
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        std::fs::write(&blob, gguf_fixture()).unwrap();
        std::fs::write(
            mf_dir.join("4b"),
            r#"{"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:abc123"}]}"#,
        )
        .unwrap();
        let found = scan_ollama_models_at(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, "qwen3:4b");
        assert_eq!(found[0].0, blob);
        assert_eq!(found[0].2, ModelSource::Ollama);
    }

    #[test]
    fn merge_discovered_dedupes_by_path() {
        let m = |p: &str, n: &str| DiscoveredModel {
            name: n.into(),
            path: PathBuf::from(p),
            size_gb: 1.0,
            source: ModelSource::Disk,
        };
        let mut dst = vec![m("/a.gguf", "a")];
        assert!(merge_discovered(&mut dst, vec![m("/b.gguf", "b"), m("/a.gguf", "a2")]));
        assert_eq!(dst.len(), 2);
        assert!(!merge_discovered(&mut dst, vec![m("/a.gguf", "a")]));
    }

    #[test]
    fn quick_scan_finds_gguf_outside_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("custom-finance.Q4_K_M.gguf"), gguf_fixture()).unwrap();
        std::fs::write(tmp.path().join("Qwen3.5-4B-Q4_K_M.gguf"), gguf_fixture()).unwrap();
        std::fs::write(tmp.path().join("broken-download.gguf"), b"not a gguf").unwrap();
        let found = quick_scan_local_models_at(tmp.path(), 0);
        // The catalog file and the invalid file are not reported.
        assert_eq!(found.len(), 1, "{:?}", found);
        assert_eq!(found[0].name, "custom-finance.Q4_K_M");
        assert_eq!(found[0].source, ModelSource::Disk);
    }

    #[test]
    fn real_model_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let w = |name: &str, body: &[u8]| {
            let p = tmp.path().join(name);
            std::fs::write(&p, body).unwrap();
            p
        };
        let real = gguf_fixture();

        // Projectors, imatrix, shards, partials are never models.
        assert_eq!(real_model_file_at(&w("mmproj-BF16.gguf", &real), 0), None);
        assert_eq!(real_model_file_at(&w("imatrix_unsloth.gguf", &real), 0), None);
        assert_eq!(real_model_file_at(&w("dflash-x-Q8_0.gguf", &real), 0), None);
        assert_eq!(real_model_file_at(&w("model.gguf.part", &real), 0), None);
        assert_eq!(
            real_model_file_at(&w("model-00002-of-00003.gguf", &real), 0),
            None,
            "non-first shard"
        );

        // First shard is an entry point, shown under its base name.
        let shard1 = real_model_file_at(&w("big-model-00001-of-00003.gguf", &real), 0);
        assert_eq!(shard1.as_deref(), Some("big-model"));

        // A file without the GGUF magic is junk even with a model-ish name.
        assert_eq!(real_model_file_at(&w("fake.gguf", b"stub"), 0), None);

        // Size floor: same file rejected under a real floor.
        assert_eq!(real_model_file_at(&w("tiny.gguf", &real), MIN_MODEL_BYTES), None);
    }

    #[test]
    fn deep_scan_roots_are_well_formed() {
        let roots = deep_scan_roots();
        assert!(!roots.is_empty(), "at least the user profile should be a root");
        assert!(roots.iter().all(|(_, d)| *d > 0 && *d <= 5));
    }

    #[test]
    fn context_builder_carries_task_facts() {
        let mut detail = TaskDetail::default();
        detail.object_name = "斯迪克".into();
        detail.period = "2026Q4".into();
        detail.status = "WAITING_REVIEW".into();
        detail.sections.inference = "净利率约2.7%；扣非低于归母约1652万元。".into();
        detail
            .sections
            .calculation
            .0
            .insert("revenue".into(), serde_json::json!(24.83));
        let g = wb_core::guardrail::check(
            &detail.sections.inference,
            &[24.83, 2.7],
            &Default::default(),
        );
        assert!(!g.ok());
        let ctx = build_system_context(Lang::Zh, &detail, Some(&g), None, None);
        assert!(ctx.contains("斯迪克"));
        assert!(ctx.contains("revenue = 24.83"));
        assert!(ctx.contains("1652"));
        assert!(ctx.contains("绝不编造数字"));
        let en = build_system_context(Lang::En, &detail, Some(&g), None, None);
        assert!(en.contains("never invent numbers"));
    }

    #[test]
    fn truncate_is_char_boundary_safe() {
        let s = truncate("深交所互动易公开问答关于斯迪克的一切", 10);
        assert!(s.ends_with('…'));
        assert!(!s.contains(char::REPLACEMENT_CHARACTER));
    }
}
