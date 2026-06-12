#[cfg(feature = "tui")]
mod tui_impl {
    use crate::app::{process_turn_owned, TurnEvent};
    use crate::mode::{AgentMode, ModeState};
    use crate::ui;
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use ratatui::layout::Constraint;
    use ratatui::layout::Layout;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span, Text};
    use ratatui::widgets::Paragraph;
    use ratatui::Frame;
    use tokio::sync::mpsc;

    const BG: Color = Color::Rgb(42, 42, 42);
    const STATUS_BG: Color = Color::Rgb(0, 80, 128);
    const AGENT: Color = Color::Rgb(74, 158, 255);
    const USER: Color = Color::Rgb(74, 255, 74);
    const ORANGE: Color = Color::Rgb(255, 165, 0);
    const DIM: Color = Color::DarkGray;

    fn badge(role: &str) -> Span<'static> {
        let c = match role {
            "agent" => AGENT,
            "user" => USER,
            _ => return Span::raw("   "),
        };
        Span::styled(" ▌ ", Style::new().fg(c).add_modifier(Modifier::BOLD))
    }

    struct AppState {
        chat_lines: Vec<Line<'static>>,
        user_indices: Vec<usize>,
        input: String,
        processing: bool,
        status: String,
        stream_buf: String,
        stream_has_badge: bool,
        scroll: usize,
        auto_scroll: bool,
    }

    fn sanitize(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_ansi = false;
        for c in s.chars() {
            if c == '\x1b' { in_ansi = true; continue; }
            if in_ansi { if c >= '@' && c <= '~' { in_ansi = false; } continue; }
            if c == '\r' || c == '\x00' || (c.is_control() && c != '\n' && c != '\t') { continue; }
            out.push(c);
        }
        out
    }

    impl AppState {
        fn new() -> Self {
            Self { chat_lines: Vec::new(), user_indices: Vec::new(), input: String::new(), processing: false, status: String::new(), stream_buf: String::new(), stream_has_badge: false, scroll: 0, auto_scroll: true }
        }

        fn push(&mut self, line: Line<'static>) {
            // Clean any control chars that survived into chat_lines
            let clean: Vec<Span> = line.spans.into_iter().map(|s| {
                Span::styled(sanitize(&s.content), s.style)
            }).collect();
            self.chat_lines.push(Line::from(clean));
            if self.auto_scroll { self.scroll = 0; }
        }

        fn push_user(&mut self, line: Line<'static>) {
            self.user_indices.push(self.chat_lines.len());
            self.push(line);
        }

        fn stream(&mut self, text: &str) {
            self.stream_buf.push_str(text);
            // Strip carriage returns (Windows \r\n) to avoid cursor-overwrite corruption
            self.stream_buf.retain(|c| c != '\r');
            while let Some(pos) = self.stream_buf.find('\n') {
                let line = self.stream_buf[..pos].to_string();
                self.stream_buf = self.stream_buf[pos + 1..].to_string();
                let mut spans = Vec::new();
                if !self.stream_has_badge { spans.push(badge("agent")); self.stream_has_badge = true; }
                spans.push(Span::raw(line));
                self.push(Line::from(spans));
                self.stream_has_badge = false;
            }
        }

        fn flush_stream(&mut self) {
            if self.stream_buf.is_empty() && !self.stream_has_badge { return; }
            if !self.stream_buf.is_empty() {
                let mut spans = Vec::new();
                if !self.stream_has_badge { spans.push(badge("agent")); }
                spans.push(Span::raw(std::mem::take(&mut self.stream_buf)));
                self.push(Line::from(spans));
            }
            self.stream_has_badge = false;
        }

        fn scroll_up(&mut self, n: usize, visible: usize) {
            self.scroll = (self.scroll + n).min(self.chat_lines.len().saturating_sub(visible));
            self.auto_scroll = self.scroll == 0;
        }

        fn scroll_down(&mut self, n: usize) {
            self.scroll = self.scroll.saturating_sub(n);
            self.auto_scroll = self.scroll == 0;
        }

        fn prev_user(&mut self, visible: usize) {
            let total = self.chat_lines.len();
            let first = total.saturating_sub(self.scroll + visible);
            let idx = self.user_indices.iter().rev().find(|&&i| i < first).copied().unwrap_or(0);
            self.scroll = total.saturating_sub(idx + visible + 1).min(total.saturating_sub(visible));
            self.auto_scroll = false;
        }

        fn next_user(&mut self, visible: usize) {
            let total = self.chat_lines.len();
            let first = total.saturating_sub(self.scroll + visible);
            let idx = self.user_indices.iter().find(|&&i| i > first + 1).copied().unwrap_or(*self.user_indices.last().unwrap_or(&0));
            self.scroll = total.saturating_sub(idx + 1).min(total.saturating_sub(visible));
            self.auto_scroll = self.scroll == 0;
        }
    }

    fn render(frame: &mut Frame, app: &AppState, cfg: &crate::config::Config) {
        let area = frame.area();
        let [chat_area, input_area, status_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // Build visible lines: chat_lines + partial stream line
        let visible = chat_area.height as usize;
        let mut lines = app.chat_lines.clone();
        let partial = sanitize(&app.stream_buf);
        if !partial.is_empty() || app.stream_has_badge {
            let mut spans = Vec::new();
            if !app.stream_has_badge { spans.push(badge("agent")); }
            spans.push(Span::styled(partial, Style::new().fg(DIM)));
            lines.push(Line::from(spans));
        }

        // Slice + pad to exactly visible lines
        let total = lines.len();
        let start = total.saturating_sub(visible + app.scroll);
        let render_count = (total - start).min(visible);
        let mut buf_lines: Vec<Line> = lines.drain(start..).take(render_count).collect();
        buf_lines.resize_with(visible, || Line::from(vec![Span::raw(" ")]));

        // Render: Paragraph with pre-sliced lines, no scroll(), no Wrap (prevents ghosting)
        let plain = Text::from(buf_lines);
        let para = Paragraph::new(plain).style(Style::new().bg(BG));
        frame.render_widget(para, chat_area);

        // Input
        let input = Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::new().fg(AGENT).add_modifier(Modifier::BOLD)),
            Span::raw(sanitize(&app.input)),
        ])).style(Style::new().bg(BG));
        frame.render_widget(input, input_area);

        // Status
        let busy = if app.processing { " ⏳" } else { "" };
        let hint = if !app.auto_scroll && app.scroll > 0 { format!(" ↑{}", app.scroll) } else { String::new() };
        let extra = if app.status.is_empty() { String::new() } else { format!(" | {}", app.status) };
        let text = format!(" Relay | {}{}{}{}", cfg.model, busy, hint, extra);
        let status = Paragraph::new(text).style(Style::new().bg(STATUS_BG).fg(Color::White));
        frame.render_widget(status, status_area);
    }

    pub async fn run(
        cfg: &crate::config::Config,
        session: &mut crate::message::Session,
        _client: &crate::client::ApiClient,
    ) {
        let mut mode = ModeState::new(match cfg.default_mode.as_str() {
            "confirm" => AgentMode::Confirm,
            "plan" => AgentMode::Plan,
            _ => AgentMode::Auto,
        });

        let mut terminal = ratatui::init();
        let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let mut app = AppState::new();

        'main: loop {
            let term_h = terminal.size().map(|s| s.height as usize).unwrap_or(24);
            let visible = term_h.saturating_sub(3);
            app.scroll = app.scroll.min(app.chat_lines.len().saturating_sub(visible));

            while let Ok(event) = turn_rx.try_recv() {
                match event {
                    TurnEvent::Content(t) => app.stream(&t),
                    TurnEvent::ToolCallExecuting { name, args } => {
                        app.flush_stream();
                        app.push(Line::from(vec![
                            Span::styled(" ◆ ", Style::new().fg(ORANGE)),
                            Span::styled(name, Style::new().fg(ORANGE).add_modifier(Modifier::BOLD)),
                            Span::styled(args, Style::new().fg(ORANGE)),
                        ]));
                    }
                    TurnEvent::ToolCallResult { display, success } => {
                        app.flush_stream();
                        let c = if success { DIM } else { Color::Red };
                        app.push(Line::from(Span::styled(display, Style::new().fg(c))));
                    }
                    TurnEvent::Stats { elapsed, tokens, ctx_pct } => {
                        app.push(Line::from(Span::styled(ui::stats_line(elapsed, tokens, ctx_pct), Style::new().fg(DIM))));
                    }
                    TurnEvent::Warning(msg) => {
                        app.push(Line::from(Span::styled(format!(" ⚠ {msg}"), Style::new().fg(Color::Yellow))));
                    }
                    TurnEvent::Error(msg) => {
                        app.push(Line::from(Span::styled(format!(" ✗ {msg}"), Style::new().fg(Color::Red))));
                    }
                    TurnEvent::Done => { app.flush_stream(); app.processing = false; }
                    TurnEvent::ModeChanged { from, to } => {
                        app.status = format!("{:?} → {:?}", from, to);
                    }
                    _ => {}
                }
            }

            if event::poll(std::time::Duration::from_millis(20)).ok() == Some(true) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != crossterm::event::KeyEventKind::Press { continue; }
                    use KeyCode::*;
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    let alt = key.modifiers.contains(KeyModifiers::ALT);
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

                    match key.code {
                        Char(c) if !ctrl && !alt => app.input.push(c),
                        Char('c') if ctrl && !app.processing => break 'main,
                        Enter if !ctrl && !app.processing && !app.input.is_empty() => {
                            let text = std::mem::take(&mut app.input);
                            if text.starts_with('/') {
                                let parts: Vec<&str> = text[1..].split_whitespace().collect();
                                match parts.first().copied() {
                                    Some("exit" | "quit") => break 'main,
                                    Some("clear") => { app.chat_lines.clear(); app.user_indices.clear(); }
                                    Some(cmd @ ("auto" | "confirm" | "plan")) => {
                                        let new = match cmd { "confirm" => AgentMode::Confirm, "plan" => AgentMode::Plan, _ => AgentMode::Auto };
                                        let old = mode.current;
                                        mode.switch_to(new);
                                        session.inject_system_message(&mode.system_prompt_suffix());
                                        app.status = format!("mode: {:?} → {cmd}", old);
                                    }
                                    _ => app.status = format!("unknown: /{}", parts[0]),
                                }
                            } else {
                                let spans = vec![Span::raw(text.clone()), Span::raw("  "), badge("user")];
                                app.push_user(Line::from(spans));
                                session.add_user_message(&text);

                                app.processing = true;
                                let local_cfg = cfg.clone();
                                let api_cfg = cfg.clone();
                                let local_session = std::mem::take(session);
                                let local_mode = std::mem::replace(&mut mode, ModeState::new(AgentMode::Auto));
                                let event_tx = turn_tx.clone();

                                tokio::spawn(async move {
                                    process_turn_owned(local_cfg, local_session, local_mode,
                                        crate::client::ApiClient::new(&api_cfg), text, event_tx,
                                        &mut mpsc::unbounded_channel::<bool>().1).await
                                });
                            }
                        }
                        Enter if ctrl => app.input.push('\n'),
                        Backspace => { app.input.pop(); }
                        Esc if !app.processing => break 'main,
                        Up => app.scroll_up(1, visible),
                        Down => app.scroll_down(1),
                        PageUp if shift => app.prev_user(visible),
                        PageDown if shift => app.next_user(visible),
                        PageUp => app.scroll_up(visible, visible),
                        PageDown => app.scroll_down(visible),
                        _ => {}
                    }
                }
            }

            let _ = terminal.draw(|f| render(f, &app, cfg));
        }

        let _ = ratatui::restore();
    }
}

#[cfg(feature = "tui")]
pub async fn run_tui(
    cfg: &crate::config::Config,
    session: &mut crate::message::Session,
    client: &crate::client::ApiClient,
) {
    tui_impl::run(cfg, session, client).await;
}

#[cfg(not(feature = "tui"))]
pub async fn run_tui(
    _cfg: &crate::config::Config,
    _session: &mut crate::message::Session,
    _client: &crate::client::ApiClient,
) {
    println!("TUI mode not available (compile with 'tui' feature). Use --no-tui for terminal mode.");
}
