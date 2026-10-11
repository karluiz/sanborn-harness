//! Small stderr-only renderer. No threads, model calls or terminal-state changes.

use std::io::{self, IsTerminal, Write};
use std::time::Instant;

use sanborn_core::plan::PlanEvent;
use sanborn_laya::Decision;

#[derive(Clone, Copy)]
pub struct Style {
    pub animate: bool,
    pub color: bool,
}

impl Style {
    pub fn detect(plain: bool, json: bool) -> Self {
        Self::from_environment(
            io::stderr().is_terminal(),
            std::env::var("TERM").ok().as_deref(),
            std::env::var_os("NO_COLOR").is_some(),
            plain,
            json,
        )
    }

    fn from_environment(
        tty: bool,
        term: Option<&str>,
        no_color: bool,
        plain: bool,
        json: bool,
    ) -> Self {
        let interactive = tty && term != Some("dumb") && !plain && !json;
        Self {
            animate: interactive,
            color: interactive && !no_color,
        }
    }
}

pub struct Feedback<W: Write> {
    writer: W,
    style: Style,
    json: bool,
    phase: Option<String>,
    frame: usize,
    started: Instant,
    error: Option<io::Error>,
}

impl<W: Write> Feedback<W> {
    pub fn new(writer: W, style: Style, json: bool) -> Self {
        Self {
            writer,
            style,
            json,
            phase: None,
            frame: 0,
            started: Instant::now(),
            error: None,
        }
    }

    fn write(&mut self, text: &str) {
        if self.error.is_none() {
            if let Err(error) = self
                .writer
                .write_all(text.as_bytes())
                .and_then(|_| self.writer.flush())
            {
                self.error = Some(error);
            }
        }
    }

    fn clear(&mut self) {
        if self.style.animate && self.phase.is_some() {
            self.write("\r\x1b[2K");
        }
    }

    fn line(&mut self, marker: &str, text: &str, code: &str) {
        self.clear();
        let text = safe_label(text);
        if self.style.color {
            self.write(&format!("\x1b[{code}m{marker}\x1b[0m {text}\n"));
        } else {
            self.write(&format!("{marker} {text}\n"));
        }
        self.tick();
    }

    pub fn phase(&mut self, text: &str) {
        if self.json {
            return;
        }
        self.clear();
        self.phase = Some(safe_label(text));
        self.frame = 0;
        if self.style.animate {
            self.tick();
        } else {
            self.write(&format!(".. {}\n", safe_label(text)));
        }
    }

    pub fn tick(&mut self) {
        if !self.style.animate || self.json {
            return;
        }
        if let Some(phase) = &self.phase {
            let frame = ["|", "/", "-", "\\"][self.frame % 4];
            let text = format!(
                "{frame} {phase} [{:.1}s]",
                self.started.elapsed().as_secs_f32()
            );
            if self.style.color {
                self.write(&format!("\r\x1b[2K\x1b[32m{text}\x1b[0m"));
            } else {
                self.write(&format!("\r\x1b[2K{text}"));
            }
            self.frame += 1;
        }
    }

