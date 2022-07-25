//! Experiment with `tui` to log AL status and general output in two panes.

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ethercrab::{al_status_code::AlStatusCode, client::Client};
use log::LevelFilter;
use smol::LocalExecutor;
use std::{error::Error, io};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame, Terminal,
};
use tui_logger::{init_logger, TuiLoggerLevelOutput, TuiLoggerWidget};

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// Silver USB NIC
const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

struct App {
    state: TableState,
    slaves: Vec<Slave>,
    client: Client<16, 16, smol::Timer>,
}

impl App {
    fn new(client: Client<16, 16, smol::Timer>) -> App {
        log::info!("Creating app");

        App {
            state: TableState::default(),
            slaves: Vec::new(),
            client,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let local_ex = LocalExecutor::new();

    init_logger(LevelFilter::Trace).unwrap();

    let client = Client::new();

    local_ex
        .spawn(client.tx_rx_task(INTERFACE).unwrap())
        .detach();

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new(client);
    let res = run_app(&mut terminal, app);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                // KeyCode::Down => app.next(),
                // KeyCode::Up => app.previous(),
                _ => {}
            }
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App) {
    let rects = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(f.size());

    // Log output
    {
        let tui_w = TuiLoggerWidget::default()
            .block(
                Block::default()
                    .title("Log output")
                    .border_style(Style::default().fg(Color::White).bg(Color::Black))
                    .borders(Borders::ALL),
            )
            .output_separator('|')
            .output_timestamp(Some("%F %H:%M:%S%.3f ".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Long))
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .style_error(Style::default().fg(Color::Red))
            .style_debug(Style::default().fg(Color::Cyan))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_trace(Style::default().fg(Color::White))
            .style_info(Style::default().fg(Color::Green));
        f.render_widget(tui_w, rects[0]);
    }

    // Slave list/status
    {
        let header_cells = ["Slave", "State"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
        let header = Row::new(header_cells).height(1).bottom_margin(1);
        let rows = app.slaves.iter().enumerate().map(|(idx, slave)| {
            Row::new([
                Cell::from(idx.to_string()),
                Cell::from(slave.status_code.to_string()),
            ])
        });

        let t = Table::new(rows)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Slave list"))
            .widths(&[
                Constraint::Percentage(50),
                Constraint::Length(30),
                Constraint::Min(10),
            ]);
        f.render_stateful_widget(t, rects[1], &mut app.state);
    }
}
