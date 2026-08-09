use std::io::{self, IsTerminal, Write};

use runtime::{save_user_provider_settings, ConfigLoader, RuntimeProviderConfig};

use serde_json;

/// The only provider. Persisted so config written by the wizard stays
/// self-describing rather than relying on an implicit default.
const PROVIDER_KIND: &str = "ollama";

pub fn run_setup_wizard() -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        return Err("setup wizard requires an interactive terminal".into());
    }

    let current = load_current_provider_config();

    println!();
    println!("  \x1b[1mDisco Code Setup\x1b[0m");
    println!("  Inference runs on a local Ollama daemon. There is no account and no API key.");
    println!("  Press Enter to keep the current value.\n");

    let host = prompt_host(&current)?;
    // Discovery must happen after the host is settled, or it would offer models
    // from the wrong daemon.
    if let Some(host) = &host {
        std::env::set_var("OLLAMA_HOST", host);
    }
    let available = api::ollama_resolve();
    let installed = discover_models();

    let model = prompt_model(&current, &installed)?;
    let fast_model = prompt_fast_model(&current, model.as_deref(), &installed)?;

    // The daemon takes no credential; an empty key keeps the stored shape valid.
    save_user_provider_settings(PROVIDER_KIND, "", host.as_deref(), model.as_deref())?;

    if let Some(fast) = &fast_model {
        save_settings_field("subagentModel", fast)?;
    }

    println!();
    println!("  \x1b[32mSaved to ~/.claw/settings.json\x1b[0m");
    if installed.is_empty() {
        println!(
            "  \x1b[33mNo models were found. Start Ollama and run `ollama pull qwen3.5:9b`.\x1b[0m"
        );
    } else {
        println!(
            "  Run \x1b[1m/model {}\x1b[0m or restart claw to activate.",
            model.as_deref().unwrap_or(available.as_str())
        );
    }
    println!();

    Ok(())
}

/// Asks the daemon which models are actually installed.
///
/// An empty result is not an error: it means Ollama is down or has nothing
/// pulled, and the wizard should still let the user record a model name they
/// intend to pull afterwards.
fn discover_models() -> Vec<String> {
    api::ollama_tags_blocking().unwrap_or_default()
}

fn load_current_provider_config() -> RuntimeProviderConfig {
    let cwd = std::env::current_dir().unwrap_or_default();
    ConfigLoader::default_for(&cwd)
        .load()
        .map(|c| c.provider().clone())
        .unwrap_or_default()
}

fn prompt_host(
    current: &RuntimeProviderConfig,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let default_url = api::OLLAMA_DEFAULT_HOST;
    let current_url = current.base_url().unwrap_or(default_url);
    let display = if current_url.is_empty() {
        default_url.to_string()
    } else {
        current_url.to_string()
    };

    if std::env::var("OLLAMA_HOST").is_ok_and(|v| !v.trim().is_empty()) {
        println!("  OLLAMA_HOST is set in the environment and takes priority over stored config.");
    }

    let input = read_line(&format!("  Ollama host [{display}]: "))?;
    if input.trim().is_empty() {
        if current_url == default_url || current_url.is_empty() {
            Ok(None)
        } else {
            Ok(Some(current_url.to_string()))
        }
    } else {
        Ok(Some(input.trim().to_string()))
    }
}

fn prompt_model(
    current: &RuntimeProviderConfig,
    installed: &[String],
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let current_model = current
        .model()
        .unwrap_or(installed.first().map_or("", String::as_str));

    println!("  \x1b[1mModel\x1b[0m");
    if installed.is_empty() {
        println!("    No models found on the daemon. Pull one with `ollama pull qwen3.5:9b`.");
    } else {
        for (index, name) in installed.iter().enumerate() {
            let marker = if name == current_model {
                " (current)"
            } else {
                ""
            };
            println!("    [{}] {name}{marker}", index + 1);
        }
        println!("    Enter a number, or type any model name.");
    }

    let input = read_line(&format!("  Model [{current_model}]: "))?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(if current_model.is_empty() {
            None
        } else {
            Some(current_model.to_string())
        });
    }
    Ok(Some(select(trimmed, installed)))
}

fn prompt_fast_model(
    current: &RuntimeProviderConfig,
    main_model: Option<&str>,
    installed: &[String],
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let _ = current;
    println!();
    println!("  \x1b[1mFast Model (for Agent subtasks)\x1b[0m");
    println!("    A smaller model used by the Agent tool when spawning Explore,");
    println!("    Plan, or Verification sub-agents. On local hardware this matters");
    println!("    more than it does in the cloud: a smaller model frees VRAM and");
    println!("    returns information-gathering results far sooner.");
    println!("    Press Enter to skip (agents will use your main model).");

    let current_fast = load_current_settings_field("subagentModel");
    let default_hint = current_fast.as_deref().or(main_model).unwrap_or("");

    let input = read_line(&format!(
        "  Fast model [{}]: ",
        if default_hint.is_empty() {
            "same as main"
        } else {
            default_hint
        }
    ))?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(current_fast)
    } else {
        Ok(Some(select(trimmed, installed)))
    }
}

/// Resolves what the user typed at a model prompt.
///
/// A bare number picks from the discovered list; anything else is taken
/// literally so a model that has not been pulled yet can still be recorded.
fn select(input: &str, installed: &[String]) -> String {
    input
        .parse::<usize>()
        .ok()
        .filter(|index| *index >= 1)
        .and_then(|index| installed.get(index - 1))
        .cloned()
        .unwrap_or_else(|| input.to_string())
}
fn load_current_settings_field(field: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let settings_path = std::path::Path::new(&home).join(".claw/settings.json");
    let content = std::fs::read_to_string(&settings_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get(field)?.as_str().map(|s| s.to_string())
}

fn save_settings_field(field: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let settings_dir = std::path::Path::new(&home).join(".claw");
    let settings_path = settings_dir.join("settings.json");

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content)?
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            field.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    std::fs::create_dir_all(&settings_dir)?;
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}

fn read_line(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut stdout = io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    Ok(buffer)
}