    pub fn event(&mut self, event: &PlanEvent) {
        if self.json {
            match serde_json::to_string(event) {
                Ok(line) => self.write(&format!("{line}\n")),
                Err(error) => self.error = Some(io::Error::other(error)),
            }
            return;
        }
        match event {
            PlanEvent::ContractStarted { specialty } => self.line(
                ">",
                &format!("Contract: {specialty} - Rust steps, Laya gates"),
                "36",
            ),
            PlanEvent::Summoned { count } => {
                self.line(
                    ">",
                    &format!(
                        "Summoning {count} {} - read-only, Laya-guided",
                        if *count == 1 { "tadpole" } else { "tadpoles" }
                    ),
                    "36",
                );
                self.phase("Tadpoles investigating");
            }
            PlanEvent::Croak { id, status } => {
                self.line(
                    if *status == "blocked" { "!" } else { "+" },
                    &format!("Croak [{id}]: {status}"),
                    if *status == "blocked" { "33" } else { "32" },
                );
            }
            PlanEvent::Tadpole { id, event } => match event.as_ref() {
                PlanEvent::ContractStarted { specialty } => {
                    self.line(">", &format!("Tadpole [{id}] contract: {specialty}"), "36")
                }
                PlanEvent::ReasoningStarted { turn } => {
                    self.phase(&format!("Tadpole [{id}] reasoning - turn {turn}"))
                }
                PlanEvent::LayaDecision { result, .. } => match result {
                    Decision::ContinuePlannedStep { .. } => {}
                    Decision::Replan { .. } => {
                        self.line("!", &format!("Tadpole [{id}] Laya replan"), "33")
                    }
                    Decision::Absent { .. } => {
                        self.line("?", &format!("Tadpole [{id}] Laya absent"), "33")
                    }
                },
                PlanEvent::ToolFinished { tool, is_error, .. } => self.line(
                    if *is_error { "!" } else { "+" },
                    &format!(
                        "Tadpole [{id}] {tool}: {}",
                        if *is_error { "error" } else { "ok" }
                    ),
                    if *is_error { "31" } else { "32" },
                ),
                _ => {}
            },
            PlanEvent::ReasoningStarted { turn } => self.phase(&format!("Reasoning - turn {turn}")),
            PlanEvent::PlanAccepted { steps } => {
                self.line(">", &format!("Laya plan: {steps} steps"), "36");
                self.phase("Executing guarded plan");
            }
            PlanEvent::LayaDecision {
                step,
                result,
                stage,
                ..
            } => match result {
                Decision::ContinuePlannedStep { .. } => {
                    if *stage == "schedule" {
                        self.phase(&format!("Laya -> {}", safe_label(step)));
                    }
                }
                Decision::Replan { reason } => {
                    self.line("!", &format!("Laya replan: {reason}"), "33")
                }
                Decision::Absent { reason } => {
                    self.line("?", &format!("Laya absent: {reason}"), "33")
                }
            },
            PlanEvent::ToolFinished {
                tool,
                step,
                is_error,
                duration_ms,
            } => self.line(
                if *is_error { "!" } else { "+" },
                &format!(
                    "{tool} [{step}]: {} ({duration_ms} ms)",
                    if *is_error { "error" } else { "ok" }
                ),
                if *is_error { "31" } else { "32" },
            ),
            PlanEvent::Finished {
                turns,
                tool_calls,
                usage,
                duration_ms,
            } => {
                self.stop();
                self.line("+", &format!(
                    "Finished: {turns} reasoning turns, {tool_calls} tools, {} in / {} out tokens, {duration_ms} ms",
                    usage.input_tokens, usage.output_tokens), "32");
            }
        }
    }

