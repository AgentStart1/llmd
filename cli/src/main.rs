use anyhow::Context;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use llmd_core::{
    ChatMessage, ChatRequest, ModelProvider, DEFAULT_HOST, DEFAULT_MODEL, DEFAULT_PORT,
};
use llmd_rlitert::RlitertProvider;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io, sync::Arc};

#[derive(Debug, Parser)]
#[command(name = "llmd")]
#[command(about = "Local LiteRT-LM API host for desktop, Android, and terminal clients")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Serve {
        #[arg(long, default_value = DEFAULT_HOST)]
        host: String,
        #[arg(short, long, default_value_t = DEFAULT_PORT)]
        port: u16,
        #[arg(long, default_value_t = 2)]
        pool_size: usize,
    },
    Models,
    Chat {
        prompt: String,
        #[arg(short, long, default_value = DEFAULT_MODEL)]
        model: String,
    },
    Tui,
}

#[cfg(test)]
fn parse_cli_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(args)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llmd=info,llmd_server=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve {
            host,
            port,
            pool_size,
        } => {
            let provider = Arc::new(RlitertProvider::with_pool_size(pool_size).await?);
            llmd_server::serve(provider, &host, port).await?;
        }
        Commands::Models => {
            let provider = RlitertProvider::new().await?;
            for model in provider.list_models().await? {
                println!("{}", model.id);
            }
        }
        Commands::Chat { prompt, model } => {
            let provider = RlitertProvider::new().await?;
            let response = provider
                .chat(ChatRequest {
                    model,
                    messages: vec![ChatMessage {
                        role: "user".to_string(),
                        content: prompt,
                    }],
                    stream: false,
                    max_tokens: None,
                    temperature: None,
                })
                .await?;
            println!("{}", response.content);
        }
        Commands::Tui => run_tui()?,
    }

    Ok(())
}

fn run_tui() -> anyhow::Result<()> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(area);

            let title = Paragraph::new(Line::from("llmd terminal"))
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(title, chunks[0]);

            let body = Paragraph::new(vec![
                Line::from("Desktop and terminal inference uses rlitert-lm."),
                Line::from("Android will use native LiteRT-LM Android integration."),
                Line::from("Press q to quit."),
            ])
            .block(Block::default().borders(Borders::ALL));
            frame.render_widget(body, chunks[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_cli_from, Commands};
    use llmd_core::{DEFAULT_HOST, DEFAULT_MODEL, DEFAULT_PORT};

    #[test]
    fn parses_serve_defaults() {
        let cli = parse_cli_from(["llmd", "serve"]).unwrap();

        match cli.command {
            Commands::Serve {
                host,
                port,
                pool_size,
            } => {
                assert_eq!(host, DEFAULT_HOST);
                assert_eq!(port, DEFAULT_PORT);
                assert_eq!(pool_size, 2);
            }
            _ => panic!("expected serve command"),
        }
    }

    #[test]
    fn parses_serve_overrides() {
        let cli = parse_cli_from([
            "llmd",
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            "8080",
            "--pool-size",
            "4",
        ])
        .unwrap();

        match cli.command {
            Commands::Serve {
                host,
                port,
                pool_size,
            } => {
                assert_eq!(host, "0.0.0.0");
                assert_eq!(port, 8080);
                assert_eq!(pool_size, 4);
            }
            _ => panic!("expected serve command"),
        }
    }

    #[test]
    fn parses_chat_defaults() {
        let cli = parse_cli_from(["llmd", "chat", "hello"]).unwrap();

        match cli.command {
            Commands::Chat { prompt, model } => {
                assert_eq!(prompt, "hello");
                assert_eq!(model, DEFAULT_MODEL);
            }
            _ => panic!("expected chat command"),
        }
    }

    #[test]
    fn parses_models_command() {
        let cli = parse_cli_from(["llmd", "models"]).unwrap();
        assert!(matches!(cli.command, Commands::Models));
    }

    #[test]
    fn parses_tui_command() {
        let cli = parse_cli_from(["llmd", "tui"]).unwrap();
        assert!(matches!(cli.command, Commands::Tui));
    }

    #[test]
    fn rejects_missing_subcommand() {
        let error = parse_cli_from(["llmd"]).unwrap_err();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
