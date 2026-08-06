//! Terminal prompts shared by the onboarding flows.

use anyhow::Context as _;
use std::io::Write as _;

/// Read one answer. `None` means end of input, which ends the flow rather than
/// looping forever against a closed stdin.
pub fn prompt(question: &str, default: Option<&str>) -> anyhow::Result<Option<String>> {
    match default {
        Some(value) => print!("{question} [{value}]: "),
        None => print!("{question}: "),
    }
    std::io::stdout().flush().context("flushing the prompt")?;

    let mut line = String::new();
    if std::io::stdin()
        .read_line(&mut line)
        .context("reading the answer")?
        == 0
    {
        println!();
        return Ok(None);
    }
    let answer = line.trim().to_string();
    if answer.is_empty() {
        return Ok(default.map(str::to_string));
    }
    Ok(Some(answer))
}

pub fn confirm(question: &str, default_yes: bool) -> anyhow::Result<bool> {
    let default = if default_yes { "Y/n" } else { "y/N" };
    let Some(answer) = prompt(question, Some(default))? else {
        return Ok(false);
    };
    if answer == default {
        return Ok(default_yes);
    }
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}
