use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What happened to a tool after building it.
enum Outcome {
    NewlyInstalled,
    Updated,
    Unchanged,
    Failed(String),
}

struct ToolResult {
    name: String,
    outcome: Outcome,
    /// Absolute path to the release binary (when it could be determined).
    binary: Option<PathBuf>,
}

fn main() {
    let base = match base_dir() {
        Some(b) => b,
        None => {
            eprintln!("Could not resolve a base directory (set $HOME or pass a path).");
            std::process::exit(1);
        }
    };

    if !base.is_dir() {
        eprintln!("Base directory does not exist: {}", base.display());
        std::process::exit(1);
    }

    println!("Installing tools from {}\n", base.display());

    let tools = discover_tools(&base);
    if tools.is_empty() {
        println!("No Rust tools (directories with a Cargo.toml) found.");
        return;
    }

    let mut results: Vec<ToolResult> = Vec::new();
    for dir in tools {
        results.push(process_tool(&dir));
    }

    // Ensure aliases for every tool that built successfully.
    let created_aliases = ensure_aliases(&results);

    print_summary(&results, &created_aliases);
}

/// Resolve the base directory: optional CLI arg overrides the default `~/code/tools`.
fn base_dir() -> Option<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        return Some(PathBuf::from(arg));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join("code").join("tools"))
}

/// A tool is any direct subdirectory containing a `Cargo.toml`.
fn discover_tools(base: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", base.display());
            return vec![];
        }
    };
    let mut tools = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("Cargo.toml").is_file() {
            tools.push(path);
        }
    }
    tools.sort();
    tools
}

/// Build one tool and classify the result by comparing the binary before/after.
fn process_tool(dir: &Path) -> ToolResult {
    let cargo_toml = dir.join("Cargo.toml");
    let name = match package_name(&cargo_toml) {
        Some(n) => n,
        None => {
            let fallback = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string());
            return ToolResult {
                name: fallback,
                outcome: Outcome::Failed("could not parse package name from Cargo.toml".into()),
                binary: None,
            };
        }
    };

    let binary = dir.join("target").join("release").join(&name);

    println!("==> Building {name}");

    let before = hash_file(&binary);

    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(dir)
        .status();

    let outcome = match status {
        Ok(s) if s.success() => {
            let after = hash_file(&binary);
            match (before, after) {
                (_, None) => Outcome::Failed("build succeeded but binary not found".into()),
                (None, Some(_)) => Outcome::NewlyInstalled,
                (Some(b), Some(a)) if b != a => Outcome::Updated,
                (Some(_), Some(_)) => Outcome::Unchanged,
            }
        }
        Ok(s) => Outcome::Failed(format!("cargo build exited with {s}")),
        Err(e) => Outcome::Failed(format!("failed to run cargo: {e}")),
    };

    println!();

    ToolResult {
        name,
        outcome,
        binary: Some(binary),
    }
}

/// Hand-parse `name = "..."` from the `[package]` section of a Cargo.toml.
fn package_name(cargo_toml: &Path) -> Option<String> {
    let content = std::fs::read_to_string(cargo_toml).ok()?;
    let mut in_package = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let value = rest.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Hash a file's bytes; `None` if it doesn't exist or can't be read.
fn hash_file(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

/// Append zsh aliases for tools that built successfully and don't already have one.
/// Returns the names of aliases that were created.
fn ensure_aliases(results: &[ToolResult]) -> Vec<String> {
    let zshrc = match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".zshrc"),
        Err(_) => {
            eprintln!("Could not resolve $HOME; skipping alias setup.");
            return Vec::new();
        }
    };

    let existing = std::fs::read_to_string(&zshrc).unwrap_or_default();

    let mut new_lines = Vec::new();
    let mut created = Vec::new();

    for r in results {
        // Only alias tools that produced a usable binary.
        let binary = match (&r.outcome, &r.binary) {
            (Outcome::Failed(_), _) | (_, None) => continue,
            (_, Some(b)) => b,
        };
        if alias_exists(&existing, &r.name) {
            continue;
        }
        new_lines.push(format!("alias {}=\"{}\"", r.name, binary.display()));
        created.push(r.name.clone());
    }

    if new_lines.is_empty() {
        return created;
    }

    let mut block = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        block.push('\n');
    }
    block.push_str("\n# toolinstall managed aliases\n");
    block.push_str(&new_lines.join("\n"));
    block.push('\n');

    let updated = existing + &block;
    if let Err(e) = std::fs::write(&zshrc, updated) {
        eprintln!("Failed to update {}: {e}", zshrc.display());
        return Vec::new();
    }

    created
}

/// True if `~/.zshrc` already defines `alias <name>=...` (tolerating leading whitespace).
fn alias_exists(zshrc: &str, name: &str) -> bool {
    let prefix = format!("alias {name}=");
    zshrc
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

fn print_summary(results: &[ToolResult], created_aliases: &[String]) {
    let collect = |f: &dyn Fn(&Outcome) -> bool| -> Vec<&str> {
        results
            .iter()
            .filter(|r| f(&r.outcome))
            .map(|r| r.name.as_str())
            .collect()
    };

    let newly = collect(&|o| matches!(o, Outcome::NewlyInstalled));
    let updated = collect(&|o| matches!(o, Outcome::Updated));
    let unchanged = collect(&|o| matches!(o, Outcome::Unchanged));

    println!("==================== Summary ====================");
    print_group("Newly installed", &newly);
    print_group("Updated", &updated);
    print_group("Unchanged", &unchanged);

    let failed: Vec<(&str, &str)> = results
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Failed(msg) => Some((r.name.as_str(), msg.as_str())),
            _ => None,
        })
        .collect();
    if !failed.is_empty() {
        println!("Failed:");
        for (name, msg) in &failed {
            println!("  - {name}: {msg}");
        }
    }

    if created_aliases.is_empty() {
        println!("Aliases created: none");
    } else {
        println!("Aliases created: {}", created_aliases.join(", "));
        println!("  Run `source ~/.zshrc` (or open a new shell) to use them.");
    }
}

fn print_group(label: &str, names: &[&str]) {
    if names.is_empty() {
        println!("{label}: none");
    } else {
        println!("{label}: {}", names.join(", "));
    }
}
