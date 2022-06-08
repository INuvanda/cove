use std::collections::hash_map::Entry;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, MouseEvent};
use crossterm::style::ContentStyle;
use futures::StreamExt;
use parking_lot::FairMutex;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task;
use toss::frame::{Frame, Pos};
use toss::terminal::Terminal;

#[derive(Debug)]
pub enum UiEvent {
    Redraw,
    Term(Event),
}

enum EventHandleResult {
    Continue,
    Stop,
}

pub struct Ui {
    event_tx: UnboundedSender<UiEvent>,
}

impl Ui {
    const POLL_DURATION: Duration = Duration::from_millis(100);

    pub async fn run(terminal: &mut Terminal) -> anyhow::Result<()> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let crossterm_lock = Arc::new(FairMutex::new(()));

        // Prepare and start crossterm event polling task
        let weak_crossterm_lock = Arc::downgrade(&crossterm_lock);
        let event_tx_clone = event_tx.clone();
        let crossterm_event_task = task::spawn_blocking(|| {
            Self::poll_crossterm_events(event_tx_clone, weak_crossterm_lock)
        });

        // Run main UI.
        //
        // If the run_main method exits at any point or if this `run` method is
        // not awaited any more, the crossterm_lock Arc should be deallocated,
        // meaning the crossterm_event_task will also stop after at most
        // `Self::POLL_DURATION`.
        //
        // On the other hand, if the crossterm_event_task stops for any reason,
        // the rest of the UI is also shut down and the client stops.
        let mut ui = Self { event_tx };
        let result = tokio::select! {
            e = ui.run_main(terminal, event_rx, crossterm_lock) => e,
            Ok(e) = crossterm_event_task => e,
        };
        result
    }

    fn poll_crossterm_events(
        tx: UnboundedSender<UiEvent>,
        lock: Weak<FairMutex<()>>,
    ) -> anyhow::Result<()> {
        while let Some(lock) = lock.upgrade() {
            let _guard = lock.lock();
            if crossterm::event::poll(Self::POLL_DURATION)? {
                let event = crossterm::event::read()?;
                tx.send(UiEvent::Term(event))?;
            }
        }
        Ok(())
    }

    async fn run_main(
        &mut self,
        terminal: &mut Terminal,
        mut event_rx: UnboundedReceiver<UiEvent>,
        crossterm_lock: Arc<FairMutex<()>>,
    ) -> anyhow::Result<()> {
        loop {
            // 1. Render current state
            terminal.autoresize()?;
            self.render(terminal.frame()).await?;
            terminal.present()?;

            // 2. Measure widths if required
            if terminal.measuring_required() {
                let _guard = crossterm_lock.lock();
                terminal.measure_widths()?;
                self.event_tx.send(UiEvent::Redraw)?;
            }

            // 3. Handle events (in batches)
            let mut event = match event_rx.recv().await {
                Some(event) => event,
                None => return Ok(()),
            };
            loop {
                let result = match event {
                    UiEvent::Redraw => EventHandleResult::Continue,
                    UiEvent::Term(Event::Key(event)) => self.handle_key_event(event).await,
                    UiEvent::Term(Event::Mouse(event)) => self.handle_mouse_event(event).await?,
                    UiEvent::Term(Event::Resize(_, _)) => EventHandleResult::Continue,
                };
                match result {
                    EventHandleResult::Continue => {}
                    EventHandleResult::Stop => return Ok(()),
                }
                event = match event_rx.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                };
            }
        }
    }

    async fn render(&mut self, frame: &mut Frame) -> anyhow::Result<()> {
        frame.write(Pos::new(0, 0), "Hello world!", ContentStyle::default());

        Ok(())
    }

    async fn handle_key_event(&mut self, event: KeyEvent) -> EventHandleResult {
        match event.code {
            KeyCode::Char('Q') => return EventHandleResult::Stop,
            _ => {}
        }

        EventHandleResult::Continue
    }

    async fn handle_mouse_event(&mut self, event: MouseEvent) -> anyhow::Result<EventHandleResult> {
        Ok(EventHandleResult::Continue)
    }
}
