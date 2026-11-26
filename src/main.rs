use std::error::Error;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui_image::picker::Picker;

use mr::app::App;
use mr::render;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.get(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: mr <file.md>");
            std::process::exit(2);
        }
    };
    let markdown = match std::fs::read_to_string(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mr: {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    // Query the terminal for its native graphics protocol BEFORE entering raw
    // mode / the alternate screen. Falls back to unicode halfblocks.
    let picker = if io::stdin().is_terminal() && io::stdout().is_terminal() {
        Picker::from_query_stdio().ok()
    } else {
        None
    };

    let mut app = App::new(path, markdown, picker);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));

    let res = run(&mut terminal, &mut app);

    restore_terminal()?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("mr: {e}");
    }
    Ok(())
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    loop {
        terminal.draw(|f| render::draw(f, app))?;

        // Poll with a timeout so transient status messages can expire.
        if event::poll(std::time::Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                        app.quit = true;
                    } else {
                        app.on_key(k);
                    }
                }
                Event::Mouse(m) => app.on_mouse(m),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        app.expire_message();
        if app.quit {
            break;
        }
    }
    Ok(())
}

fn restore_terminal() -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    io::stdout().flush()?;
    Ok(())
}