    pub fn stop(&mut self) {
        self.clear();
        self.phase = None;
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.stop();
        match self.error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl<W: Write> Drop for Feedback<W> {
    fn drop(&mut self) {
        self.stop();
    }
}

fn safe_label(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(100)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_pipes_no_color_dumb_and_json() {
        for style in [
            Style::from_environment(false, Some("xterm"), false, false, false),
            Style::from_environment(true, Some("dumb"), false, false, false),
            Style::from_environment(true, Some("xterm"), false, true, false),
            Style::from_environment(true, Some("xterm"), false, false, true),
        ] {
            assert!(!style.color && !style.animate);
        }
        let no_color = Style::from_environment(true, Some("xterm"), true, false, false);
        assert!(!no_color.color && no_color.animate);
    }

    #[test]
    fn plain_has_no_escapes_and_json_has_only_events() {
        let style = Style {
            animate: false,
            color: false,
        };
        let mut plain = Feedback::new(Vec::new(), style, false);
        plain.phase("Reasoning");
        plain.event(&PlanEvent::ToolFinished {
            step: "probe".into(),
            tool: "read_file".into(),
            is_error: false,
            duration_ms: 1,
        });
        plain.finish().unwrap();
        let output = String::from_utf8(plain.writer.clone()).unwrap();
        assert!(!output.contains('\x1b') && !output.contains('\r'));
        assert!(output.contains("read_file"));
        let mut json = Feedback::new(Vec::new(), style, true);
        json.phase("hidden");
        json.tick();
        json.event(&PlanEvent::ReasoningStarted { turn: 1 });
        json.finish().unwrap();
        let output = String::from_utf8(json.writer.clone()).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap()["turn"],
            1
        );
    }

    #[test]
    fn animated_cleanup_sanitizes_labels_and_reports_write_failures() {
        let mut progress = Feedback::new(
            Vec::new(),
            Style {
                animate: true,
                color: true,
            },
            false,
        );
        progress.phase("read\n\x1b[31m");
        progress.tick();
        progress.finish().unwrap();
        assert!(progress.phase.is_none());
        assert!(progress.writer.ends_with(b"\r\x1b[2K"));
        assert_eq!(safe_label("bad\n\r\x1b"), "bad");
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("fixture"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut broken = Feedback::new(
            Broken,
            Style {
                animate: false,
                color: false,
            },
            false,
        );
        broken.phase("test");
        assert!(broken.finish().is_err());
    }

    #[test]
    fn tadpole_feedback_preserves_plain_and_nested_json_contracts() {
        let events = [
            PlanEvent::Summoned { count: 1 },
            PlanEvent::Tadpole {
                id: "scout".into(),
                event: Box::new(PlanEvent::ReasoningStarted { turn: 1 }),
            },
            PlanEvent::Croak {
                id: "scout".into(),
                status: "returned",
            },
        ];
        let style = Style {
            animate: false,
            color: false,
        };
        let mut plain = Feedback::new(Vec::new(), style, false);
        let mut json = Feedback::new(Vec::new(), style, true);
        for event in &events {
            plain.event(event);
            json.event(event);
        }
        plain.finish().unwrap();
        json.finish().unwrap();
        let text = String::from_utf8(plain.writer.clone()).unwrap();
        assert!(text.contains("Summoning 1 tadpole -"));
        assert!(text.contains("Croak [scout]: returned"));
        assert!(!text.contains('\x1b'));
        let text = String::from_utf8(json.writer.clone()).unwrap();
        let records: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1]["event"], "tadpole");
        assert_eq!(records[1]["detail"]["event"], "reasoning_started");
        assert_eq!(records[1]["id"], "scout");
    }

    #[test]
    #[ignore = "opt-in renderer microbenchmark; no inference calls"]
    fn rendering_overhead() {
        let started = Instant::now();
        let mut feedback = Feedback::new(
            io::sink(),
            Style {
                animate: true,
                color: true,
            },
            false,
        );
        feedback.phase("Reasoning");
        for _ in 0..100_000 {
            feedback.tick();
        }
        feedback.finish().unwrap();
        let elapsed = started.elapsed();
        eprintln!(
            "100000 styled spinner frames to sink: {elapsed:?}; {:.2} us/frame",
            elapsed.as_secs_f64() * 1_000_000.0 / 100_000.0
        );
        assert!(
            elapsed.as_secs_f64() / 100_000.0 < 0.001,
            "renderer exceeds 1 ms/frame budget"
        );
    }

    #[test]
    #[ignore = "opt-in actual stderr write benchmark; use a drained PTY"]
    fn terminal_write_overhead() {
        let started = Instant::now();
        let mut feedback = Feedback::new(
            io::stderr(),
            Style {
                animate: true,
                color: true,
            },
            false,
        );
        feedback.phase("Sanborn benchmark");
        for _ in 0..2_000 {
            feedback.tick();
        }
        feedback.finish().unwrap();
        let elapsed = started.elapsed();
        println!(
            "2000 styled frames written to stderr: {elapsed:?}; {:.2} us/frame",
            elapsed.as_secs_f64() * 1_000_000.0 / 2_000.0
        );
        assert!(
            elapsed.as_secs_f64() / 2_000.0 < 0.001,
            "terminal writes exceed 1 ms/frame budget"
        );
    }
}
