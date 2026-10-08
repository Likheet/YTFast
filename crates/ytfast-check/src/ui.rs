//! Plain terminal output: step headings, indented lines, one progress line
//! that rewrites itself, and simple questions.

use std::io::{BufRead, IsTerminal, Write};

use crate::report::Outcome;

pub fn heading(number: usize, total: usize, title: &str) {
    println!();
    println!("Step {number} of {total}: {title}");
}

/// A heading for something done after the numbered steps.
pub fn extra_heading(title: &str) {
    println!();
    println!("{title}");
}

pub fn say(text: &str) {
    for line in text.lines() {
        println!("   {line}");
    }
}

pub fn result(outcome: Outcome, text: &str) {
    let mut lines = text.lines();
    if let Some(first) = lines.next() {
        println!("   {} {first}", outcome.tag());
    }
    for line in lines {
        println!("          {line}");
    }
}

/// Rewrites the current line (for download progress).
pub fn progress(text: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\r   {text:<70}");
    let _ = out.flush();
}

pub fn end_progress() {
    println!();
}

pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Asks a question and returns the trimmed answer, or `None` at the end of
/// input.
pub fn ask(question: &str) -> Option<String> {
    print!("   {question} ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}
