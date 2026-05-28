#[cfg(feature = "tui")]
mod tui_impl {
    use crate::app::{process_turn_owned, TurnEvent};
    use crate::mode::{AgentMode, ModeState};
    use crate::ui;
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
    use ratatui::text::Text;
    use ratatui::Frame;
    use tokio::sync::mpsc;

    const AGENT_BLUE: Color = Color::Rgb(74, 158, 255);
    const USER_GREEN: Color = Color::Rgb(74, 255, 74);
    const ORANGE: Color = Color::Rgb(255, 165, 0);

    #[allow(unused)]
    fn agent_avatar(size: usize) -> Vec<Line<'static>> {
        let c = "█".repeat(size);
        let r = format!("{}{}{}", "█".repeat((size - 1).max(0)), "R", "█".repeat(size.saturating_sub(2)));
        vec![
            Line::from(Span::styled(c.clone(), Style::new().fg(AGENT_BLUE))),
            Line::from(Span::styled(r, Style::new().fg(AGENT_BLUE))),
            Line::from(Span::styled(c, Style::new().fg(AGENT_BLUE))),
        ]
    }

    #[allow(unused)]
    fn user_avatar(size: usize) -> Vec<Line<'static>> {
        let ul = "█".repeat(size);
        let um = format!("{}{}{}", "█".repeat((size - 1).max(0)), "U", "█".repeat(size.saturating_sub(2)));
        vec![
            Line::from(Span::styled(ul.clone(), Style::new().fg(USER_GREEN))),
            Line::from(Span::styled(um, Style::new().fg(USER_GREEN))),
            Line::from(Span::styled(ul, Style::new().fg(USER_GREEN))),
        ]
    }

    fn prefix_badge(role: &str) -> Span<'static> {
        match role {
            "agent" => Span::styled(" ▌ ", Style::new().fg(AGENT_BLUE).add_modifier(Modifier::BOLD)),
            "user" => Span::styled(" ▌ ", Style::new().fg(USER_GREEN).add_modifier(Modifier::BOLD)),
            _ => Span::raw("   "),
        }
    }

    struct AppState {
        chat_lines: Vec<Line<'static>>,
        user_msg_indices: Vec<usize>,
        input: String,
        processing: bool,
        status_text: String,
        content_buf: String,
        scroll_offset: usize,
        auto_scroll: bool,
    }

    impl AppState {
        fn new() -> Self {
            Self {
                chat_lines: Vec::new(),
                user_msg_indices: Vec::new(),
                input: String::new(),
                processing: false,
                status_text: String::new(),
                content_buf: String::new(),
                scroll_offset: 0,
                auto_scroll: true,
            }
        }

        fn add_line(&mut self, line: Line<'static>) {
            self.chat_lines.push(line);
            if self.auto_scroll {
                self.scroll_offset = 0;
            }
        }

        fn add_user_line(&mut self, line: Line<'static>) {
            self.user_msg_indices.push(self.chat_lines.len());
            self.chat_lines.push(line);
            if self.auto_scroll {
                self.scroll_offset = 0;
            }
        }

        fn flush_content(&mut self) {
            if !self.content_buf.is_empty() {
                let text = std::mem::take(&mut self.content_buf);
                for (i, segment) in text.split('\n').enumerate() {
                    let mut spans = Vec::new();
                    if i == 0 {
                        spans.push(prefix_badge("agent"));
                    }
                    spans.push(Span::raw(segment.to_string()));
                    self.add_line(Line::from(spans));
                }
            }
        }

        fn buffer_content(&mut self, text: &str) {
            self.content_buf.push_str(text);
        }

        fn scroll_up(&mut self, amount: usize, visible: usize) {
            let max_scroll = self.chat_lines.len().saturating_sub(visible);
            self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
            self.auto_scroll = self.scroll_offset == 0;
        }

        fn scroll_down(&mut self, amount: usize) {
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
            self.auto_scroll = self.scroll_offset == 0;
        }

        fn prev_user_msg(&mut self, visible: usize) {
            if self.user_msg_indices.is_empty() { return; }
            let total = self.chat_lines.len();
            let first_visible = total.saturating_sub(self.scroll_offset + visible);
            let idx = self.user_msg_indices.iter().rev()
                .find(|&&i| i < first_visible)
                .copied()
                .unwrap_or(self.user_msg_indices[0]);
            self.scroll_offset = total.saturating_sub(idx + visible + 1).min(total.saturating_sub(visible));
            self.auto_scroll = false;
        }

        fn next_user_msg(&mut self, visible: usize) {
            if self.user_msg_indices.is_empty() { return; }
            let total = self.chat_lines.len();
            let first_visible = total.saturating_sub(self.scroll_offset + visible);
            let idx = self.user_msg_indices.iter()
                .find(|&&i| i > first_visible + 1)
                .copied()
                .unwrap_or(*self.user_msg_indices.last().unwrap_or(&0));
            self.scroll_offset = total.saturating_sub(idx + 1).min(total.saturating_sub(visible));
            self.auto_scroll = self.scroll_offset == 0;
        }
    }

    fn render(frame: &mut Frame, app: &AppState, cfg: &crate::config::Config) {
        let area = frame.area();
        let [chat_area, input_area, status_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

        // Chat area — build live text from chat_lines + streaming buffer
        frame.render_widget(Clear, chat_area);
        let visible = chat_area.height as usize;
        let mut display_lines = app.chat_lines.clone();
        if !app.content_buf.is_empty() {
            for (i, line) in app.content_buf.split('\n').enumerate() {
                let mut spans = vec![Span::styled(if i == 0 { " ▌ " } else { "    " },
                    Style::new().fg(AGENT_BLUE).add_modifier(Modifier::BOLD))];
                spans.push(Span::styled(line.to_string(), Style::new().fg(Color::DarkGray)));
                display_lines.push(Line::from(spans));
            }
        }
        let total = display_lines.len();
        let scroll_lines = total.saturating_sub(visible + app.scroll_offset);
        let text = Text::from(display_lines);
        // Use style to ensure old cells are overwritten
        let bg_style = Style::new().bg(Color::Rgb(42, 42, 42));
        let chat = Paragraph::new(text)
            .style(bg_style)
            .wrap(Wrap { trim: false })
            .scroll((scroll_lines as u16, 0));
        frame.render_widget(chat, chat_area);

        // Input
        let input = Paragraph::new(app.input.as_str())
            .block(Block::default().borders(Borders::ALL).title(" Input "));
        frame.render_widget(input, input_area);

        // Status bar
        let mode_indicator = if app.processing { " ⏳ " } else { "" };
        let scroll_hint = if !app.auto_scroll && app.scroll_offset > 0 {
            format!(" ↑{} ", app.scroll_offset)
        } else { String::new() };
        let status = format!(" Relay | {}{}{}{}", cfg.model, mode_indicator, scroll_hint, app.status_text);
        let status_widget = Paragraph::new(status)
            .style(Style::new().bg(Color::Rgb(0, 80, 128)).fg(Color::White));
        frame.render_widget(status_widget, status_area);
    }

    pub async fn run(
        cfg: &crate::config::Config,
        session: &mut crate::message::Session,
        _client: &crate::client::ApiClient,
    ) {
        let mut mode = ModeState::new(
            match cfg.default_mode.as_str() {
                "confirm" => AgentMode::Confirm,
                "plan" => AgentMode::Plan,
                _ => AgentMode::Auto,
            }
        );

        let mut terminal = ratatui::init();
        let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let mut app = AppState::new();

        'main: loop {
            // Check for resize and get terminal size
            let term_h = terminal.size().map(|s| s.height as usize).unwrap_or(24);
            let visible_lines = term_h.saturating_sub(5);
            // Clamp scroll_offset after resize
            let max_scroll = app.chat_lines.len().saturating_sub(visible_lines);
            app.scroll_offset = app.scroll_offset.min(max_scroll);

            // Drain turn events
            while let Ok(event) = turn_rx.try_recv() {
                match event {
                    TurnEvent::Content(t) => app.buffer_content(&t),
                    TurnEvent::ToolCallExecuting { name, args } => {
                        app.flush_content();
                        app.add_line(Line::from(Span::raw("")));
                        app.add_line(Line::from(vec![
                            Span::styled(" ◆ ", Style::new().fg(Color::Rgb(255, 165, 0))),
                            Span::styled(name, Style::new().fg(Color::Rgb(255, 165, 0)).add_modifier(Modifier::BOLD)),
                            Span::styled(format!("({})", args), Style::new().fg(Color::Rgb(255, 165, 0))),
                        ]));
                    }
                    TurnEvent::ToolCallResult { display, success } => {
                        app.flush_content();
                        if success {
                            app.add_line(Line::from(Span::styled(display, Style::new().fg(Color::DarkGray))));
                        } else {
                            app.add_line(Line::from(Span::styled(format!(" ✗ {}", display), Style::new().fg(Color::Red))));
                        }
                    }
                    TurnEvent::Stats { elapsed, tokens, ctx_pct } => {
                        app.add_line(Line::from(Span::styled(
                            ui::stats_line(elapsed, tokens, ctx_pct), Style::new().fg(Color::DarkGray),
                        )));
                    }
                    TurnEvent::Warning(msg) => {
                        app.add_line(Line::from(Span::styled(format!(" ⚠ {}", msg), Style::new().fg(Color::Yellow))));
                    }
                    TurnEvent::Error(msg) => {
                        app.add_line(Line::from(Span::styled(format!(" ✗ {}", msg), Style::new().fg(Color::Red))));
                    }
                    TurnEvent::Done => { app.flush_content(); app.processing = false; }
                    TurnEvent::ModeChanged { from, to } => {
                        app.status_text = format!("{:?} → {:?}", from, to);
                    }
                    _ => {}
                }
            }

            // Keyboard
            if event::poll(std::time::Duration::from_millis(20)).ok() == Some(true) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != crossterm::event::KeyEventKind::Press { continue; }
                    use KeyCode::*;
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    let alt = key.modifiers.contains(KeyModifiers::ALT);

                    match key.code {
                        Char(c) if !ctrl && !alt && !shift => app.input.push(c),
                        Char('c') if ctrl && !app.processing => break 'main,
                        Enter if !app.processing && !app.input.is_empty() => {
                            let text = std::mem::take(&mut app.input);
                            if text.starts_with('/') {
                                let parts: Vec<&str> = text[1..].split_whitespace().collect();
                                match parts.first().map(|s| *s) {
                                    Some("exit") | Some("quit") => break 'main,
                                    Some("clear") => { app.chat_lines.clear(); app.user_msg_indices.clear(); }
                                    Some("auto") => {
                                        let old = mode.current;
                                        mode.switch_to(AgentMode::Auto);
                                        session.inject_system_message(&mode.system_prompt_suffix());
                                        app.status_text = format!("mode: {:?} → Auto", old);
                                    }
                                    Some("confirm") => {
                                        let old = mode.current;
                                        mode.switch_to(AgentMode::Confirm);
                                        session.inject_system_message(&mode.system_prompt_suffix());
                                        app.status_text = format!("mode: {:?} → Confirm", old);
                                    }
                                    Some("plan") => {
                                        let old = mode.current;
                                        mode.switch_to(AgentMode::Plan);
                                        session.inject_system_message(&mode.system_prompt_suffix());
                                        app.status_text = format!("mode: {:?} → Plan", old);
                                    }
                                    _ => app.status_text = format!("unknown: /{}", parts.first().unwrap_or(&"")),
                                }
                            } else {
                                app.add_user_line(Line::from(vec![
                                    Span::raw(text.clone()),
                                    Span::raw("  "),
                                    prefix_badge("user"),
                                ]));
                                session.add_user_message(&text);

                                app.processing = true;
                                let local_cfg = cfg.clone();
                                let api_cfg = cfg.clone();
                                let local_session = std::mem::take(session);
                                let local_mode = std::mem::replace(&mut mode, ModeState::new(AgentMode::Auto));
                                let event_tx = turn_tx.clone();

                                tokio::spawn(async move {
                                    process_turn_owned(
                                        local_cfg, local_session, local_mode,
                                        crate::client::ApiClient::new(&api_cfg),
                                        text, event_tx,
                                        &mut mpsc::unbounded_channel::<bool>().1,
                                    ).await
                                });
                            }
                        }
                        Backspace => { app.input.pop(); }
                        Esc if !app.processing => break 'main,
                        Up => app.scroll_up(1, visible_lines),
                        Down => app.scroll_down(1),
                        PageUp if shift => app.prev_user_msg(visible_lines),
                        PageDown if shift => app.next_user_msg(visible_lines),
                        PageUp => app.scroll_up(visible_lines, visible_lines),
                        PageDown => app.scroll_down(visible_lines),
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
