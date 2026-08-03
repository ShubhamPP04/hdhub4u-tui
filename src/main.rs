use hdhub4u_tui::tui::app::App;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct TerminalGuard;

fn restore_terminal() {
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::Show,
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableFocusChange,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        restore_terminal();
        eprintln!("{}", info);
    }));

    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|arg| arg == "--version" || arg == "-v" || arg == "-V")
    {
        println!("hdhub4u-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let stdout = std::io::stdout();
    let backend =
        ratatui::backend::CrosstermBackend::new(std::io::BufWriter::with_capacity(65536, stdout));
    let mut terminal = ratatui::Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::event::EnableFocusChange
    )?;

    hdhub4u_tui::cache::clean_old_cache_background();

    let mut app = App::new();
    app.run(&mut terminal).await
}
