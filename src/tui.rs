#[cfg(feature = "tui")]
mod tui_impl {
    use crate::app::{process_turn_owned, TurnEvent};
    use crate::mode::{AgentMode, ModeState};
    use crate::ui;
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
    use ratatui::Frame;
    use tokio::sync::mpsc;

    struct AppState {
        chat_lines: Vec<Line<'static>>,
        input: String,
        processing: bool,
        status_text: String,
        content_buf: String,
    }

    impl AppState {
        fn new() -> Self {
            Self {
                chat_lines: Vec::new(),
                input: String::new(),
                processing: false,
                status_text: String::new(),
                content_buf: String::new(),
            }
        }

        fn add_line(&mut self, line: Line<'static>) {
            self.chat_lines.push(line);
        }

        /// Flush buffered content as a single line.
        fn flush_content(&mut self) {
            if !self.content_buf.is_empty() {
                let text = std::mem::take(&mut self.content_buf);
                self.add_line(Line::from(ratatui::text::Span::raw(text)));
            }
        }

        /// Buffer streaming content text.
        fn buffer_content(&mut self, text: &str) {
            self.content_buf.push_str(text);
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

        // Chat list
        let items: Vec<ListItem> = app.chat_lines.iter().map(|l| ListItem::new(l.clone())).collect();
        let chat = List::new(items).block(Block::default());
        frame.render_widget(chat, chat_area);

        // Input
        let input = Paragraph::new(app.input.as_str())
            .block(Block::default().borders(Borders::ALL).title(" Input "));
        frame.render_widget(input, input_area);

        // Status
        let status = if app.processing {
            format!(" Relay | {} | ⏳ processing ", cfg.model)
        } else {
            format!(" Relay | {} ", cfg.model)
        };
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

        // Setup terminal
        let mut terminal = ratatui::init();

        let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let mut app = AppState::new();
        let mut tick_count = 0u64;

        'main: loop {
            // Drain turn events
            while let Ok(event) = turn_rx.try_recv() {
                match event {
                    TurnEvent::Content(t) => app.buffer_content(&t),
                    TurnEvent::ToolCallExecuting { name, args } => {
                        app.flush_content();
                        app.add_line(Line::from(Span::raw(""))); // blank separator
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
                    TurnEvent::Done => { app.flush_content(); app.processing = false; },
                    TurnEvent::ModeChanged { from, to } => {
                        app.status_text = format!("{:?} → {:?}", from, to);
                    }
                    _ => {}
                }
            }

            // Handle keyboard (poll non-blocking)
            if event::poll(std::time::Duration::from_millis(20)).ok() == Some(true) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != crossterm::event::KeyEventKind::Press { continue; }
                    use KeyCode::*;
                    match key.code {
                        Char(c) if key.modifiers == KeyModifiers::NONE => app.input.push(c),
                        Char('c') if key.modifiers == KeyModifiers::CONTROL && !app.processing => break 'main,
                        Enter if !app.processing && !app.input.is_empty() => {
                            let text = std::mem::take(&mut app.input);
                            if text.starts_with('/') {
                                let parts: Vec<&str> = text[1..].split_whitespace().collect();
                                match parts.first().map(|s| *s) {
                                    Some("exit") | Some("quit") => break 'main,
                                    Some("clear") => app.chat_lines.clear(),
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
                                app.add_line(Line::from(Span::styled(
                                    format!(" │ {} ", text), Style::new().fg(Color::Green),
                                )));
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
                        _ => {}
                    }
                }
            }

            // Render
            let _ = terminal.draw(|f| render(f, &app, cfg));

            tick_count += 1;
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
